//! Throwaway benchmark for wayfinder ticket 07 (prewarm audio capture).
//! Measures steady-state Recorder::start() cost against the same device the
//! prewarmed stream was built for — compare against ticket 01's ~170ms
//! cold-start number.
//! Run: cargo run --example audio_prewarm_bench --release

use flow_local_lib::audio::Recorder;

fn main() {
    let recorder = Recorder::new();
    recorder.prewarm();

    for i in 1..=5 {
        let started = std::time::Instant::now();
        recorder.start().expect("start failed");
        let start_elapsed = started.elapsed();
        std::thread::sleep(std::time::Duration::from_millis(50));
        recorder.stop().expect("stop failed");
        println!("run {i}: steady-state start() = {start_elapsed:?}");
    }
}
