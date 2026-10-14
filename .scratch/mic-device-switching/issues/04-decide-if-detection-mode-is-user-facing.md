# Decide whether the VAD fix is user-facing

Type: grilling
Status: closed
Blocked by: 03
Assignee: lawrence.chase

## Question

Once [Decide VAD gain/threshold fix strategy](03-decide-device-detection-strategy.md) settles how the app fixes the RMS-threshold gain mismatch, does it need to be exposed as a user-facing Settings control (e.g. a mic sensitivity slider, a "recalibrate" button), or is it purely an internal implementation fix (lower default / auto-calibration) with no user-visible mode?

## Resolution

**Fully invisible — no Settings surface at all.** Ruled out along the way: a dedicated "Calibrate mic" action/section and a manual sensitivity slider were both considered and rejected — both would be greenfield UI (confirmed via recon: no "Microphone"/"Audio" section exists in `src/pages/Settings.tsx` today, and no `#[tauri::command]` anywhere exposes the current device name or a device list to the frontend), and both ask the user to notice a problem and go do something about it.

Mechanism: today, when the streaming VAD returns empty (`raw_text.is_empty()` in `src-tauri/src/pipeline.rs:366`), the pipeline just fails with "Nothing transcribed" — there's no fallback to the whole-utterance `Transcriber::transcribe(&samples)` path, even though that path exists and doesn't depend on `STREAMING_VAD_RMS_THRESHOLD` at all. The fix: on an empty streaming result, retry with whole-utterance transcription; if that succeeds, treat the recovered text as confirmation that the captured samples contain genuine speech, and use those samples' real RMS to back-fill the per-device calibration decided in [Decide VAD gain/threshold fix strategy](03-decide-device-detection-strategy.md). This self-heals starting the very next press on that device, with no dedicated calibration step and no user-visible mode. Tradeoff accepted: one extra whole-utterance transcription pass' latency, but only on an already-broken take.

This resolves the map's "Not yet specified" item about Settings UI/copy shape — there is none; it's dropped as no-longer-applicable rather than graduated.
