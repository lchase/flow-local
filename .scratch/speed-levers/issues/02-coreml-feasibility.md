# Research: CoreML execution provider feasibility

Type: research
Status: closed
Blocked by: 01
Assignee: agent (claude)

## Question

transcribe-rs exposes an `ort-coreml` Cargo feature (`ort/coreml` execution provider) — investigate whether enabling it works correctly for our STT models:

- Accuracy parity: does it produce correct transcriptions vs. the current CPU ONNX path, for both Parakeet v3 int8 (default) and the Whisper models?
- Latency delta: measure against the baseline STT-only number from [Baseline: measure current dictation pipeline latency](01-baseline-latency.md).
- Platform coverage: does it work on Apple Silicon and Intel, given `tauri.conf.json`'s `minimumSystemVersion: 12.0` — do we still need to support Intel Macs?
- Failure behavior: what happens if the CoreML EP is unavailable or fails to initialize — crash, silent fallback, or something else?
- Build/packaging/signing implications of adding the feature.

Produce a recommendation: adopt it (and how — always-on vs. feature-flagged with fallback), or don't.

## Answer

**Recommendation: don't adopt `ort-coreml` for Parakeet (our default STT model).** Real benchmark data on our exact model class shows it's a net regression, not a speedup.

**Latency delta (decisive):** `cjpais/transcribe-rs` issue #86 (closed 2026-07-02, "CPU-only beats CoreML Auto for int8 Parakeet on macOS") — a user benchmarked `OrtAccelerator::CpuOnly` vs `OrtAccelerator::Auto` (CoreML) for int8 Parakeet:
- CPU-only: ~3.5us/sample
- CoreML Auto, warm: ~12us/sample (3.4x slower than CPU)
- CoreML Auto, cold: ~24us/sample (6.9x slower than CPU)

This matches our setup exactly (Parakeet v3 int8, the same quantization class). Community explanation (onnxruntime issue threads) is consistent: CoreML EP support for int8-quantized ops is inconsistent — some patterns force dequant stubs or CPU↔ANE round-trips per unsupported node, which can make CoreML slower than plain CPU ONNX for quantized models. No macOS hardware was available in this session to re-run the benchmark ourselves; treating the upstream, on-point, recent result as sufficient to not invest further without contrary evidence.

**Feature/build:** `ort-coreml` is a real feature on `transcribe-rs` 0.3.11 (gated behind `onnx`, already enabled in our `Cargo.toml`). Described upstream as "lightweight" (no extra bundled binary, unlike `ort-webgpu`'s ~5MB Dawn DLL) — so adoption cost would've been low if it had paid off.

**Failure behavior:** `OrtAccelerator`/`GpuProvider` selection tries the chosen EP first and falls back to CPU silently per-node on failure (standard onnxruntime EP partitioning, corroborated by transcribe-rs PR #49's description). No crash risk either way — moot now given the recommendation.

**Platform coverage:** CoreML is macOS-only, available on both Apple Silicon and Intel (Intel lacks the Neural Engine, so no Apple Silicon-only benefit story either) — moot given the recommendation.

**Accuracy parity:** not verified — no macOS hardware in this session, and the perf regression alone is enough to not pursue this further. Flagging as an open gap only if adoption is revisited later.

**Consequence for the map:** Whisper's GPU acceleration is separate (`whisper-metal`, via whisper.cpp's own Metal backend, not this `ort` CoreML EP) and unaffected — already enabled in `Cargo.toml`, no change needed. For lever 1 (streaming ASR), this *resolves* rather than blocks: "the final inference backend" streaming should target is the existing CPU ONNX path, since CoreML isn't being adopted.

This also means lever 3 as named in the map's Destination ("CoreML/ANE execution provider for STT inference") doesn't pan out as a speed lever — see [Decide disposition of lever 3 given CoreML regression](06-lever3-disposition.md), opened to work out with the user how the map should handle this.
