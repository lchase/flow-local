use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use transcribe_rs::{
    onnx::{
        parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
        Quantization,
    },
    set_ort_accelerator,
    transcriber::{Transcriber as ChunkTranscriber, VadChunked, VadChunkedConfig},
    vad::{EnergyVad, SmoothedVad},
    whisper_cpp::{WhisperEngine, WhisperInferenceParams},
    OrtAccelerator, TranscribeOptions,
};

/// Default RMS threshold for the inner `EnergyVad`, used for any device with
/// no per-device calibration yet (see `AppSettings::mic_calibration`).
/// ponytail: starting guess, not derived; raise if background noise triggers
/// false speech onsets, lower if soft speech gets missed.
pub const STREAMING_VAD_RMS_THRESHOLD: f32 = 0.02;

/// Build a fresh per-recording `VadChunked` session: `SmoothedVad` wrapping
/// `EnergyVad`, tuned per the streaming ASR integration design (frame_size
/// 480 = 30ms @16kHz; onset_frames=2, hangover_frames=15, prefill_frames=15).
/// `threshold` is per-device calibrated RMS onset (see
/// `AppSettings::mic_calibration`), falling back to `STREAMING_VAD_RMS_THRESHOLD`.
pub fn new_streaming_vad(threshold: f32) -> VadChunked {
    let inner = EnergyVad::new(480, threshold);
    let vad = SmoothedVad::new(Box::new(inner), 15, 15, 2);
    VadChunked::new(Box::new(vad), VadChunkedConfig::default(), TranscribeOptions::default())
}

#[derive(Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Parakeet,
    Whisper,
}

#[derive(Clone, Serialize)]
pub struct SttModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    /// File name (whisper) or extracted directory name (parakeet tarballs).
    pub artifact: &'static str,
    pub is_archive: bool,
    pub size_mb: u64,
    pub engine: EngineKind,
}

/// Model artifacts mirror the ones Handy (MIT) distributes.
pub const MODELS: &[SttModelInfo] = &[
    SttModelInfo {
        id: "parakeet-tdt-0.6b-v3",
        name: "Parakeet V3 (recommended)",
        description: "Fastest. 25 European languages.",
        url: "https://blob.handy.computer/parakeet-v3-int8.tar.gz",
        sha256: "43d37191602727524a7d8c6da0eef11c4ba24320f5b4730f1a2497befc2efa77",
        artifact: "parakeet-tdt-0.6b-v3-int8",
        is_archive: true,
        size_mb: 456,
        engine: EngineKind::Parakeet,
    },
    SttModelInfo {
        id: "whisper-large-v3-turbo",
        name: "Whisper Large v3 Turbo",
        description: "Slower, but supports ~99 languages.",
        url: "https://blob.handy.computer/ggml-large-v3-turbo.bin",
        sha256: "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        artifact: "ggml-large-v3-turbo.bin",
        is_archive: false,
        size_mb: 1549,
        engine: EngineKind::Whisper,
    },
    SttModelInfo {
        id: "whisper-small",
        name: "Whisper Small",
        description: "Light multilingual fallback.",
        url: "https://blob.handy.computer/ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        artifact: "ggml-small.bin",
        is_archive: false,
        size_mb: 465,
        engine: EngineKind::Whisper,
    },
];

pub const DEFAULT_MODEL_ID: &str = "parakeet-tdt-0.6b-v3";

pub fn model_info(id: &str) -> Option<&'static SttModelInfo> {
    MODELS.iter().find(|m| m.id == id)
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub stage: String, // "downloading" | "verifying" | "extracting" | "loading" | "ready" | "error"
    pub error: Option<String>,
}

enum LoadedEngine {
    Parakeet(ParakeetModel),
    Whisper(WhisperEngine),
}

pub struct Transcriber {
    engine: Mutex<Option<(String, LoadedEngine)>>,
}

impl Transcriber {
    /// Drop the loaded model. Called on app exit: whisper.cpp's Metal device asserts
    /// (`[rsets->data count] == 0`) in a static destructor if a context is still alive
    /// when `exit()` runs, which showed up as an "unexpected error" dialog on quit.
    pub fn unload(&self) {
        if let Ok(mut engine) = self.engine.lock() {
            engine.take();
        }
    }

    pub fn new() -> Self {
        // Pin CPU-only explicitly: upstream benchmarks on int8 Parakeet (our default) show
        // CoreML Auto is 3.4-6.9x slower than CPU (transcribe-rs issue #86), and the
        // `ort-coreml` feature isn't even compiled in — this just forecloses someone flipping
        // it on later without re-checking.
        set_ort_accelerator(OrtAccelerator::CpuOnly);
        Self {
            engine: Mutex::new(None),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.engine.lock().unwrap().is_some()
    }

    pub fn loaded_model_id(&self) -> Option<String> {
        self.engine.lock().unwrap().as_ref().map(|(id, _)| id.clone())
    }

    pub fn load(&self, info: &SttModelInfo, artifact_path: &Path) -> Result<()> {
        let mut guard = self.engine.lock().unwrap();
        if guard.as_ref().is_some_and(|(id, _)| id == info.id) {
            return Ok(());
        }
        *guard = None; // Drop the old engine before loading the next one.
        let started = std::time::Instant::now();
        let engine = match info.engine {
            EngineKind::Parakeet => LoadedEngine::Parakeet(
                ParakeetModel::load(artifact_path, &Quantization::Int8)
                    .map_err(|e| anyhow!("failed to load parakeet model: {e}"))?,
            ),
            EngineKind::Whisper => LoadedEngine::Whisper(
                WhisperEngine::load(artifact_path)
                    .map_err(|e| anyhow!("failed to load whisper model: {e}"))?,
            ),
        };
        log::info!("{} loaded in {:?}", info.id, started.elapsed());
        *guard = Some((info.id.to_string(), engine));
        Ok(())
    }

    /// Run a short silent inference to keep model memory pages hot.
    /// Uses try_lock so a real transcription in progress is never delayed.
    pub fn keepalive(&self) {
        let Ok(mut guard) = self.engine.try_lock() else { return };
        let Some((_, engine)) = guard.as_mut() else { return };
        let silence = vec![0.0f32; 8000]; // 0.5 s of silence at 16 kHz
        let _ = match engine {
            LoadedEngine::Parakeet(p) => {
                p.transcribe_with(&silence, &ParakeetParams::default()).map(|_| ())
            }
            LoadedEngine::Whisper(w) => {
                w.transcribe_with(&silence, &WhisperInferenceParams::default()).map(|_| ())
            }
        };
    }

    /// Transcribe 16kHz mono f32 samples. Returns (text, elapsed_ms).
    pub fn transcribe(&self, audio: &[f32]) -> Result<(String, u128)> {
        let mut guard = self.engine.lock().unwrap();
        let (_, engine) = guard
            .as_mut()
            .ok_or_else(|| anyhow!("STT model not loaded yet"))?;
        let started = std::time::Instant::now();
        let text = match engine {
            LoadedEngine::Parakeet(parakeet) => {
                let params = ParakeetParams {
                    timestamp_granularity: Some(TimestampGranularity::Segment),
                    ..Default::default()
                };
                parakeet
                    .transcribe_with(audio, &params)
                    .map_err(|e| anyhow!("transcription failed: {e}"))?
                    .text
            }
            LoadedEngine::Whisper(whisper) => {
                let params = WhisperInferenceParams::default();
                whisper
                    .transcribe_with(audio, &params)
                    .map_err(|e| anyhow!("transcription failed: {e}"))?
                    .text
            }
        };
        Ok((text.trim().to_string(), started.elapsed().as_millis()))
    }

    /// Feed a chunk of 16kHz mono f32 samples into a live streaming session.
    /// Internally chunk-transcribes on speech/silence boundaries; most of a
    /// long recording is already transcribed by the time `finish_streaming`
    /// runs, which is what buys the tail-latency win.
    pub fn feed_streaming(&self, vad: &mut VadChunked, audio: &[f32]) -> Result<()> {
        if audio.is_empty() {
            return Ok(());
        }
        let mut guard = self.engine.lock().unwrap();
        let (_, engine) = guard
            .as_mut()
            .ok_or_else(|| anyhow!("STT model not loaded yet"))?;
        match engine {
            LoadedEngine::Parakeet(m) => vad.feed(m, audio),
            LoadedEngine::Whisper(m) => vad.feed(m, audio),
        }
        .map_err(|e| anyhow!("streaming feed failed: {e}"))?;
        Ok(())
    }

    /// Feed the final tail and merge all chunk results into one transcript.
    /// Returns (text, elapsed_ms) — elapsed covers only this call, since most
    /// of the audio was already transcribed by prior `feed_streaming` calls.
    pub fn finish_streaming(&self, vad: &mut VadChunked) -> Result<(String, u128)> {
        let mut guard = self.engine.lock().unwrap();
        let (_, engine) = guard
            .as_mut()
            .ok_or_else(|| anyhow!("STT model not loaded yet"))?;
        let started = std::time::Instant::now();
        let result = match engine {
            LoadedEngine::Parakeet(m) => vad.finish(m),
            LoadedEngine::Whisper(m) => vad.finish(m),
        }
        .map_err(|e| anyhow!("streaming finish failed: {e}"))?;
        Ok((result.text.trim().to_string(), started.elapsed().as_millis()))
    }
}

pub fn models_dir(app: &AppHandle) -> Result<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| anyhow!("no app data dir: {e}"))?
        .join("models");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn artifact_path(app: &AppHandle, info: &SttModelInfo) -> Result<PathBuf> {
    Ok(models_dir(app)?.join(info.artifact))
}

fn emit_progress(
    app: &AppHandle,
    model_id: &str,
    stage: &str,
    downloaded: u64,
    total: u64,
    error: Option<String>,
) {
    let _ = app.emit(
        "stt-model-progress",
        DownloadProgress {
            model_id: model_id.to_string(),
            downloaded_bytes: downloaded,
            total_bytes: total,
            stage: stage.to_string(),
            error,
        },
    );
}

/// Ensure the given model is on disk and loaded into the Transcriber.
/// Emits "stt-model-progress" events for the frontend.
pub async fn ensure_model_ready(app: AppHandle, model_id: &str) -> Result<()> {
    let info = model_info(model_id).ok_or_else(|| anyhow!("unknown STT model '{model_id}'"))?;
    let path = artifact_path(&app, info)?;
    if !path.exists() {
        download(&app, info).await.map_err(|e| {
            emit_progress(&app, info.id, "error", 0, 0, Some(e.to_string()));
            e
        })?;
    }
    emit_progress(&app, info.id, "loading", 0, 0, None);
    let transcriber = app.state::<Transcriber>();
    let result = tokio::task::block_in_place(|| transcriber.load(info, &path));
    match result {
        Ok(()) => {
            emit_progress(&app, info.id, "ready", 0, 0, None);
            Ok(())
        }
        Err(e) => {
            // A corrupt artifact would fail here forever; remove so the next
            // attempt re-downloads.
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
            emit_progress(&app, info.id, "error", 0, 0, Some(e.to_string()));
            Err(e)
        }
    }
}

async fn download(app: &AppHandle, info: &SttModelInfo) -> Result<()> {
    use futures_util::StreamExt;

    let models = models_dir(app)?;
    let partial = models.join(format!("{}.partial", info.id));

    log::info!("downloading {} from {}", info.id, info.url);
    let resp = reqwest::get(info.url).await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(info.size_mb * 1024 * 1024);

    let mut file = tokio::fs::File::create(&partial).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed().as_millis() > 200 {
            emit_progress(app, info.id, "downloading", downloaded, total, None);
            last_emit = std::time::Instant::now();
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    emit_progress(app, info.id, "verifying", downloaded, total, None);
    let partial_clone = partial.clone();
    let models_clone = models.clone();
    let info_clone = info.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut hasher = Sha256::new();
        let mut f = std::fs::File::open(&partial_clone)?;
        std::io::copy(&mut f, &mut hasher)?;
        let actual = format!("{:x}", hasher.finalize());
        if actual != info_clone.sha256 {
            std::fs::remove_file(&partial_clone)?;
            return Err(anyhow!(
                "model checksum mismatch (got {actual}, expected {})",
                info_clone.sha256
            ));
        }
        if info_clone.is_archive {
            let tar_gz = std::fs::File::open(&partial_clone)?;
            let tar = flate2::read::GzDecoder::new(tar_gz);
            let mut archive = tar::Archive::new(tar);
            archive.unpack(&models_clone)?;
            std::fs::remove_file(&partial_clone)?;
        } else {
            std::fs::rename(&partial_clone, models_clone.join(info_clone.artifact))?;
        }
        Ok(())
    })
    .await??;

    emit_progress(app, info.id, "extracting", total, total, None);
    Ok(())
}
