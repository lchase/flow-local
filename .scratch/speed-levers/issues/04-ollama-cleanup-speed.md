# Fix Ollama cleanup speed: shared Client + verified prefix-cache reuse

Type: task
Status: closed
Blocked by: 01
Assignee: agent (claude)

## Question

`cleanup.rs` already sets `"keep_alive": -1` (model stays loaded) and has a `warm_model()`/`warm_ollama` command — the "keep model warm" half of this lever is already shipped. Two remaining gaps:

1. `reqwest::Client::new()` is called fresh in three separate places in `cleanup.rs` instead of sharing one pooled `Client` (no connection reuse — a fresh TCP connection per request). Fix this to share a single client instance.
2. Ollama's llama.cpp backend does automatic prefix-KV-cache reuse per model slot when consecutive requests share an identical prefix. Verify this is actually triggering for our system prompt: measure repeat-call latency before/after the Client fix, and check whether `build_system_prompt`'s output is truly byte-stable call-to-call (dictionary/app-profile content could be breaking cache reuse without us noticing).

Land the Client fix. If prefix-cache reuse isn't triggering, identify why and fix it if feasible within this ticket's scope — otherwise record what's blocking it as a follow-up.

## Answer

**Landed.** `cleanup.rs` now has one module-level `static CLIENT: LazyLock<reqwest::Client>` (`std::sync::LazyLock`, stable since Rust 1.80, no new dependency), used by `clean_transcript`, `warm_model`, and `ollama_status` instead of each calling `reqwest::Client::new()` separately.

**Before/after** (localhost, throwaway bench: `src-tauri/examples/ollama_client_bench.rs`, `cargo run --release --example ollama_client_bench`), 5 sequential `GET /api/tags` calls each way, steady-state runs 2-5:
- Fresh `Client::new()` per call: ~2.6-3.4ms/call
- Shared `Client` (fixed): ~1.0-1.6ms/call

Roughly 2x faster per call even on loopback (connection-pool reuse skips a fresh `connect()` each time); the win would be larger over a real network path to a remote Ollama host.

**Prefix-cache reuse confirmed with real (non-empty) dictionary + app-profile content** — the gap this ticket was checking, since the baseline ticket only measured the bare `BASE_PROMPT` with no dictionary/mode fragment. Same bench, `/api/chat` with a system prompt carrying a populated dictionary clause + the "code" mode fragment (mirroring `build_system_prompt`'s non-empty-dictionary/code-mode shape):
- Call 1 (cold): wall 5.37s, `prompt_eval_duration` 4710ms
- Calls 2-3 (repeat, byte-identical prompt): wall ~600ms, `prompt_eval_duration` ~33-38ms → **~125-140x speedup**, consistent with the baseline's ~160x bare-prompt result.

`build_system_prompt` is a pure function of `&AppSettings`/`Option<&FrontmostApp>` with no per-call randomness, so it's byte-stable call-to-call as long as settings/app don't change mid-session (the common case: same app, same dictionary, repeated dictations) — dictionary/app-profile content does not break cache reuse. No further Ollama/prompt-structure tuning needed; closing the "Not yet specified" follow-on this ticket's resolution was gating.

Linked asset: `src-tauri/examples/ollama_client_bench.rs` (kept, reusable for future connection/cache checks).
