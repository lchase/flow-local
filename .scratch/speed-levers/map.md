# Speed levers for implementation

## Destination

Ship 4 concrete speed levers identified from studying altic-dev/FluidVoice against Flow Local's dictation pipeline (hotkey → STT → cleanup → paste), each landed as working code with a measured before/after latency number:

1. Chunk-level streaming ASR overlapped with recording, via transcribe-rs's `VadChunked` `feed()` API.
2. Always-on prewarmed audio capture instead of a per-recording `cpal::Stream` rebuild.
3. CoreML/ANE execution provider for STT inference — resolved as a non-adoption: pin CPU-only ONNX explicitly with before/after numbers proving CPU wins (see Decisions so far), not an adoption of transcribe-rs's `ort-coreml` feature.
4. Tightened local-LLM cleanup speed (shared `reqwest::Client` + verified Ollama prefix-KV-cache reuse) rather than replacing Ollama.

## Notes

- Repo: Flow Local (macOS dictation app, Tauri v2 + Rust backend + React/TS frontend). See project `CLAUDE.md` for architecture/gotchas (cpal `!Send` worker-thread pattern, `time` crate pinned to 0.3.47, Ollama model tag `qwen3:4b-instruct`, Parakeet ITN digit normalization).
- **Destination overrides "plan, don't do."** Every ticket here should land working code plus a before/after number, not just a recommendation.
- Use the `/verify` or `/run` skill to confirm each lever's change actually works in the built app before closing its ticket, per this repo's normal convention (CLAUDE.md: "test the golden path... before reporting task as complete").
- Sequencing constraint (resolved): CoreML research came back "don't adopt" (see Decisions so far) — streaming ASR's final inference backend is the existing CPU ONNX path, not CoreML. Streaming's ticket is now free to proceed.
- The baseline-benchmark ticket blocks the first ticket of all 4 levers — no lever's "faster" claim ships without a before/after number against that baseline.

## Decisions so far

- [Baseline: measure current dictation pipeline latency](issues/01-baseline-latency.md) — steady-state stage numbers: audio cold-start ~170ms, STT-only ~225-260ms, cleanup-only ~620ms (prefix-KV-cache reuse confirmed working, ~160x prompt_eval speedup on repeat prompts). Full multi-dictation e2e wall-clock not measured (needs a human dictating); not blocking, since each lever's before/after only needs its own stage number.
- [Research: CoreML execution provider feasibility](issues/02-coreml-feasibility.md) — don't adopt: upstream benchmark on the exact model class (int8 Parakeet) shows CPU ONNX beats CoreML Auto by 3.4-7x. Whisper's separate `whisper-metal` path is unaffected. Raises a new question for lever 3's disposition (see frontier).
- [Design: prewarm audio capture architecture](issues/03-prewarm-audio-design.md) — always-on stream built once at app startup, gated by an atomic `recording` flag (idle callback is a no-op check); `start()` rebuilds only on an actual default-device change, not every recording; mic-in-use dot stays lit for the app's whole runtime, accepted as the tradeoff. Implementation split out to [Implement prewarm audio capture](issues/07-prewarm-audio-implement.md).
- [Fix Ollama cleanup speed: shared Client + verified prefix-cache reuse](issues/04-ollama-cleanup-speed.md) — landed a shared `LazyLock<reqwest::Client>` (~2x faster per call even on loopback); confirmed prefix-KV-cache reuse still holds (~125-140x prompt_eval speedup) once the system prompt carries real dictionary + app-profile content, not just the bare baseline prompt. Lever 4 done, no further Ollama tuning needed.
- [Research: streaming ASR feasibility via transcribe-rs VadChunked](issues/05-streaming-asr-feasibility.md) — adopt `VadChunked` (not `EnergyAdaptiveChunked`); works generically with Parakeet/Whisper via `SpeechModel`, no engine-specific blocker. Buys tail-latency reduction (work overlapped during Recording), not total-latency reduction. Real open risk: VAD threshold tuning against real mic input. Integration design split out to [Design: streaming ASR integration architecture](issues/08-streaming-asr-integration-design.md).
- [Decide disposition of lever 3 given CoreML regression](issues/06-lever3-disposition.md) — ship the negative result: pin `OrtAccelerator::CpuOnly` with a comment citing the CoreML research, plus before/after numbers proving CPU wins. Counts as lever 3's deliverable (non-adoption, not a speedup). Implementation split out to [Implement lever 3 CPU-only pin + benchmark](issues/09-lever3-cpu-pin-implement.md).
- [Implement prewarm audio capture](issues/07-prewarm-audio-implement.md) — landed always-on gated stream (long-lived worker thread + `AtomicBool` gate, rebuild only on device-name mismatch); steady-state `start()` measured ~0.18-0.4ms vs. baseline's ~170ms cold-start (400-900x). Verified live: real dictations through the full hotkey→STT→cleanup→paste path, plus a real mid-session device switch, both clean. Lever 2 done.
- [Design: streaming ASR integration architecture](issues/08-streaming-asr-integration-design.md) — poll-based tail-feed (200ms `tokio::time::interval`, reuses existing sample buffer, no new cpal-callback channel); `VadChunked` as a `Pipeline` field, not a singleton; `finish()`'s merged text replaces the whole-utterance `Transcriber::transcribe()` call outright (no dual-path toggle); VAD is `SmoothedVad(EnergyVad)` (no new deps/model); no UI change; before/after measured via a short+long test-utterance pair, not the baseline number. Implementation split out to [Implement streaming ASR integration](issues/10-streaming-asr-implement.md).
- [Implement lever 3 CPU-only pin + benchmark](issues/09-lever3-cpu-pin-implement.md) — landed `set_ort_accelerator(OrtAccelerator::CpuOnly)` in `Transcriber::new()`; real-hardware before/after confirms CPU wins (CpuOnly avg 191ms vs. CoreML Auto avg 345ms, ~1.8x — first local measurement backing issue 02's upstream-only numbers); e2e transcription + live app boot verified clean; live hotkey-UI drive was blocked by sandbox GUI-focus limits, not a code issue. Lever 3 done.
- [Implement streaming ASR integration](issues/10-streaming-asr-implement.md) — landed the design: `Recorder::drain_since`/`input_sample_rate` + `TailResampler` in `audio.rs`, `Transcriber::feed_streaming`/`finish_streaming` + `new_streaming_vad` in `stt.rs`, a 200ms poll task + `StreamingSession` in `pipeline.rs` replacing the whole-utterance `Transcriber::transcribe()` call on the `stop_and_process` path. Regression test passes against the real model; a real live dictation through the actual hotkey confirmed the path end-to-end with zero warnings/panics. Synthetic (no-overlap) timing only shows per-chunk overhead, not the real win (only visible when polling overlaps live recording). Left to a human: hand-tune the `EnergyVad` RMS threshold against real mic noise floor, and a longer live dictation with a pause/resume for the true before/after `stt_ms` pair. Lever 1 done.

## Not yet specified

(none — all in-scope fog has graduated into open tickets. All four levers are now landed; only human-only calibration/verification tasks noted above remain outside the map's scope of decisions.)

## Out of scope

- Replacing Ollama with an embedded local LLM runtime (e.g. llama.cpp bindings) for direct KV-cache control — ruled out; Ollama's own `keep_alive` + llama.cpp server-side slot-cache reuse covers this once verified/tightened (see lever 4 rescoping in [Fix Ollama cleanup speed](issues/04-ollama-cleanup-speed.md)).
- Adopting FluidVoice's true token-level streaming ASR model architecture (their CoreML end-of-utterance streaming model) — ruled out; using transcribe-rs's existing chunk-level `VadChunked`/`EnergyAdaptiveChunked` `feed()` API instead, no new model or runtime.
