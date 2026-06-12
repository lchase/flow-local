use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::Recorder;
use crate::cleanup;
use crate::context::{frontmost_app, FrontmostApp};
use crate::inject::Injector;
use crate::settings::SettingsStore;
use crate::stt::Transcriber;

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
            _ => overlay.show(),
        };
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            stage: Mutex::new(Stage::Idle),
            target_app: Mutex::new(None),
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

        let recorder = app.state::<Recorder>();
        match recorder.start() {
            Ok(()) => {
                *self.stage.lock().unwrap() = Stage::Recording;
                emit_state(app, "recording", None);
            }
            Err(e) => {
                log::error!("recording start failed: {e}");
                emit_state(app, "error", Some(format!("Mic error: {e}")));
            }
        }
    }

    fn stop_and_process(&self, app: &AppHandle) {
        *self.stage.lock().unwrap() = Stage::Processing;
        emit_state(app, "transcribing", None);

        let recorder = app.state::<Recorder>();
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
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = process(&app, samples, target_app).await;
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
    target_app: Option<FrontmostApp>,
) -> anyhow::Result<DictationResult> {
    let audio_seconds = samples.len() as f64 / crate::audio::TARGET_SAMPLE_RATE as f64;

    // STT runs on a blocking thread; the parakeet engine call is CPU-bound.
    let (raw_text, stt_ms) = {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let transcriber = app.state::<Transcriber>();
            transcriber.transcribe(&samples)
        })
        .await??
    };
    log::info!("STT ({stt_ms}ms): {raw_text}");
    if raw_text.is_empty() {
        return Err(anyhow::anyhow!("Nothing transcribed"));
    }

    let settings = app.state::<SettingsStore>().get();
    let (final_text, cleaned, llm_ms) = if settings.cleanup_enabled {
        emit_state(app, "cleaning", None);
        match cleanup::clean_transcript(&settings, target_app.as_ref(), &raw_text).await {
            Ok(result) => {
                log::info!(
                    "cleanup ({}ms, {:.0} tok/s): {}",
                    result.llm_ms,
                    result.tokens_per_sec.unwrap_or(0.0),
                    result.text
                );
                (result.text, true, Some(result.llm_ms))
            }
            Err(e) => {
                // Cleanup is best-effort: fall back to the raw transcript.
                log::warn!("cleanup failed, pasting raw transcript: {e}");
                (raw_text.clone(), false, None)
            }
        }
    } else {
        (raw_text.clone(), false, None)
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
