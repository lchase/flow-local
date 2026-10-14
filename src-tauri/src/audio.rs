use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SizedSample};
use rubato::{FftFixedIn, Resampler};

pub const TARGET_SAMPLE_RATE: usize = 16_000;

/// Chunk size (in samples, at the stream's native rate) fed to the resampler
/// at a time, both for the one-shot batch resample and the persistent
/// incremental one.
const RESAMPLE_CHUNK: usize = 1024;

enum Cmd {
    Start,
    Stop,
}

enum WorkerMsg {
    /// Stream is live, recording at this native sample rate for this device.
    Ready(u32, String),
    Done(Vec<f32>, u32),
    InitError(String),
}

/// Push-to-talk recorder. A worker thread owns the cpal stream (cpal streams
/// are !Send, so they can't live in tauri State directly). The stream is
/// opened fresh on `start()` and fully torn down on `stop()`, so the OS
/// mic-in-use indicator only lights up for the duration of an actual
/// recording — not for the app's whole lifetime.
pub struct Recorder {
    recording: Arc<AtomicBool>,
    cmd_tx: mpsc::Sender<Cmd>,
    msg_rx: Mutex<mpsc::Receiver<WorkerMsg>>,
    level: Arc<Mutex<f32>>,
    /// Shared with the worker's stream callback. Read-only from here —
    /// `drain_since` lets a poll task see new tail samples mid-recording
    /// without disturbing `stop()`'s full-buffer take.
    samples: Arc<Mutex<Vec<f32>>>,
    /// Native sample rate of the stream currently backing recording,
    /// updated each time `start()` (re)builds it.
    rate: Mutex<u32>,
    /// cpal device name backing the current stream, updated alongside `rate`.
    /// Used to key per-device VAD calibration (`AppSettings::mic_calibration`).
    device_name: Mutex<String>,
    /// Set by the stream's error callback when the input device disappears
    /// mid-recording (`cpal::StreamError::DeviceNotAvailable`); drained by
    /// the pipeline's streaming poll loop to surface it instead of just
    /// logging it.
    stream_error: Arc<Mutex<Option<String>>>,
}

impl Recorder {
    pub fn new() -> Self {
        let recording = Arc::new(AtomicBool::new(false));
        let level = Arc::new(Mutex::new(0.0));
        let samples: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let (cmd_tx, cmd_rx) = mpsc::channel::<Cmd>();
        let (msg_tx, msg_rx) = mpsc::channel::<WorkerMsg>();

        let stream_error = Arc::new(Mutex::new(None));

        let worker_recording = recording.clone();
        let worker_level = level.clone();
        let worker_samples = samples.clone();
        let worker_stream_error = stream_error.clone();
        std::thread::spawn(move || {
            worker_loop(
                cmd_rx,
                msg_tx,
                worker_recording,
                worker_level,
                worker_samples,
                worker_stream_error,
            )
        });

        Self {
            recording,
            cmd_tx,
            msg_rx: Mutex::new(msg_rx),
            level,
            samples,
            rate: Mutex::new(TARGET_SAMPLE_RATE as u32),
            device_name: Mutex::new(String::new()),
            stream_error,
        }
    }

    /// Native sample rate of the stream currently backing recording.
    pub fn input_sample_rate(&self) -> u32 {
        *self.rate.lock().unwrap()
    }

    /// cpal device name backing the current stream (empty if none built yet).
    pub fn current_device_name(&self) -> String {
        self.device_name.lock().unwrap().clone()
    }

    /// Take and clear a pending mid-recording device error, if any (e.g. the
    /// input device was unplugged). Best-effort, polled rather than pushed.
    pub fn take_stream_error(&self) -> Option<String> {
        self.stream_error.lock().unwrap().take()
    }

    /// Non-destructive tail read: samples (at `input_sample_rate()`) appended
    /// to the recording buffer since `cursor`, advancing `cursor` to the
    /// current length. Safe to call mid-recording — doesn't touch `stop()`'s
    /// full-buffer take.
    pub fn drain_since(&self, cursor: &mut usize) -> Vec<f32> {
        let buf = self.samples.lock().unwrap();
        let start = (*cursor).min(buf.len());
        let tail = buf[start..].to_vec();
        *cursor = buf.len();
        tail
    }

    /// RMS level of the most recent audio callback, for the overlay waveform.
    pub fn current_level(&self) -> f32 {
        *self.level.lock().unwrap()
    }

    pub fn start(&self) -> Result<()> {
        if self.recording.load(Ordering::SeqCst) {
            return Err(anyhow!("already recording"));
        }
        self.cmd_tx
            .send(Cmd::Start)
            .map_err(|_| anyhow!("audio worker is gone"))?;
        match self
            .msg_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(3))
        {
            Ok(WorkerMsg::Ready(rate, name)) => {
                *self.rate.lock().unwrap() = rate;
                *self.device_name.lock().unwrap() = name;
                Ok(())
            }
            Ok(WorkerMsg::InitError(e)) => Err(anyhow!(e)),
            Ok(WorkerMsg::Done(..)) => Err(anyhow!("unexpected message starting recording")),
            Err(e) => Err(anyhow!("audio worker did not start: {e}")),
        }
    }

    /// Stop recording and return mono samples resampled to 16kHz.
    pub fn stop(&self) -> Result<Vec<f32>> {
        if !self.recording.load(Ordering::SeqCst) {
            return Err(anyhow!("not recording"));
        }
        self.cmd_tx
            .send(Cmd::Stop)
            .map_err(|_| anyhow!("audio worker is gone"))?;
        match self
            .msg_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
        {
            Ok(WorkerMsg::Done(samples, rate)) => resample_to_16k(samples, rate as usize),
            Ok(WorkerMsg::InitError(e)) => Err(anyhow!(e)),
            Ok(WorkerMsg::Ready(..)) => Err(anyhow!("unexpected message stopping recording")),
            Err(e) => Err(anyhow!("audio worker did not stop: {e}")),
        }
    }
}

/// Stream plus the device it was built for, so `ensure_stream` can tell
/// whether the default input device moved since it was built.
type StreamState = (cpal::Stream, String, u32);

fn worker_loop(
    cmd_rx: mpsc::Receiver<Cmd>,
    msg_tx: mpsc::Sender<WorkerMsg>,
    recording: Arc<AtomicBool>,
    level: Arc<Mutex<f32>>,
    samples: Arc<Mutex<Vec<f32>>>,
    stream_error: Arc<Mutex<Option<String>>>,
) {
    let mut stream_state: Option<StreamState> = None;

    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            Cmd::Start => {
                match ensure_stream(&mut stream_state, &samples, &level, &recording, &stream_error)
                {
                    Ok((rate, name)) => {
                        samples.lock().unwrap().clear();
                        recording.store(true, Ordering::SeqCst);
                        let _ = msg_tx.send(WorkerMsg::Ready(rate, name));
                    }
                    Err(e) => {
                        let _ = msg_tx.send(WorkerMsg::InitError(e));
                    }
                }
            }
            Cmd::Stop => {
                recording.store(false, Ordering::SeqCst);
                let collected = std::mem::take(&mut *samples.lock().unwrap());
                let rate = stream_state
                    .as_ref()
                    .map(|(_, _, rate)| *rate)
                    .unwrap_or(TARGET_SAMPLE_RATE as u32);
                // Fully close the device here (not just gate `recording`) so the
                // OS mic-in-use indicator turns off between dictations.
                stream_state = None;
                let _ = msg_tx.send(WorkerMsg::Done(collected, rate));
            }
        }
    }
}

/// Ensure `state` holds a live stream for the current default input device,
/// reusing it if the device hasn't changed and rebuilding (paying the usual
/// cold-start cost) only if it has, or none exists yet.
fn ensure_stream(
    state: &mut Option<StreamState>,
    samples: &Arc<Mutex<Vec<f32>>>,
    level: &Arc<Mutex<f32>>,
    recording: &Arc<AtomicBool>,
    stream_error: &Arc<Mutex<Option<String>>>,
) -> Result<(u32, String), String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "no input device found".to_string())?;
    let device_name = device.name().unwrap_or_default();

    if let Some((_, built_for, rate)) = state.as_ref() {
        if *built_for == device_name {
            return Ok((*rate, device_name));
        }
        log::info!("audio: default input device changed to {device_name:?}, rebuilding stream");
    }

    let config = device
        .default_input_config()
        .map_err(|e| format!("no default input config: {e}"))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    log::info!(
        "audio: building stream for {device_name:?} @ {}Hz x{} ({:?})",
        sample_rate,
        channels,
        config.sample_format()
    );

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build_stream::<f32>(
            &device,
            &config.into(),
            channels,
            samples.clone(),
            level.clone(),
            recording.clone(),
            stream_error.clone(),
        ),
        cpal::SampleFormat::I16 => build_stream::<i16>(
            &device,
            &config.into(),
            channels,
            samples.clone(),
            level.clone(),
            recording.clone(),
            stream_error.clone(),
        ),
        cpal::SampleFormat::U16 => build_stream::<u16>(
            &device,
            &config.into(),
            channels,
            samples.clone(),
            level.clone(),
            recording.clone(),
            stream_error.clone(),
        ),
        cpal::SampleFormat::I32 => build_stream::<i32>(
            &device,
            &config.into(),
            channels,
            samples.clone(),
            level.clone(),
            recording.clone(),
            stream_error.clone(),
        ),
        other => Err(format!("unsupported sample format {other:?}")),
    }?;
    stream.play().map_err(|e| format!("stream play failed: {e}"))?;

    *state = Some((stream, device_name.clone(), sample_rate));
    Ok((sample_rate, device_name))
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<f32>>,
    recording: Arc<AtomicBool>,
    stream_error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: cpal::FromSample<T>,
{
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                // Idle cost of the always-on stream: a single relaxed load,
                // no downmix/RMS work, when not actively recording.
                if !recording.load(Ordering::Relaxed) {
                    return;
                }
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
            move |e| {
                log::error!("audio stream error: {e}");
                if matches!(e, cpal::StreamError::DeviceNotAvailable) {
                    *stream_error.lock().unwrap() =
                        Some("Microphone disconnected".to_string());
                }
            },
            None,
        )
        .map_err(|e| format!("build_input_stream failed: {e}"))
}

fn resample_to_16k(samples: Vec<f32>, in_rate: usize) -> Result<Vec<f32>> {
    if in_rate == TARGET_SAMPLE_RATE {
        return Ok(samples);
    }
    let mut resampler = FftFixedIn::<f32>::new(in_rate, TARGET_SAMPLE_RATE, RESAMPLE_CHUNK, 1, 1)
        .map_err(|e| anyhow!("resampler init failed: {e}"))?;
    let mut out =
        Vec::with_capacity(samples.len() * TARGET_SAMPLE_RATE / in_rate + RESAMPLE_CHUNK);
    for chunk in samples.chunks(RESAMPLE_CHUNK) {
        let input = if chunk.len() == RESAMPLE_CHUNK {
            chunk.to_vec()
        } else {
            // Zero-pad the trailing partial chunk (adds <64ms of silence).
            let mut padded = chunk.to_vec();
            padded.resize(RESAMPLE_CHUNK, 0.0);
            padded
        };
        let result = resampler
            .process(&[input], None)
            .map_err(|e| anyhow!("resample failed: {e}"))?;
        out.extend_from_slice(&result[0]);
    }
    Ok(out)
}

/// Incremental counterpart to [`resample_to_16k`], for feeding a live tail of
/// native-rate samples into a streaming consumer a poll tick at a time.
/// Reuses one `FftFixedIn` instance across calls (rebuilding it every tick
/// would re-pay its FFT-plan setup cost) and carries forward any samples
/// short of a full chunk to the next `push()`.
pub struct TailResampler {
    resampler: Option<FftFixedIn<f32>>, // None when the stream is already 16kHz
    pending: Vec<f32>,
}

impl TailResampler {
    pub fn new(in_rate: u32) -> Result<Self> {
        let in_rate = in_rate as usize;
        let resampler = if in_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            Some(
                FftFixedIn::<f32>::new(in_rate, TARGET_SAMPLE_RATE, RESAMPLE_CHUNK, 1, 1)
                    .map_err(|e| anyhow!("resampler init failed: {e}"))?,
            )
        };
        Ok(Self {
            resampler,
            pending: Vec::new(),
        })
    }

    /// Feed newly-captured native-rate samples, returning any complete
    /// 16kHz chunks produced. A remainder short of a full chunk is buffered
    /// for the next `push()`.
    pub fn push(&mut self, native: &[f32]) -> Result<Vec<f32>> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(native.to_vec());
        };
        self.pending.extend_from_slice(native);
        let mut out = Vec::new();
        while self.pending.len() >= RESAMPLE_CHUNK {
            let chunk: Vec<f32> = self.pending.drain(..RESAMPLE_CHUNK).collect();
            let result = resampler
                .process(&[chunk], None)
                .map_err(|e| anyhow!("resample failed: {e}"))?;
            out.extend_from_slice(&result[0]);
        }
        Ok(out)
    }

    /// Flush any buffered remainder at end of stream, zero-padding to a full
    /// chunk like `resample_to_16k`'s trailing partial chunk (adds <64ms of
    /// silence).
    pub fn flush(&mut self) -> Result<Vec<f32>> {
        let Some(resampler) = self.resampler.as_mut() else {
            return Ok(std::mem::take(&mut self.pending));
        };
        if self.pending.is_empty() {
            return Ok(Vec::new());
        }
        let mut padded = std::mem::take(&mut self.pending);
        padded.resize(RESAMPLE_CHUNK, 0.0);
        let result = resampler
            .process(&[padded], None)
            .map_err(|e| anyhow!("resample failed: {e}"))?;
        Ok(result[0].clone())
    }
}
