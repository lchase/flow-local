//! End-to-end STT check against the real downloaded Parakeet model.
//! Requires the app to have downloaded the model once and a 16kHz mono WAV:
//!   say -o /tmp/stt_test.wav --data-format=LEI16@16000 "Let's meet at two, actually three PM. Thanks so much for setting this up."
//! Run with: cargo test --test stt_e2e -- --ignored --nocapture

use transcribe_rs::{
    onnx::{
        parakeet::{ParakeetModel, ParakeetParams, TimestampGranularity},
        Quantization,
    },
    transcriber::{Transcriber as ChunkTranscriber, VadChunked, VadChunkedConfig},
    vad::{EnergyVad, SmoothedVad},
    TranscribeOptions,
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

/// Regression check for the streaming-ASR plumbing (issue 10): feeding audio
/// through `VadChunked` in ~200ms slices (mirroring the poll task's tick
/// size) then `finish()` must reproduce the same content as one whole-utterance
/// `transcribe_with` call. Also reports both timings for the design's
/// before/after measurement — see doc comment on `bench_utterance` below for
/// why the "after" number here isn't the real production win.
#[test]
#[ignore = "needs downloaded model + generated wavs (see bench_utterance)"]
fn streaming_matches_batch_transcription() {
    let home = std::env::var("HOME").unwrap();
    let model_dir = std::path::PathBuf::from(home)
        .join("Library/Application Support/com.lawrencechase.flowlocal/models/parakeet-tdt-0.6b-v3-int8");
    assert!(model_dir.exists(), "model not downloaded at {model_dir:?}");

    for (label, path, expect_words) in [
        (
            "short",
            "/tmp/stt_test_short.wav",
            vec!["meet", "3", "thanks", "setting"],
        ),
        (
            "long",
            "/tmp/stt_test_long.wav",
            vec!["roadmap", "mobile", "database", "security", "engineering"],
        ),
    ] {
        let mut model = ParakeetModel::load(&model_dir, &Quantization::Int8).expect("load failed");
        let samples = read_wav_16k(path);

        let (batch_text, batch_ms) = bench_utterance(&mut model, &samples, false);
        let (stream_text, stream_ms) = bench_utterance(&mut model, &samples, true);

        println!(
            "[{label}] batch={batch_ms}ms streaming(no-overlap)={stream_ms}ms batch_text={batch_text:?} stream_text={stream_text:?}"
        );

        for word in &expect_words {
            assert!(
                batch_text.to_lowercase().contains(word),
                "[{label}] batch missing '{word}' in: {batch_text}"
            );
            assert!(
                stream_text.to_lowercase().contains(word),
                "[{label}] streaming missing '{word}' in: {stream_text}"
            );
        }
    }
}

fn read_wav_16k(path: &str) -> Vec<f32> {
    let mut reader = hound::WavReader::open(path).unwrap_or_else(|e| panic!("{path} missing: {e}"));
    let spec = reader.spec();
    assert_eq!(spec.sample_rate, 16_000, "expected 16kHz wav");
    reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / i16::MAX as f32)
        .collect()
}

/// Times either one whole-utterance `transcribe_with` call, or feeding the
/// same audio through a fresh `VadChunked` session in ~200ms slices (the poll
/// task's tick size) then `finish()`.
///
/// This does NOT measure the real production win: in the app, most of a
/// recording is already chunk-transcribed *during* `Recording`, in parallel
/// with the user still talking, so only the last unpolled tail is left by
/// the time `Stop` fires. Here there's no recording to overlap with — all
/// slices are fed back-to-back with no wall-clock gap — so this instead
/// measures per-chunk overhead vs. one big call. Use it as a functional
/// regression check; get the real before/after `stt_ms` from a live
/// dictation's History entry (`/verify` or `/run`, per the design ticket).
fn bench_utterance(model: &mut ParakeetModel, samples: &[f32], streaming: bool) -> (String, u128) {
    const SLICE: usize = 200 * 16; // ~200ms at 16kHz, matching the poll interval
    let started = std::time::Instant::now();
    let text = if streaming {
        let vad = SmoothedVad::new(Box::new(EnergyVad::new(480, 0.02)), 15, 15, 2);
        let mut chunker = VadChunked::new(Box::new(vad), VadChunkedConfig::default(), TranscribeOptions::default());
        for slice in samples.chunks(SLICE) {
            chunker.feed(model, slice).expect("feed failed");
        }
        chunker.finish(model).expect("finish failed").text
    } else {
        let params = ParakeetParams {
            timestamp_granularity: Some(TimestampGranularity::Segment),
            ..Default::default()
        };
        model.transcribe_with(samples, &params).expect("transcribe failed").text
    };
    (text.trim().to_string(), started.elapsed().as_millis())
}
