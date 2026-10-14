# Design: streaming ASR integration architecture

Type: grilling
Status: closed
Blocked by: (none)
Assignee: agent (claude)

## Question

[Research: streaming ASR feasibility via transcribe-rs VadChunked](05-streaming-asr-feasibility.md) recommends adopting `VadChunked` (CPU ONNX backend, per [Research: CoreML execution provider feasibility](02-coreml-feasibility.md)) for tail-latency reduction, not total-latency reduction. Decide the concrete integration design before implementation:

- `audio.rs`: how incremental 16kHz resampling + a chunk channel to a non-realtime consumer thread plug in, without touching the cpal callback's realtime constraints.
- Which `Vad` implementation and thresholds (`EnergyVad`/`SmoothedVad`, frame size, `VadChunkedConfig`'s `min_chunk_secs`/`max_chunk_secs`/`smart_split_search_secs`) — and how they'll be tuned/validated against real mic input, not just the crate's synthetic tests.
- `pipeline.rs`: exact lifecycle of a per-session `VadChunked` across `Idle`/`Recording`/`Processing` — when it's constructed, fed, and `finish()`-ed relative to today's `Stage` transitions.
- Whether intermediate chunk text should surface live in the UI/overlay, or stay purely internal (tail-latency reduction only, no UI change).
- How to measure this lever's before/after number given the destination's requirement (baseline's STT-only ~225-260ms number is whole-utterance; streaming's win is specifically in the tail after the last speech/silence boundary — needs its own measurement methodology, not a direct diff against the baseline number).

Resolve to a concrete design ready to implement (a follow-up task ticket will build it, mirroring [Design: prewarm audio capture architecture](03-prewarm-audio-design.md) → [Implement prewarm audio capture](07-prewarm-audio-implement.md)).

## Answer

Resolved via grilling. Six decisions, in dependency order:

1. **Sample delivery: poll-based, reuse the existing buffer.** No new channel from the cpal callback (`audio.rs::build_stream`'s closure stays exactly as it is today — realtime constraint untouched). A `tokio::time::interval` task (same pattern `pipeline.rs::finish()` already uses for its 3s error-clear timer) is spawned in `Pipeline::start()` when `Recording` begins, ticking every ~200ms.

2. **`VadChunked` lives on `Pipeline`, not as a new singleton.** New field `streaming: Mutex<Option<VadChunked>>` alongside the existing `target_app`/`clipboard_text` ephemeral-per-recording fields. Constructed in `start()`, fed by the poll task each tick, `finish()`-ed and cleared in `stop_and_process()`. No new Tauri-managed state — `VadChunked` is cheap to build (no model loading), unlike `Recorder`/`Transcriber`.

3. **`finish()`'s merged text replaces the whole-utterance `Transcriber::transcribe()` call.** That call is deleted from the `stop_and_process` → `process()` path entirely — no dual-path/settings toggle. This is what actually buys the latency win: by `Stop`, most audio was already chunk-transcribed during `Recording`, so finishing is just the last unpolled tail + `finish()`. Losing per-request `ParakeetParams::timestamp_granularity` (VadChunked drives models through the generic `SpeechModel` trait, not the concrete `transcribe_with`) is a non-issue — nothing downstream reads segments, only `raw_text`.

4. **VAD: `SmoothedVad` wrapping `EnergyVad`.** Zero new dependencies, no model download (`SileroVad` needs a new `vad-silero` feature + ONNX model — deferred, not adopted here). Starting tune from the crate's documented defaults: onset_frames=2, hangover_frames=15 (@480-sample/30ms frames ≈ 450ms), prefill_frames=15. The RMS threshold on the inner `EnergyVad` is the one knob to hand-tune against real mic input during `/verify` — this is the "real open risk" research ticket 05 flagged, and it's a calibration knob, not a value derivable up front.

5. **No UI change.** Merged text surfaces through the existing `dictation-complete` event, same shape as today. Live partial-chunk display in the overlay is a real future feature but is out of scope for this lever.

6. **Before/after measurement: two fixed test utterances, before/after per length.** Generate a short (~3s) and long (~12-15s monologue) `.wav` via `say -o ... --data-format=LEI16@16000`, reusing the project's `stt_e2e` test convention. Measure `stt_ms` (already recorded on `DictationResult`) for both utterances on `main` (before) and again after this lands (after). Report both pairs, not a single number — streaming is expected to look flat/worse on the short utterance (VAD onset/hangover + multiple small-chunk inference overhead vs. one big call) and win clearly on the long one (most of it already transcribed by the time `Stop` fires).

**Implementation plumbing** (mechanical, not decisions — for the follow-up implement ticket):
- `Recorder` gains a cloned `Arc<Mutex<Vec<f32>>>` handle to the worker's sample buffer (currently moved into `worker_loop` and not reachable from `Recorder` itself) plus a non-destructive `drain_since(&self, cursor: &mut usize) -> Vec<f32>` accessor, so the poll task can read new tail samples without disturbing `stop()`'s full-buffer return (still needed for `audio_seconds`/history).
- A persistent incremental resampler (stateful `rubato::FftFixedIn` instance reused across poll ticks, not the current one-shot `resample_to_16k`) turns native-rate tail slices into 16kHz chunks per tick, buffering any leftover < `CHUNK` samples between ticks.
- `Transcriber` gains `feed_streaming`/`finish_streaming` methods that dispatch on the loaded `LoadedEngine` variant (`Parakeet`/`Whisper`) and call `VadChunked::feed`/`finish` generically through the `SpeechModel` trait both engines already implement.

Follow-up: [Implement streaming ASR integration](10-streaming-asr-implement.md).
