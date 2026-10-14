use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalPosition, Manager};
use transcribe_rs::transcriber::VadChunked;

use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::audio::{Recorder, TailResampler};
use crate::cleanup;
use crate::context::{frontmost_app, FrontmostApp};
use crate::inject::Injector;
use crate::settings::SettingsStore;
use crate::stt::Transcriber;

/// How often the streaming-ASR poll task drains newly-captured tail samples
/// during `Recording`, matching the design's "poll-based, reuse the existing
/// buffer" decision (no new realtime channel out of the cpal callback).
const STREAMING_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(200);

/// Per-recording streaming-ASR state: a `VadChunked` session fed a tail poll
/// at a time during `Recording`, so most of the audio is already
/// chunk-transcribed by the time `Stop` fires — that's the tail-latency win.
struct StreamingSession {
    vad: VadChunked,
    resampler: TailResampler,
    /// Position into the recorder's native-rate sample buffer already fed.
    cursor: usize,
    poll_handle: Option<tauri::async_runtime::JoinHandle<()>>,
}

impl Drop for StreamingSession {
    fn drop(&mut self) {
        if let Some(handle) = self.poll_handle.take() {
            handle.abort();
        }
    }
}

/// Recordings shorter than this are treated as accidental taps.
const MIN_SAMPLES: usize = 4800; // 0.3s at 16kHz

#[derive(Clone, Copy, PartialEq, Debug)]
enum Stage {
    Idle,
    Recording,
    Processing,
}

#[derive(Clone, Serialize)]
pub struct PipelineEvent {
    pub state: String, // idle | recording | transcribing | cleaning | pasting | error
    pub detail: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct DictationResult {
    pub raw_text: String,
    pub final_text: String,
    pub cleaned: bool,
    pub app_name: Option<String>,
    pub app_bundle_id: Option<String>,
    pub audio_seconds: f64,
    pub stt_ms: u128,
    pub llm_ms: Option<u128>,
}

pub struct Pipeline {
    stage: Mutex<Stage>,
    /// App that was focused when recording started — captured at press time,
    /// since processing happens later and focus may have moved.
    target_app: Mutex<Option<FrontmostApp>>,
    /// Clipboard text snapshotted at recording start, for spoken "in the clipboard" substitution.
    clipboard_text: Mutex<Option<String>>,
    /// Live streaming-ASR session for the current recording, `None` when idle.
    streaming: Mutex<Option<StreamingSession>>,
}

fn emit_state(app: &AppHandle, state: &str, detail: Option<String>) {
    let _ = app.emit(
        "pipeline-state",
        PipelineEvent {
            state: state.to_string(),
            detail,
        },
    );
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = match state {
            "idle" | "error" => overlay.hide(),
            _ => {
                position_overlay_near_dock(app, &overlay);
                overlay.show()
            }
        };
    }
}

/// Replace spoken clipboard-reference phrases with the actual clipboard text.
/// Longest phrases are checked first to avoid partial matches.
fn substitute_clipboard(text: &str, clipboard: &str) -> String {
    let triggers = [
        "paste from clipboard",
        "from the clipboard",
        "in the clipboard",
        "paste clipboard",
        "from clipboard",
        "in clipboard",
    ];
    let lower = text.to_lowercase();
    for trigger in &triggers {
        if let Some(pos) = lower.find(trigger) {
            return format!("{}{}{}", &text[..pos], clipboard, &text[pos + trigger.len()..]);
        }
    }
    text.to_string()
}

/// Position the overlay centered horizontally, just above the dock area.
fn position_overlay_near_dock(app: &AppHandle, overlay: &tauri::WebviewWindow) {
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let screen_w = monitor.size().width as f64 / scale;
        let screen_h = monitor.size().height as f64 / scale;
        // 200×44 matches the overlay window size in tauri.conf.json.
        let x = (screen_w - 200.0) / 2.0;
        // ~120 logical px from the bottom sits just above a standard-size dock.
        let y = screen_h - 44.0 - 120.0;
        let _ = overlay.set_position(LogicalPosition::new(x, y));
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            stage: Mutex::new(Stage::Idle),
            target_app: Mutex::new(None),
            clipboard_text: Mutex::new(None),
            streaming: Mutex::new(None),
        }
    }

    /// Entry point from the global shortcut handler.
    pub fn on_hotkey(&self, app: &AppHandle, pressed: bool) {
        let mode = app.state::<SettingsStore>().get().activation_mode;
        let push_to_talk = mode != "toggle";

        let stage = *self.stage.lock().unwrap();
        match stage {
            Stage::Idle => {
                if pressed {
                    self.start(app);
                }
            }
            Stage::Recording => {
                if (push_to_talk && !pressed) || (!push_to_talk && pressed) {
                    self.stop_and_process(app);
                }
            }
            Stage::Processing => {} // ignore input while busy
        }
    }

    fn start(&self, app: &AppHandle) {
        let transcriber = app.state::<Transcriber>();
        if !transcriber.is_loaded() {
            emit_state(
                app,
                "error",
                Some("Speech model still loading — try again shortly".into()),
            );
            return;
        }
        *self.target_app.lock().unwrap() = frontmost_app();
        *self.clipboard_text.lock().unwrap() = app.clipboard().read_text().ok();

        // Instant feedback: show the overlay before the mic stream spins up.
        emit_state(app, "recording", None);
        let recorder = app.state::<Recorder>();
        match recorder.start() {
            Ok(()) => {
                *self.stage.lock().unwrap() = Stage::Recording;
                self.start_streaming(app, &recorder);
            }
            Err(e) => {
                log::error!("recording start failed: {e}");
                self.finish(app, Some(format!("Mic error: {e}")));
            }
        }
    }

    /// Spin up this recording's streaming-ASR session: a fresh `VadChunked`
    /// plus a poll task that drains and chunk-transcribes new tail samples
    /// every ~200ms, so most of the audio is done by the time `Stop` fires.
    fn start_streaming(&self, app: &AppHandle, recorder: &Recorder) {
        let resampler = match TailResampler::new(recorder.input_sample_rate()) {
            Ok(r) => r,
            Err(e) => {
                // Best-effort: skip streaming for this recording, whole-utterance
                // fallback still happens naturally since finish_streaming just
                // gets nothing fed and transcribes the (empty) remainder — so
                // instead we simply don't spawn a session here.
                log::warn!("streaming ASR: resampler init failed, skipping for this recording: {e}");
                return;
            }
        };
        let device_name = recorder.current_device_name();
        let settings = app.state::<SettingsStore>().get();
        let threshold = settings
            .mic_calibration
            .get(&device_name)
            .copied()
            .unwrap_or(crate::stt::STREAMING_VAD_RMS_THRESHOLD);

        *self.streaming.lock().unwrap() = Some(StreamingSession {
            vad: crate::stt::new_streaming_vad(threshold),
            resampler,
            cursor: 0,
            poll_handle: None,
        });

        let app = app.clone();
        let handle = tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(STREAMING_POLL_INTERVAL);
            loop {
                interval.tick().await;
                let pipeline = app.state::<Pipeline>();
                let recorder = app.state::<Recorder>();
                let transcriber = app.state::<Transcriber>();

                // Mid-recording device unplug: cpal already delivers this to
                // the stream's error callback, but only logged it — surface
                // it as a normal pipeline error and abort the recording
                // rather than silently discarding it until the next press.
                if let Some(msg) = recorder.take_stream_error() {
                    pipeline.streaming.lock().unwrap().take();
                    let _ = recorder.stop();
                    pipeline.finish(&app, Some(msg));
                    return;
                }

                let mut guard = pipeline.streaming.lock().unwrap();
                let Some(session) = guard.as_mut() else {
                    return; // session already taken by stop_and_process
                };
                let tail = recorder.drain_since(&mut session.cursor);
                if tail.is_empty() {
                    continue;
                }
                match session.resampler.push(&tail) {
                    Ok(resampled) if !resampled.is_empty() => {
                        if let Err(e) = transcriber.feed_streaming(&mut session.vad, &resampled) {
                            log::warn!("streaming ASR feed failed: {e}");
                        }
                    }
                    Ok(_) => {}
                    Err(e) => log::warn!("streaming ASR resample failed: {e}"),
                }
            }
        });
        if let Some(session) = self.streaming.lock().unwrap().as_mut() {
            session.poll_handle = Some(handle);
        }
    }

    fn stop_and_process(&self, app: &AppHandle) {
        *self.stage.lock().unwrap() = Stage::Processing;
        emit_state(app, "transcribing", None);

        let recorder = app.state::<Recorder>();
        // Take the streaming session and grab its final not-yet-polled tail
        // before `recorder.stop()` clears the native-rate buffer.
        let mut session = self.streaming.lock().unwrap().take();
        let tail_native = session
            .as_mut()
            .map(|s| recorder.drain_since(&mut s.cursor))
            .unwrap_or_default();

        let samples = match recorder.stop() {
            Ok(s) => s,
            Err(e) => {
                log::error!("recording stop failed: {e}");
                self.finish(app, Some(format!("Mic error: {e}")));
                return;
            }
        };
        if samples.len() < MIN_SAMPLES {
            log::info!("recording too short ({} samples), discarding", samples.len());
            self.finish(app, None);
            return;
        }

        let target_app = self.target_app.lock().unwrap().clone();
        let clipboard_text = self.clipboard_text.lock().unwrap().clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = process(
                &app,
                samples,
                session,
                tail_native,
                target_app,
                clipboard_text,
            )
            .await;
            let pipeline = app.state::<Pipeline>();
            match outcome {
                Ok(result) => {
                    let _ = app.emit("dictation-complete", &result);
                    record_history(&app, &result);
                    pipeline.finish(&app, None);
                }
                Err(e) => {
                    log::error!("pipeline failed: {e}");
                    pipeline.finish(&app, Some(e.to_string()));
                }
            }
        });
    }

    fn finish(&self, app: &AppHandle, error: Option<String>) {
        *self.stage.lock().unwrap() = Stage::Idle;
        match error {
            Some(e) => {
                emit_state(app, "error", Some(e));
                // Leave the error visible briefly, then clear.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    emit_state(&app, "idle", None);
                });
            }
            None => emit_state(app, "idle", None),
        }
    }
}

/// ponytail: threshold = half the measured speech RMS, clamped — a simple,
/// untuned starting fraction (like `stt::STREAMING_VAD_RMS_THRESHOLD` itself),
/// not derived; revisit if calibrated devices still misfire.
fn calibrated_threshold(speech_rms: f32) -> f32 {
    (speech_rms * 0.5).clamp(0.002, crate::stt::STREAMING_VAD_RMS_THRESHOLD)
}

/// Persist a per-device streaming-VAD threshold derived from a real speech
/// RMS measurement (see `process`'s whole-utterance retry).
fn calibrate_mic_threshold(app: &AppHandle, speech_rms: f32) {
    let recorder = app.state::<Recorder>();
    let device = recorder.current_device_name();
    if device.is_empty() {
        return;
    }
    let threshold = calibrated_threshold(speech_rms);
    let store = app.state::<SettingsStore>();
    let mut settings = store.get();
    settings.mic_calibration.insert(device.clone(), threshold);
    match store.set(settings) {
        Ok(()) => log::info!(
            "mic calibration: {device:?} -> threshold {threshold:.4} (measured speech rms {speech_rms:.4})"
        ),
        Err(e) => log::warn!("failed to persist mic calibration for {device:?}: {e}"),
    }
}

fn record_history(app: &AppHandle, result: &DictationResult) {
    let settings = app.state::<SettingsStore>().get();
    if settings.history_retention == "never" {
        return;
    }
    let store = app.state::<crate::store::Store>();
    match store.record_dictation(result) {
        Ok(newly_unlocked) => {
            if !newly_unlocked.is_empty() {
                let _ = app.emit("achievements-unlocked", &newly_unlocked);
            }
            let _ = store.apply_retention(&settings.history_retention);
        }
        Err(e) => log::error!("failed to record history: {e}"),
    }
}

async fn process(
    app: &AppHandle,
    samples: Vec<f32>,
    session: Option<StreamingSession>,
    tail_native: Vec<f32>,
    target_app: Option<FrontmostApp>,
    clipboard_text: Option<String>,
) -> anyhow::Result<DictationResult> {
    let audio_seconds = samples.len() as f64 / crate::audio::TARGET_SAMPLE_RATE as f64;

    // STT runs on a blocking thread; the engine call is CPU-bound. Most of a
    // long recording is already chunk-transcribed by the streaming session's
    // poll task — this just feeds the final tail and merges the results.
    // `session` is only `None` if starting the streaming session failed at
    // recording start (e.g. resampler init); fall back to whole-utterance.
    // Kept for a possible whole-utterance retry below — only the streaming
    // path below empties `samples` into the blocking closure.
    let used_streaming = session.is_some();
    let retry_samples = if used_streaming { Some(samples.clone()) } else { None };

    let (raw_text, stt_ms) = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<(String, u128)> {
            let transcriber = app.state::<Transcriber>();
            match session {
                Some(mut session) => {
                    let mut tail_16k = session.resampler.push(&tail_native)?;
                    tail_16k.extend(session.resampler.flush()?);
                    transcriber.feed_streaming(&mut session.vad, &tail_16k)?;
                    transcriber.finish_streaming(&mut session.vad)
                }
                None => transcriber.transcribe(&samples),
            }
        })
        .await??
    };
    // Never log dictated text: the log file persists on disk.
    log::info!("STT ({stt_ms}ms): {} chars", raw_text.chars().count());

    // Streaming VAD found nothing — retry with whole-utterance transcription,
    // which doesn't depend on `STREAMING_VAD_RMS_THRESHOLD`. Success means
    // genuine speech was captured but the VAD missed it (gain/distance
    // mismatch, not silence); back-fill this device's calibrated threshold
    // from the real speech RMS so subsequent presses on it self-heal.
    let raw_text = if raw_text.is_empty() {
        match retry_samples {
            Some(retry_samples) => {
                log::warn!("streaming ASR returned nothing, retrying whole-utterance transcription");
                let rms = (retry_samples.iter().map(|s| s * s).sum::<f32>()
                    / retry_samples.len().max(1) as f32)
                    .sqrt();
                let retry_app = app.clone();
                let (retried_text, _) = tauri::async_runtime::spawn_blocking(
                    move || -> anyhow::Result<(String, u128)> {
                        let transcriber = retry_app.state::<Transcriber>();
                        transcriber.transcribe(&retry_samples)
                    },
                )
                .await??;
                if !retried_text.is_empty() {
                    log::info!(
                        "whole-utterance retry recovered: {} chars",
                        retried_text.chars().count()
                    );
                    calibrate_mic_threshold(app, rms);
                }
                retried_text
            }
            None => raw_text,
        }
    } else {
        raw_text
    };
    if raw_text.is_empty() {
        return Err(anyhow::anyhow!("Nothing transcribed"));
    }

    let settings = app.state::<SettingsStore>().get();
    let transcript = if settings.clipboard_substitution_enabled {
        match clipboard_text.as_deref() {
            Some(clip) if !clip.trim().is_empty() => substitute_clipboard(&raw_text, clip),
            _ => raw_text.clone(),
        }
    } else {
        raw_text.clone()
    };

    let (final_text, cleaned, llm_ms) = if settings.cleanup_level != "none" {
        emit_state(app, "cleaning", None);
        match cleanup::clean_transcript(&settings, target_app.as_ref(), &transcript).await {
            Ok(result) => {
                log::info!(
                    "cleanup ({}ms, {:.0} tok/s): {} chars",
                    result.llm_ms,
                    result.tokens_per_sec.unwrap_or(0.0),
                    result.text.chars().count()
                );
                (result.text, true, Some(result.llm_ms))
            }
            Err(e) => {
                // Cleanup is best-effort: fall back to the substituted transcript.
                log::warn!("cleanup failed, pasting raw transcript: {e}");
                (transcript.clone(), false, None)
            }
        }
    } else {
        (transcript.clone(), false, None)
    };

    emit_state(app, "pasting", None);
    {
        let app = app.clone();
        let text = final_text.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let injector = app.state::<Injector>();
            injector.paste(&app, &text)
        })
        .await??;
    }

    Ok(DictationResult {
        raw_text,
        final_text,
        cleaned,
        app_name: target_app.as_ref().map(|a| a.name.clone()),
        app_bundle_id: target_app.as_ref().map(|a| a.bundle_id.clone()),
        audio_seconds,
        stt_ms,
        llm_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::calibrated_threshold;

    #[test]
    fn calibrated_threshold_halves_measured_speech_rms() {
        assert!((calibrated_threshold(0.01) - 0.005).abs() < 1e-6);
    }

    #[test]
    fn calibrated_threshold_never_exceeds_static_default() {
        // A very loud retry shouldn't push the calibrated threshold above
        // today's known-safe default — calibration should only ever help.
        assert_eq!(calibrated_threshold(10.0), crate::stt::STREAMING_VAD_RMS_THRESHOLD);
    }

    #[test]
    fn calibrated_threshold_has_a_floor() {
        // A near-silent "successful" retry (e.g. a very quiet whisper) still
        // yields a usable, non-degenerate threshold.
        assert_eq!(calibrated_threshold(0.0), 0.002);
    }
}
