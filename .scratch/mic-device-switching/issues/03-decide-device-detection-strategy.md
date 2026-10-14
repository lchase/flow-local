# Decide VAD gain/threshold fix strategy

Type: grilling
Status: closed
Blocked by: 01, 02, 05
Assignee: lawrence.chase

## Question

[Reproduce & log the desktop-mic switch-back failure](01-repro-desktop-mic-switch-back-failure.md) and [Confirm/deny: fixed VAD RMS threshold breaks capture on lower-gain mics](05-confirm-vad-rms-threshold-device-gain-mismatch.md) confirmed the root cause is `STREAMING_VAD_RMS_THRESHOLD = 0.02` in `src-tauri/src/stt.rs` — a single hardcoded value that genuine desk-distance speech on at least one real mic doesn't reliably cross, causing silent "Nothing transcribed" failures. Separately, [Research: cpal/CoreAudio device-change notification](02-research-cpal-device-change-notification.md) already settled that the existing poll-on-hotkey-press device-switch detection itself doesn't need to change (no push-based monitoring needed).

So the remaining decision is purely about the VAD threshold, not device-switch detection: lower the hardcoded default, calibrate per-device (e.g. sample ambient/speech RMS on `Cmd::Start` or `prewarm()` and set a per-session threshold), auto-calibrate continuously, switch to a different VAD approach (e.g. `SmoothedVad`'s existing hangover/onset tuning is enough once the threshold itself is sane, or swap `EnergyVad` for `SilenceVad`/Silero-based detection already available in `transcribe_rs`), or something else entirely?

## Resolution

Strategy: **per-device calibration measured from actual speech**, not a static default and not continuous/noise-floor auto-calibration.

Ruled out along the way, with reasons:
- Just lowering the static default: rejected upfront — still one hardcoded value, doesn't adapt across mics.
- Continuous auto-calibration during recording: rejected — risks the threshold silently drifting mid-utterance.
- Swapping `EnergyVad`/`SmoothedVad` for `SileroVad`: rejected — sits behind the unused `vad-silero` feature and needs its own separately-downloaded ONNX model (same sha256/size machinery as `stt.rs::MODELS`); not worth the weight when the existing energy-VAD design works fine once the threshold is sane.
- Automatic noise-floor guess from the first ~200-300ms of each recording: rejected — `audio.rs`'s RMS `level` is only computed while `recording` is true (early-returns otherwise, `audio.rs:308`), and `prewarm()` only runs once at app launch (`lib.rs:332`), not per device-switch, so there's no reliable silence-only window to sample; a quiet speech onset is indistinguishable from ambient noise by RMS alone in that window, reproducing the same bug.

Mechanism, confirmed: derive the threshold from a measurement of **actual speech RMS** for the current device (not noise), persisted keyed by device name so a device only needs measuring once. Fallback for a device with no stored calibration yet (first-ever use, or before the user runs whatever UI 04 decides): **keep the current static `0.02` default** — an uncalibrated device stays exactly as broken as today rather than risking a lower conservative guess that trades in more false-positive noise triggers.

Explicitly deferred, not decided here: *how* the user triggers/sees this (dedicated "Calibrate mic" action, implicit measurement on first utterance, a manual sensitivity slider instead of measurement at all, or none of the above/fully invisible) — that's [Decide whether the VAD fix is user-facing](04-decide-if-detection-mode-is-user-facing.md), which was already blocked on this ticket and is now unblocked.
