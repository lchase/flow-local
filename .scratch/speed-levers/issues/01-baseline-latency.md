# Baseline: measure current dictation pipeline latency

Type: task
Status: closed
Blocked by: (none)
Assignee: agent (claude)

## Question

Measure current end-to-end dictation latency on today's main branch, broken into stages, so every lever ticket in this map can quote a before/after against a shared number:

- Audio cold-start: time from hotkey-press to first sample captured (current `audio.rs::start()` builds a fresh `cpal::Stream` + worker thread every recording).
- STT-only latency: transcribe time for a fixed test WAV (reuse the `cargo test --test stt_e2e -- --ignored --nocapture` fixture approach from CLAUDE.md), model already loaded.
- Cleanup-only latency: Ollama `/api/chat` call time for a fixed sample transcript, model already warm (`keep_alive: -1` already set).
- Full end-to-end wall clock for a handful of real dictations (hotkey release → text pasted).

Record the numbers and methodology in the resolution so later tickets can reproduce the same measurement.

## Answer

Machine: this dev Mac, Ollama already serving `qwen3:4b-instruct` with `keep_alive: -1` (warm), Parakeet v3 int8 model already downloaded. All numbers below are steady-state (post-warmup); first-call numbers noted separately since they hit one-time init cost, not per-recording cost.

**Audio cold-start** (host+device+config+`build_input_stream`+`play`, mirroring `audio.rs::start()`'s stream construction): measured with a throwaway microbenchmark, `src-tauri/examples/audio_coldstart_bench.rs` (`cargo run --release --example audio_coldstart_bench`), 5 runs:
- Run 1 (process cold): 314.6ms
- Runs 2-5 (steady state): 159.7ms, 172.8ms, 175.2ms, 172.2ms → **~170ms per recording**, paid on every hotkey-press today since `start()` rebuilds the stream each time. This is exactly the cost lever 2 (prewarm audio) should eliminate to near-zero.

**STT-only latency** (`ParakeetModel::transcribe_with`, model already loaded, fixed WAV via `say -o /tmp/stt_test.wav --data-format=LEI16@16000 "Let's meet at two, actually three PM. Thanks so much for setting this up."`): `cargo test --test stt_e2e -- --ignored --nocapture`, 3 runs (each a fresh process, so includes one model-load per run outside the timed span):
- 259.3ms, 225.6ms, 225.4ms → **~225-260ms** transcribe-only, this is the number levers 1 and 3 (streaming ASR, CoreML) should move.

**Cleanup-only latency** (`POST /api/chat`, same body shape as `cleanup.rs::clean_transcript` — real `BASE_PROMPT`, no dictionary/app-profile, `temperature: 0.2`, non-streaming, model already warm): 3 calls, transcript = the STT output above:
- Call 1 wall: 5.643s (`prompt_eval_duration`: 4977ms, `eval_duration`: 390ms) — first time Ollama sees this exact prompt, no prefix-cache hit yet.
- Calls 2-3 wall: 0.625s, 0.621s (`prompt_eval_duration`: ~31ms, `eval_duration`: ~388ms) → **~620ms** once the prefix is cached.
- **Prefix-KV-cache reuse is confirmed working today** (`prompt_eval_duration` drops ~160x, 4977ms → 31ms, for byte-identical repeat prompts) — directly informs lever 4's ticket: the remaining win there is the shared-`reqwest::Client` fix plus verifying real dictation traffic (varying dictionary/app-profile) still hits this cache, not re-proving the cache mechanism exists.

**Full end-to-end (hotkey-release → paste)**: not measured with real speech — requires a human to actually press the hotkey and dictate, which this agent session can't do. Not needed to unblock any of the 4 lever tickets: each lever targets exactly one stage above (audio / STT / STT / cleanup), and that stage's before/after is sufficient to prove its own claim. Sum of steady-state stages ≈ 170ms (audio) + 240ms (STT) + 620ms (cleanup) + inject (not measured, expected low-single-digit-ms clipboard+paste) ≈ **~1.0-1.1s**, as a rough end-to-end estimate — not a substitute for a real measurement. If a real multi-dictation wall-clock number becomes useful later, whoever picks that up should: build the app (`pnpm tauri dev`), dictate 3-5 varied real phrases, and time hotkey-release-to-paste-complete by eye or by adding temporary `log::info!` timestamps at the `pipeline.rs` state transitions.

Linked asset: `src-tauri/examples/audio_coldstart_bench.rs` (kept — directly reusable for lever 2's own before/after measurement).
