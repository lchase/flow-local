//! Throwaway benchmark for wayfinder ticket 01 (baseline latency).
//! Measures cpal host/device/stream construction cost, mirroring audio.rs::start().
//! Run: cargo run --example audio_coldstart_bench --release

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

fn main() {
    for i in 1..=5 {
        let started = std::time::Instant::now();
        let host = cpal::default_host();
        let device = host.default_input_device().expect("no input device");
        let config = device.default_input_config().expect("no default config");
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &config.into(),
                    move |_data: &[f32], _| {},
                    move |err| eprintln!("stream error: {err}"),
                    None,
                )
                .expect("build stream failed"),
            _ => panic!("unexpected sample format for this bench"),
        };
        stream.play().expect("play failed");
        let elapsed = started.elapsed();
        println!("run {i}: host+device+config+build+play = {elapsed:?}");
        drop(stream);
    }
}
