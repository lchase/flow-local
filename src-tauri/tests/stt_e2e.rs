//! End-to-end STT check against the real downloaded Parakeet model.
//! Requires the app to have downloaded the model once and a 16kHz mono WAV:
//!   say -o /tmp/stt_test.wav --data-format=LEI16@16000 "Let's meet at two, actually three PM. Thanks so much for setting this up."
//! Run with: cargo test --test stt_e2e -- --ignored --nocapture

use transcribe_rs::onnx::{
    parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
    Quantization,
};

#[test]
#[ignore = "needs downloaded model + generated wav"]
fn transcribes_synthesized_speech() {
    let home = std::env::var("HOME").unwrap();
    let model_dir = std::path::PathBuf::from(home)
        .join("Library/Application Support/com.lawrencechase.flowlocal/models/parakeet-tdt-0.6b-v3-int8");
    assert!(model_dir.exists(), "model not downloaded at {model_dir:?}");

    let mut reader = hound::WavReader::open("/tmp/stt_test.wav").expect("test wav missing");
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "expected 16kHz wav");
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect();

    let mut engine = ParakeetModel::load(&model_dir, &Quantization::Int8).expect("load failed");
    let params = ParakeetParams {
        timestamp_granularity: Some(TimestampGranularity::Segment),
        ..Default::default()
    };
    let started = std::time::Instant::now();
    let result = engine.transcribe_with(&samples, &params).expect("transcribe failed");
    let text = result.text.to_lowercase();
    println!("transcript ({:?}): {}", started.elapsed(), result.text);

    // Parakeet applies inverse text normalization: "three" comes out as "3".
    for word in ["meet", "3", "thanks", "setting"] {
        assert!(text.contains(word), "missing '{word}' in: {text}");
    }
}
