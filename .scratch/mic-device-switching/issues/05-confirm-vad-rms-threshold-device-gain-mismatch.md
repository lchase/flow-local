# Confirm/deny: fixed VAD RMS threshold breaks capture on lower-gain mics

Type: task
Status: closed
Blocked by: (none)
Assignee: lawrence.chase

## Question

[Reproduce & log the desktop-mic switch-back failure](01-repro-desktop-mic-switch-back-failure.md) found the device-switch rebuild logic works correctly, but transcription silently fails ("Nothing transcribed", near-zero-sample chunks) on the desktop mic after the very first press, despite genuine speech. Leading hypothesis: `STREAMING_VAD_RMS_THRESHOLD = 0.02` in `src-tauri/src/stt.rs:24` — a single hardcoded value already flagged in-code as `// ponytail: starting guess, not derived` — doesn't cross onset for this mic's (lower?) gain, so `EnergyVad`/`VadChunked` treats real speech as silence.

Confirm or deny by instrumenting: temporarily log the RMS `level` value (`src-tauri/src/audio.rs:323`, already computed per-callback for the overlay waveform) alongside each VAD chunk decision, then repro speaking the same phrase into AirPods vs the desktop mic ("Sennheiser Profile"). Report the RMS ranges seen on each device relative to the 0.02 threshold.

If confirmed, this is a separate root cause from device-switch detection and should reframe what [Decide device-detection/fix strategy](03-decide-device-detection-strategy.md) is actually deciding.

## Resolution

Confirmed 2026-07-14 via temp instrumentation (logged resampled tail RMS alongside `feed_streaming` each ~200ms poll tick, `src-tauri/src/pipeline.rs`).

On "Sennheiser Profile" at normal desk distance, RMS during genuine speech ranged ~0.0005–0.028, mostly sitting *below* the 0.02 `STREAMING_VAD_RMS_THRESHOLD` and only briefly grazing it — never sustaining `onset_frames=2` consecutive frames above threshold, so `EnergyVad` never latches speech onset and the chunk transcribes as empty ("Nothing transcribed" despite genuine speech, exactly the reported symptom). Same phrase spoken close to the mic produced RMS 0.017–0.034, comfortably and consistently above threshold, and transcribed correctly ("Can you hear me better now?").

This is gain/distance sensitivity, not device-identity-specific — the same single mic fails or works purely based on how close the user is to it. Confirms the hypothesis: `STREAMING_VAD_RMS_THRESHOLD = 0.02` is too high for normal desk-distance speech on at least one real mic, and is not derived from any calibration process. Reframes [Decide device-detection/fix strategy](03-decide-device-detection-strategy.md): the fix isn't in device-switch detection (which already works, per [Reproduce & log the desktop-mic switch-back failure](01-repro-desktop-mic-switch-back-failure.md)) — it's the VAD threshold needing to adapt (lower default, and/or per-device or auto-calibrated gain) rather than being a single hardcoded constant.

Temp instrumentation reverted after confirming (see commit).
