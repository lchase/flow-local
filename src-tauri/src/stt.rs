use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use transcribe_rs::onnx::{
    parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
    Quantization,
};

pub const PARAKEET_V3: SttModelInfo = SttModelInfo {
    id: "parakeet-tdt-0.6b-v3",
    name: "Parakeet V3 (int8)",
    // Tarball mirrors the one Handy (MIT) distributes; extracts to a directory.
    url: "https://blob.handy.computer/parakeet-v3-int8.tar.gz",
    sha256: "43d37191602727524a7d8c6da0eef11c4ba24320f5b4730f1a2497befc2efa77",
    dir_name: "parakeet-tdt-0.6b-v3-int8",
    size_mb: 456,
};

pub struct SttModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub dir_name: &'static str,
    pub size_mb: u64,
}

#[derive(Clone, Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub stage: String, // "downloading" | "verifying" | "extracting" | "loading" | "ready" | "error"
    pub error: Option<String>,
}

pub struct Transcriber {
    engine: Mutex<Option<ParakeetModel>>,
}

impl Transcriber {
    pub fn new() -> Self {
        Self {
            engine: Mutex::new(None),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.engine.lock().unwrap().is_some()
    }

    pub fn load(&self, model_dir: &Path) -> Result<()> {
        let mut guard = self.engine.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let engine = ParakeetModel::load(model_dir, &Quantization::Int8)
            .map_err(|e| anyhow!("failed to load parakeet model: {e}"))?;
        log::info!("parakeet loaded in {:?}", started.elapsed());
        *guard = Some(engine);
        Ok(())
    }

    /// Transcribe 16kHz mono f32 samples. Returns (text, elapsed_ms).
    pub fn transcribe(&self, audio: &[f32]) -> Result<(String, u128)> {
        let mut guard = self.engine.lock().unwrap();
        let engine = guard
            .as_mut()
            .ok_or_else(|| anyhow!("STT model not loaded yet"))?;
        let params = ParakeetParams {
            timestamp_granularity: Some(TimestampGranularity::Segment),
            ..Default::default()
        };
        let started = std::time::Instant::now();
        let result = engine
            .transcribe_with(audio, &params)
            .map_err(|e| anyhow!("transcription failed: {e}"))?;
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

pub fn model_path(app: &AppHandle) -> Result<PathBuf> {
    Ok(models_dir(app)?.join(PARAKEET_V3.dir_name))
}

fn emit_progress(app: &AppHandle, stage: &str, downloaded: u64, total: u64, error: Option<String>) {
    let _ = app.emit(
        "stt-model-progress",
        DownloadProgress {
            model_id: PARAKEET_V3.id.to_string(),
            downloaded_bytes: downloaded,
            total_bytes: total,
            stage: stage.to_string(),
            error,
        },
    );
}

/// Ensure the Parakeet model is on disk and loaded into the Transcriber.
/// Emits "stt-model-progress" events for the frontend. Blocking; call from a
/// background thread or tokio task.
pub async fn ensure_model_ready(app: AppHandle) -> Result<()> {
    let dir = model_path(&app)?;
    if !dir.exists() {
        download_and_extract(&app).await.map_err(|e| {
            emit_progress(&app, "error", 0, 0, Some(e.to_string()));
            e
        })?;
    }
    emit_progress(&app, "loading", 0, 0, None);
    let transcriber = app.state::<Transcriber>();
    let result = tokio::task::block_in_place(|| transcriber.load(&dir));
    match result {
        Ok(()) => {
            emit_progress(&app, "ready", 0, 0, None);
            Ok(())
        }
        Err(e) => {
            // A corrupt/partial extraction would fail here forever; remove so
            // the next launch re-downloads.
            let _ = std::fs::remove_dir_all(&dir);
            emit_progress(&app, "error", 0, 0, Some(e.to_string()));
            Err(e)
        }
    }
}

async fn download_and_extract(app: &AppHandle) -> Result<()> {
    use futures_util::StreamExt;

    let models = models_dir(app)?;
    let tarball = models.join(format!("{}.tar.gz.partial", PARAKEET_V3.id));

    log::info!("downloading {} from {}", PARAKEET_V3.id, PARAKEET_V3.url);
    let resp = reqwest::get(PARAKEET_V3.url).await?.error_for_status()?;
    let total = resp.content_length().unwrap_or(PARAKEET_V3.size_mb * 1024 * 1024);

    let mut file = tokio::fs::File::create(&tarball).await?;
    let mut stream = resp.bytes_stream();
    let mut downloaded: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk).await?;
        downloaded += chunk.len() as u64;
        if last_emit.elapsed().as_millis() > 200 {
            emit_progress(app, "downloading", downloaded, total, None);
            last_emit = std::time::Instant::now();
        }
    }
    tokio::io::AsyncWriteExt::flush(&mut file).await?;
    drop(file);

    emit_progress(app, "verifying", downloaded, total, None);
    let tarball_clone = tarball.clone();
    let expected = PARAKEET_V3.sha256;
    let models_clone = models.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut hasher = Sha256::new();
        let mut f = std::fs::File::open(&tarball_clone)?;
        std::io::copy(&mut f, &mut hasher)?;
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            std::fs::remove_file(&tarball_clone)?;
            return Err(anyhow!(
                "model checksum mismatch (got {actual}, expected {expected})"
            ));
        }
        let tar_gz = std::fs::File::open(&tarball_clone)?;
        let tar = flate2::read::GzDecoder::new(tar_gz);
        let mut archive = tar::Archive::new(tar);
        archive.unpack(&models_clone)?;
        std::fs::remove_file(&tarball_clone)?;
        Ok(())
    })
    .await??;

    emit_progress(app, "extracting", total, total, None);
    Ok(())
}
