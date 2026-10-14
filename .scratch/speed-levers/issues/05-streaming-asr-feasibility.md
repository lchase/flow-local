# Research: streaming ASR feasibility via transcribe-rs VadChunked

Type: research
Status: closed
Blocked by: 02
Assignee: agent (claude)

## Question

transcribe-rs's `transcriber::VadChunked` (and `EnergyAdaptiveChunked`) expose a `feed()`/`finish()` API, generic over any `SpeechModel` — investigate whether it fits our dictation pipeline:

- Does `feed()` support true incremental feeding during an in-progress recording (not just post-hoc chunking of a completed file)?
- Does it work correctly with the Parakeet `SpeechModel` implementation specifically (our default model)?
- Latency/accuracy tradeoff of chunk boundaries — VAD-based silence detection vs. fixed energy-adaptive chunks — for our use case (short-to-medium dictation utterances, not long-form).
- What changes this requires in `stt.rs`/`pipeline.rs`'s Idle/Recording/Processing state machine to feed audio chunks as they arrive from `audio.rs`, rather than only transcribing once at recording-stop.
- Should be benchmarked against whichever inference backend [Research: CoreML execution provider feasibility](02-coreml-feasibility.md) recommends adopting (this ticket is blocked on that one landing).

Produce a recommendation for whether/how to adopt it.

## Answer

**Recommendation: adopt `VadChunked` (not `EnergyAdaptiveChunked`).** Read the actual `transcribe-rs` 0.3.11 source on GitHub (`src/transcriber/{mod,vad_chunked}.rs`, `src/onnx/parakeet/mod.rs`) rather than relying on docs.rs summaries, since docs.rs didn't render method bodies.

- **`feed()` supports true incremental live feeding, not just post-hoc file chunking.** The doc comment on `VadChunked` states this directly ("Works for both live audio (small per-frame feeds) and file transcription (one large feed)"), and the crate's own unit tests exercise it exactly that way: multiple small `feed()` calls across separate calls, with a `pending` buffer carrying sub-frame remainders forward between calls. `transcribe_file`/`transcribe` (the whole-file convenience path) are just `feed()` + `finish()` wrappers over the same state machine — there's no separate "batch mode."
- **Works with Parakeet with no special-casing.** The model is passed as `&mut dyn SpeechModel` per chunk — `Transcriber` is a trait, object-safe (`Box<dyn Transcriber>` is tested), generic over any engine. `ParakeetModel::transcribe_with` already zeroes its RNN-T decoder state fresh on every call (`create_decoder_state` allocates zeroed arrays each invocation) — so it has no persistent streaming state to violate by calling it repeatedly per chunk; each chunk is transcribed as an independent short utterance today already (via our own `keepalive()`/`transcribe`), and `VadChunked` just does that same thing at chunk boundaries instead of once at the end. Whisper (`WhisperEngine`) implements the same `SpeechModel` trait, so nothing here is Parakeet-only.
- **VAD-based chunking over energy-adaptive, for our workload.** Our recordings are single short-to-medium dictation utterances (hold hotkey, speak a sentence or two, release), not long-form audio. `EnergyAdaptiveChunked` is fixed-duration chunking with energy-based split search — built for long monologues where *some* chunk boundary is unavoidable. `VadChunked` only splits at real speech→silence boundaries (plus a `max_chunk_secs` force-split safety valve with optional low-energy backward search to avoid mid-word cuts) — for a single utterance under `max_chunk_secs`, it will typically emit zero intermediate chunks and do all its work in `finish()`, which is fine: the win isn't "many parallel chunks," it's "the last chunk transcribed at `finish()` is short," because everything before the final pause was already transcribed during `Recording`.
- **What this actually buys us: tail-latency reduction, not total-latency reduction.** The STT-only baseline (~225-260ms, see [Baseline: measure current dictation pipeline latency](01-baseline-latency.md)) is paid today entirely *after* the hotkey is released, on the whole utterance. With streaming, most of that work happens *during* `Recording` (overlapped with the user still talking); only the tail after the last speech→silence boundary remains at release time. Total STT compute doesn't shrink — chunk boundaries mean the model runs its encoder pass more times, not fewer — but perceived latency (release-to-paste) drops by roughly "however much audio was left unflushed at release," which for natural speech-then-pause dictation is small.
- **Already benchmarked against the right backend.** Per [Research: CoreML execution provider feasibility](02-coreml-feasibility.md), CoreML is not being adopted — CPU ONNX is the backend `VadChunked` chunks will run through, so no re-benchmarking needed here.

**Required changes, sized (design left to a follow-up ticket, not decided here):**
- `audio.rs`: today the cpal callback pushes raw native-rate samples into an `Arc<Mutex<Vec<f32>>>`, resampled to 16kHz *once* at `stop()`. Streaming needs 16kHz frames available *during* recording — either resample incrementally (the existing `FftFixedIn` resampler already operates in fixed `CHUNK` blocks, so it can run per-block instead of once at the end) and hand each resampled block to a channel, consumed by a dedicated non-realtime thread (never the cpal callback thread — STT inference is CPU-bound and must not run on the audio-realtime path, same reason `audio.rs`'s worker thread pattern exists today per this repo's cpal `!Send` gotcha).
- `stt.rs`: no new engine machinery — `ParakeetModel`/`WhisperEngine` already implement `transcribe_rs::SpeechModel`. Just need `&mut dyn SpeechModel` access from the existing `Transcriber`'s `Mutex<Option<(String, LoadedEngine)>>` during the recording window, locked briefly per chunk (same short-hold pattern `keepalive()` already uses).
- `pipeline.rs`: a per-session `VadChunked` (wrapping some `Vad` — crate ships `EnergyVad`/`SmoothedVad`) is created at `start()`, fed by the new audio-chunk consumer while `Stage::Recording`; `stop_and_process()` calls `finish()` instead of today's one-shot `transcriber.transcribe(&samples)` over the full buffer.
- **Real risk, not yet resolved here:** VAD threshold tuning against actual mic input/noise floors. The crate's `EnergyVad` is a simple RMS-threshold VAD — no unit test proves it behaves well on real speech; wrong thresholds either fragment mid-word (chunk too eager) or never flush before `finish()` (chunk too lax, erasing the whole benefit). This needs empirical tuning on a real Mac mic, not just the crate's synthetic tests.
- Whether/how partial text surfaces in the UI live, versus staying purely an internal tail-latency trick, is an explicit design decision, not implied by this research — left to the integration-design ticket below.
