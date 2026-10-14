# Implement prewarm audio capture

Type: task
Status: closed
Blocked by: 03
Assignee: agent (claude)

## Question

Build the design from [Design: prewarm audio capture architecture](03-prewarm-audio-design.md):

- `audio.rs`: add a `recording: Arc<AtomicBool>` gate to the stream callback (skip downmix/RMS when false); move stream construction out of `start()` into a `Recorder::prewarm()` that builds the worker thread + stream once; `start()` compares the live device against the one the stream was built for and only rebuilds on mismatch; `stop()` just flips the gate off instead of tearing down the stream.
- `lib.rs`: call `Recorder::prewarm()` right after `app.manage(Recorder::new())` in setup; handle a permission-not-yet-granted init error by logging (not surfacing to UI) and letting `start()`'s existing on-demand path cover that first call.
- Measure before/after against the baseline's ~170ms audio cold-start number (reuse `src-tauri/examples/audio_coldstart_bench.rs` or an equivalent steady-state timing of `start()`) to close out lever 2's before/after claim.
- Use `/verify` or `/run` to confirm real dictation still works end-to-end (including a real device-switch case) before closing.

## Answer

Landed per the design in [Design: prewarm audio capture architecture](03-prewarm-audio-design.md):

- **`audio.rs`**: replaced the per-recording worker-thread-and-stream spawn with a single long-lived worker thread (spawned in `Recorder::new()`) that owns an `Option<(cpal::Stream, device_name, sample_rate)>` and services `Prewarm`/`Start`/`Stop` over an mpsc command channel. A `recording: Arc<AtomicBool>` gate lives in the stream callback (`build_stream`): idle cost is one `Relaxed` load and an early return, no downmix/RMS work. `ensure_stream()` compares the live `default_input_device()` name against the device the open stream was built for — same device: reuse (near-zero cost); different or none yet: tear down (`Drop`) and rebuild, paying the usual cold-start cost only in that edge case. `stop()` no longer tears down the stream, just flips the gate and drains the sample buffer.
- **`lib.rs`**: `mod audio` made `pub` (needed for the bench example below); `app.state::<Recorder>().prewarm()` called right after `app.manage(Recorder::new())` in setup. Prewarm failure (e.g. mic permission not yet granted) just logs a warning — `start()`'s `ensure_stream()` retries on the next call, so first-dictation-after-grant still works, just without the prewarm win that once.
- **Measured**: new `examples/audio_prewarm_bench.rs` times steady-state `Recorder::start()` against a prewarmed stream, same device, 5 runs: **~0.18-0.4ms**, vs. the baseline's ~170ms cold-start ([Baseline: measure current dictation pipeline latency](01-baseline-latency.md)) — roughly 400-900x faster for the common case (device unchanged between recordings).
- **Verified live** (`pnpm tauri dev`): startup log shows `audio: prewarmed input stream` against the real "MacBook Pro Microphone" device with no init error; user then dictated several real phrases through the hotkey → STT → cleanup → paste path with clean logs (no errors/panics), and separately verified the device-switch path (switching input mid-session correctly triggers the rebuild-on-mismatch branch rather than silently capturing from a stale device).

Lever 2's before/after claim is closed. No further work needed on prewarm audio; the mic-indicator-always-lit tradeoff from the design ticket is accepted as-is.
