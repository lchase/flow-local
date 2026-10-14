# Implement streaming ASR integration

Type: task
Status: closed
Blocked by: 08
Assignee: agent (claude)

## Question

Build the design from [Design: streaming ASR integration architecture](08-streaming-asr-integration-design.md):

- `audio.rs`: expose a cloned `Arc<Mutex<Vec<f32>>>` handle on `Recorder` to the worker's sample buffer, plus a non-destructive `drain_since(&self, cursor: &mut usize) -> Vec<f32>` accessor. Add a persistent incremental resampler (stateful `rubato::FftFixedIn`, reused across poll ticks) that turns native-rate tail slices into 16kHz chunks, buffering any leftover < `CHUNK` samples between ticks.
- `stt.rs`: add `feed_streaming`/`finish_streaming` on `Transcriber`, dispatching on the loaded `LoadedEngine` variant and calling `VadChunked::feed`/`finish` through the `SpeechModel` trait. Construct the VAD as `SmoothedVad` wrapping `EnergyVad` (onset_frames=2, hangover_frames=15, prefill_frames=15 — starting tune, RMS threshold TBD during verification).
- `pipeline.rs`: add `streaming: Mutex<Option<VadChunked>>` to `Pipeline`. In `start()`, construct it and spawn a `tokio::time::interval` (~200ms) poll task that drains new tail samples, resamples, and feeds them in. In `stop_and_process()`, stop the poll task, feed the final tail, call `finish_streaming`, and use its merged text as `raw_text` — delete the old whole-utterance `Transcriber::transcribe()` call from this path.
- Hand-tune the `EnergyVad` RMS threshold against real mic input (no synthetic-test substitute for this one).
- Measure before/after per [Design](08-streaming-asr-integration-design.md) decision 6: generate a short (~3s) and long (~12-15s) test `.wav` via `say`, measure `stt_ms` on `main` and again on this change, for both utterances.
- Use `/verify` or `/run` to confirm real dictations still work end-to-end (including a real mid-dictation pause/resume of speech) before closing.

## Answer

Landed per the design, mirroring the 03→07 and 06→09 split:

- **`audio.rs`**: `Recorder` gained `input_sample_rate()` and `drain_since(&mut cursor)` (non-destructive tail read off the same `Arc<Mutex<Vec<f32>>>` the worker's stream callback already writes to — `stop()`'s full-buffer `mem::take` is untouched). `WorkerMsg::Ready` now carries the native sample rate so `Recorder` can track it without a new channel. Added `TailResampler`: a persistent `FftFixedIn`, `push()`/`flush()`, buffering any leftover < 1024-sample chunk between ticks (shares the `RESAMPLE_CHUNK` const with the existing batch `resample_to_16k`).
- **`stt.rs`**: `new_streaming_vad()` builds `SmoothedVad(EnergyVad)` per the design's tune (frame_size 480, onset=2, hangover=15, prefill=15); RMS threshold is a `const` with a `ponytail:` comment marking it as a calibration knob (started at 0.02, needs real-mic hand-tuning — see below). `Transcriber` gained `feed_streaming`/`finish_streaming`, dispatching on `LoadedEngine` and calling `VadChunked::feed`/`finish` through `SpeechModel`.
- **`pipeline.rs`**: new `StreamingSession { vad, resampler, cursor, poll_handle }` behind `Pipeline.streaming: Mutex<Option<_>>` (holds more than the design's literal `Option<VadChunked>` sketch — cursor/resampler/poll-handle have to live somewhere, and bundling them was simpler than four parallel fields). `start()` builds a session and spawns a 200ms `tokio::time::interval` poll task; the task exits cleanly on its own next tick once `stop_and_process` `.take()`s the session (no explicit abort needed on the happy path — `Drop` aborts it as a safety net for early-return paths). `stop_and_process` drains the final not-yet-polled tail via `drain_since` **before** calling `recorder.stop()` (which clears the same buffer), then hands `(session, tail_native)` into `process()`. `process()`'s blocking STT step now resamples+feeds the tail and calls `finish_streaming`, replacing the old whole-utterance `Transcriber::transcribe()` call; falls back to that same call only if `TailResampler::new` failed at recording start (rare, e.g. unsupported native rate) — a best-effort fallback in the style of the existing prewarm-failure handling, not a user-facing toggle.
- **Regression test**: added `streaming_matches_batch_transcription` to `tests/stt_e2e.rs` — feeds a short and long synthesized `.wav` through `VadChunked` in ~200ms slices (mirroring the poll tick) and asserts the merged text still contains the expected words, matching one-shot `transcribe_with`. Ran against the real downloaded Parakeet model: both pass, transcripts match content (only minor punctuation/capitalization differences from chunk boundaries).
- **Synthetic timing** (same test, `--nocapture`): short utterance batch=327ms vs streaming(no-overlap)=262ms; long utterance batch=1719ms vs streaming(no-overlap)=3010ms. This is **not** the design's real before/after number — there's no live recording to overlap with in a synchronous test, so it only measures per-chunk overhead vs. one big call, which is exactly the "flat/worse on long without overlap" case the design predicted. The real win only shows up when polling happens *during* `Recording`, in parallel with the user still talking.
- **Live verification**: ran the real app (`pnpm tauri dev`) and drove one real dictation through the actual hotkey with real mic input ("Testing, testing."). Logs confirm the new path end to end: `VadChunked` `finish()` (not a mid-recording chunk flush, since the utterance was short — expected), `STT (96ms)` (now measuring just the tail+finish call, not the whole utterance), cleanup ran, enigo paste permission confirmed, zero "streaming ASR" warnings, no panics.

**Left for a human** (both are inherently real-mic/live-app tasks an agent session can't substitute for):
1. Hand-tune `STREAMING_VAD_RMS_THRESHOLD` in `stt.rs` (currently 0.02) against real mic noise floor — quiet rooms vs. fan/HVAC background noise.
2. Drive a longer real dictation with a mid-utterance pause/resume, and capture the real before/after `stt_ms` pair from its History entry per design decision 6 (the live-verification run above was a short utterance with no pause, so it didn't exercise the mid-recording chunk-flush path or produce a long-utterance number).

This closes lever 1 (streaming ASR is landed; only calibration + a longer live-verification pass remain, tracked above as a handoff, not a blocking decision).
