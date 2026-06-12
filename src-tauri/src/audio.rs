use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SizedSample};
use rubato::{FftFixedIn, Resampler};

pub const TARGET_SAMPLE_RATE: usize = 16_000;

enum WorkerMsg {
    Done(Vec<f32>, u32),
    InitError(String),
}

/// Push-to-talk recorder. `start()` spawns a worker thread that owns the cpal
/// stream (cpal streams are !Send, so they can't live in tauri State directly).
/// The callback downmixes interleaved frames to mono f32; `stop()` joins the
/// worker and returns samples resampled to 16kHz.
pub struct Recorder {
    stop_flag: Arc<AtomicBool>,
    worker: Mutex<Option<(std::thread::JoinHandle<()>, mpsc::Receiver<WorkerMsg>)>>,
    level: Arc<Mutex<f32>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            stop_flag: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            level: Arc::new(Mutex::new(0.0)),
        }
    }

    /// RMS level of the most recent audio callback, for the overlay waveform.
    pub fn current_level(&self) -> f32 {
        *self.level.lock().unwrap()
    }

    pub fn start(&self) -> Result<()> {
        let mut worker_slot = self.worker.lock().unwrap();
        if worker_slot.is_some() {
            return Err(anyhow!("already recording"));
        }
        self.stop_flag.store(false, Ordering::SeqCst);

        let stop_flag = self.stop_flag.clone();
        let level = self.level.clone();
        let (msg_tx, msg_rx) = mpsc::channel::<WorkerMsg>();

        let handle = std::thread::spawn(move || {
            let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));

            let init = (|| -> Result<(cpal::Stream, u32), String> {
                let host = cpal::default_host();
                let device = host
                    .default_input_device()
                    .ok_or_else(|| "no input device found".to_string())?;
                let config = device
                    .default_input_config()
                    .map_err(|e| format!("no default input config: {e}"))?;
                let sample_rate = config.sample_rate().0;
                let channels = config.channels() as usize;
                log::info!(
                    "recording from {:?} @ {}Hz x{} ({:?})",
                    device.name().unwrap_or_default(),
                    sample_rate,
                    channels,
                    config.sample_format()
                );

                let stream = match config.sample_format() {
                    cpal::SampleFormat::F32 => {
                        build_stream::<f32>(&device, &config.into(), channels, samples.clone(), level)
                    }
                    cpal::SampleFormat::I16 => {
                        build_stream::<i16>(&device, &config.into(), channels, samples.clone(), level)
                    }
                    cpal::SampleFormat::U16 => {
                        build_stream::<u16>(&device, &config.into(), channels, samples.clone(), level)
                    }
                    cpal::SampleFormat::I32 => {
                        build_stream::<i32>(&device, &config.into(), channels, samples.clone(), level)
                    }
                    other => Err(format!("unsupported sample format {other:?}")),
                }?;
                stream.play().map_err(|e| format!("stream play failed: {e}"))?;
                Ok((stream, sample_rate))
            })();

            let (stream, sample_rate) = match init {
                Ok(ok) => ok,
                Err(e) => {
                    let _ = msg_tx.send(WorkerMsg::InitError(e));
                    return;
                }
            };

            while !stop_flag.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            drop(stream);

            let collected = std::mem::take(&mut *samples.lock().unwrap());
            let _ = msg_tx.send(WorkerMsg::Done(collected, sample_rate));
        });

        // Surface device/permission errors immediately rather than at stop().
        match msg_rx.recv_timeout(std::time::Duration::from_millis(1500)) {
            Ok(WorkerMsg::InitError(e)) => {
                let _ = handle.join();
                return Err(anyhow!(e));
            }
            Ok(WorkerMsg::Done(..)) => unreachable!("worker finished before stop"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No error within the window: stream is up and running.
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = handle.join();
                return Err(anyhow!("audio worker died during init"));
            }
        }

        *worker_slot = Some((handle, msg_rx));
        Ok(())
    }

    /// Stop recording and return mono samples resampled to 16kHz.
    pub fn stop(&self) -> Result<Vec<f32>> {
        let Some((handle, msg_rx)) = self.worker.lock().unwrap().take() else {
            return Err(anyhow!("not recording"));
        };
        self.stop_flag.store(true, Ordering::SeqCst);
        let msg = msg_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| anyhow!("audio worker did not stop: {e}"))?;
        let _ = handle.join();
        match msg {
            WorkerMsg::Done(samples, rate) => resample_to_16k(samples, rate as usize),
            WorkerMsg::InitError(e) => Err(anyhow!(e)),
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<f32>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut buf = samples.lock().unwrap();
                let mut sum_sq = 0.0f32;
                for frame in data.chunks(channels) {
                    let mono = frame
                        .iter()
                        .map(|s| f32::from_sample(*s))
                        .sum::<f32>()
                        / channels as f32;
                    sum_sq += mono * mono;
                    buf.push(mono);
                }
                let n = (data.len() / channels).max(1);
                *level.lock().unwrap() = (sum_sq / n as f32).sqrt();
            },
            |e| log::error!("audio stream error: {e}"),
            None,
        )
        .map_err(|e| format!("build_input_stream failed: {e}"))
}

fn resample_to_16k(samples: Vec<f32>, in_rate: usize) -> Result<Vec<f32>> {
    if in_rate == TARGET_SAMPLE_RATE {
        return Ok(samples);
    }
    const CHUNK: usize = 1024;
    let mut resampler = FftFixedIn::<f32>::new(in_rate, TARGET_SAMPLE_RATE, CHUNK, 1, 1)
        .map_err(|e| anyhow!("resampler init failed: {e}"))?;
    let mut out =
        Vec::with_capacity(samples.len() * TARGET_SAMPLE_RATE / in_rate + CHUNK);
    for chunk in samples.chunks(CHUNK) {
        let input = if chunk.len() == CHUNK {
            chunk.to_vec()
        } else {
            // Zero-pad the trailing partial chunk (adds <64ms of silence).
            let mut padded = chunk.to_vec();
            padded.resize(CHUNK, 0.0);
            padded
        };
        let result = resampler
            .process(&[input], None)
            .map_err(|e| anyhow!("resample failed: {e}"))?;
        out.extend_from_slice(&result[0]);
    }
    Ok(out)
}
