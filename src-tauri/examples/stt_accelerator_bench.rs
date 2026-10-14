//! Throwaway benchmark for wayfinder ticket 09 (lever 3: CPU-only ONNX pin).
//! Regression-guard: proves `OrtAccelerator::CpuOnly` still beats CoreML `Auto`
//! on this machine for int8 Parakeet, matching ticket 02's upstream numbers.
//! To exercise the `Auto` (CoreML) path, temporarily add to Cargo.toml:
//!   [features]
//!   ort-coreml = ["transcribe-rs/ort-coreml"]
//! then: cargo run --example stt_accelerator_bench --release --features ort-coreml
//! Revert the Cargo.toml addition afterward — not part of the shipped feature set.
//! Run (CPU-only only): cargo run --example stt_accelerator_bench --release

use transcribe_rs::onnx::{
    parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
    Quantization,
};
use transcribe_rs::{set_ort_accelerator, OrtAccelerator};

const RUNS: usize = 10;

fn bench(label: &str, accel: OrtAccelerator, samples: &[f32]) {
    set_ort_accelerator(accel);
    let home = std::env::var("HOME").unwrap();
    let model_dir = std::path::PathBuf::from(home).join(
        "Library/Application Support/com.lawrencechase.flowlocal/models/parakeet-tdt-0.6b-v3-int8",
    );
    let mut engine = ParakeetModel::load(&model_dir, &Quantization::Int8).expect("load failed");
    let params = ParakeetParams {
        timestamp_granularity: Some(TimestampGranularity::Segment),
        ..Default::default()
    };

    // Warm-up run (excluded — first inference pays one-time init cost).
    engine.transcribe_with(samples, &params).expect("transcribe failed");

    let mut total = std::time::Duration::ZERO;
    for _ in 0..RUNS {
        let started = std::time::Instant::now();
        engine.transcribe_with(samples, &params).expect("transcribe failed");
        total += started.elapsed();
    }
    println!("{label}: avg {:?} over {RUNS} runs", total / RUNS as u32);
}

fn main() {
    let mut reader = hound::WavReader::open("/tmp/stt_test.wav").expect("test wav missing");
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();

    bench("CpuOnly", OrtAccelerator::CpuOnly, &samples);
    #[cfg(feature = "ort-coreml")]
    bench("Auto (CoreML)", OrtAccelerator::Auto, &samples);
    #[cfg(not(feature = "ort-coreml"))]
    println!("ort-coreml feature not enabled — skipping Auto/CoreML comparison");
}
