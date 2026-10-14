# Implement lever 3 CPU-only pin + benchmark

Type: task
Status: closed
Blocked by: (none)
Assignee: agent (claude)

## Question

Land lever 3 per [Decide disposition of lever 3 given CoreML regression](06-lever3-disposition.md): ship the negative result, not an adoption.

- `stt.rs`: pin the ONNX execution provider to CPU-only explicitly (e.g. `OrtAccelerator::CpuOnly` or the transcribe-rs equivalent) at STT model init, with a comment citing [Research: CoreML execution provider feasibility](02-coreml-feasibility.md)'s numbers (CPU ~3.5us/sample vs. CoreML Auto ~12us/sample warm, ~24us/sample cold, int8 Parakeet) so a future change doesn't silently flip `ort-coreml` back on.
- Measure and record a before/after: "before" = current (already CPU by default, so this is really a regression-guard benchmark) vs. explicit CoreML-enabled run, proving CPU wins on this machine/model — reuse the harness from issue 02's research if one exists, otherwise a small bench matching its methodology.
- Use `/verify` or `/run` to confirm dictation still works end-to-end after the pin before closing.

## Answer

Landed in `src-tauri/src/stt.rs`: `Transcriber::new()` now calls `set_ort_accelerator(OrtAccelerator::CpuOnly)` before any model load, with a comment citing [Research: CoreML execution provider feasibility](02-coreml-feasibility.md)'s numbers. This runs unconditionally at app startup (single global preference), so it forecloses a future `ort-coreml` feature flip silently changing behavior.

**Before/after (real hardware, this machine, int8 Parakeet, `examples/stt_accelerator_bench.rs`, 10 runs after 1 warm-up):**
- `CpuOnly`: avg 191.4ms
- `Auto` (CoreML, via a temporary `ort-coreml` forwarding feature added to `Cargo.toml` for the run only, then reverted): avg 345.1ms

CPU wins by ~1.8x on this machine — same direction as issue 02's upstream numbers (that research had no macOS hardware available; this is the first real local measurement confirming it). The bench file's doc comment records how to temporarily re-enable the `ort-coreml` feature to reproduce.

**Verified via `/verify`:**
- Real `pnpm tauri dev` launch → Parakeet model loads successfully under the pin, no crash (`parakeet-tdt-0.6b-v3 loaded in 1.56s`).
- `cargo test --test stt_e2e -- --ignored --nocapture` → passes against real downloaded model + real audio, transcription correct under `CpuOnly`.
- Live hotkey-triggered dictation could not be automated from the sandboxed verification shell (GUI focus stays with the sandbox's own controlling process, so synthetic key events never reach the real app session) — this is an environment limitation, not a code gap. Risk is low given the change is a one-line pin ahead of a model-load path already covered by the e2e test; a human doing one manual real dictation is the one remaining confirmation.

Lever 3 done — non-adoption shipped with working code + a real before/after number, per [Decide disposition of lever 3 given CoreML regression](06-lever3-disposition.md).
