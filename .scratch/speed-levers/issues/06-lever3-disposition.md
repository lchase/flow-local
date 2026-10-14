# Decide disposition of lever 3 given CoreML regression

Type: grilling
Status: resolved
Blocked by: (none)

## Question

[Research: CoreML execution provider feasibility](02-coreml-feasibility.md) found that `ort-coreml` is a net regression for our default model (int8 Parakeet): CPU-only ~3.5us/sample vs. CoreML Auto ~12us/sample warm, ~24us/sample cold — a real upstream benchmark on the exact model class we use. Adopting CoreML would make dictation slower, not faster.

The map's Destination commits to shipping 4 levers, each "landed as working code with a measured before/after latency number." Lever 3 as scoped ("CoreML/ANE execution provider for STT inference") can't be landed as a speedup — the honest before/after shows CoreML losing.

Decide with the user how to handle this:
- Ship the negative result itself as lever 3's deliverable — e.g. a `set_ort_accelerator(OrtAccelerator::CpuOnly)` pin with a comment citing the finding, plus the before/after numbers proving CPU is faster (technically "working code + a number," but it's a non-adoption, not a lever)?
- Drop lever 3 and redraw the Destination to 3 levers (or substitute a different 4th lever)?
- Something else (e.g. revisit only if/when transcribe-rs's CoreML int8 support improves)?

Whatever is decided, update the map's Destination/Out of scope accordingly.

## Answer

Ship the negative result as lever 3's deliverable: pin `OrtAccelerator::CpuOnly` (or equivalent explicit CPU-only ONNX config) in the STT init path, with a comment citing [Research: CoreML execution provider feasibility](02-coreml-feasibility.md)'s numbers, plus a before/after benchmark proving CPU wins. This counts as lever 3's "working code + measured number" — it's a non-adoption pin, not a speedup adoption, and it forecloses accidental future regression (someone flipping the `ort-coreml` feature on without re-checking).

Follow-up implementation split out to [Implement lever 3 CPU-only pin + benchmark](09-lever3-cpu-pin-implement.md), mirroring the design/implement split used for lever 2 (03 → 07).
