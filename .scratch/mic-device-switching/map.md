# Mic device-switching strategy

## Destination

Root-cause diagnosis of the desktop-mic switch failure, plus a decision on the fix strategy — handed off for implementation after. Root cause confirmed as a hardcoded VAD RMS threshold that doesn't adapt to per-device/distance gain (not device-switch detection, which is already settled as fine as-is); remaining decision is how to fix the threshold and whether that's user-facing.

## Notes

- Domain: `src-tauri/src/audio.rs` (`Recorder`, `ensure_stream`, `worker_loop`) — device selection is entirely cpal's `host.default_input_device()`, compared by name against the currently-built stream on every hotkey press (`Cmd::Start`) and at `prewarm()`.
- Cleanup-step (Ollama) latency is out of scope here — capture-side timing isn't the bottleneck per the reporting user; don't let that motivate the mode decision.
- Use `/grilling` and `/domain-modeling` for the decision tickets; `/diagnosing-bugs` framing may help structure the repro ticket.
- This map is a decision spec only — tickets close with an answer, not a shipped fix. Implementation is a follow-up effort.
- **Map closed 2026-07-14, implementation landed** per tickets 03/04/06's decisions: `settings.rs` (`AppSettings::mic_calibration`), `stt.rs` (`new_streaming_vad` takes a threshold param), `audio.rs` (`Recorder::current_device_name`/`take_stream_error`), `pipeline.rs` (calibration lookup in `start_streaming`, whole-utterance retry + back-fill in `process`, mid-recording unplug abort in the streaming poll loop). `cargo check`/`cargo test --lib`/`tsc --noEmit` all clean; not yet live-verified against real hardware (the original repro was on the reporting user's actual Sennheiser mic).

## Decisions so far

- [Reproduce & log the desktop-mic switch-back failure](issues/01-repro-desktop-mic-switch-back-failure.md) — device-switch rebuild logic works correctly (fires, names both devices right, first press transcribes); real failure is downstream — genuine speech on the new device produces near-zero-sample chunks and "Nothing transcribed" on later presses. Leading hypothesis: hardcoded `STREAMING_VAD_RMS_THRESHOLD` doesn't adapt to per-device gain — see [Confirm/deny: fixed VAD RMS threshold breaks capture on lower-gain mics](issues/05-confirm-vad-rms-threshold-device-gain-mismatch.md).
- [Research: does cpal/CoreAudio device-change notification](issues/02-research-cpal-device-change-notification.md) — no push API exists in cpal; CoreAudio itself supports it via `AudioObjectAddPropertyListener`, reachable through `objc2-core-audio`, but not worth adding — existing poll-on-hotkey-press (`ensure_stream`) is already correct-at-the-moment-of-recording, and the only gap (mid-recording default-device swap) self-corrects next press. Real actionable gap found instead: cpal's `StreamError::DeviceNotAvailable` disconnect signal is already wired to `audio.rs`'s error callback but only logged, never surfaced — split out to [Decide how to surface mid-recording device-unplug errors](issues/06-surface-device-not-available-error.md).
- [Confirm/deny: fixed VAD RMS threshold breaks capture on lower-gain mics](issues/05-confirm-vad-rms-threshold-device-gain-mismatch.md) — confirmed via temp RMS instrumentation: on "Sennheiser Profile" at normal desk distance, genuine-speech RMS mostly sits below the 0.02 `STREAMING_VAD_RMS_THRESHOLD` and rarely sustains onset; held close to the mic it's well above and works fine. Gain/distance sensitivity, not device-identity-specific. Confirms the real root cause is the hardcoded VAD threshold, not device-switch detection.
- [Decide VAD gain/threshold fix strategy](issues/03-decide-device-detection-strategy.md) — threshold gets derived from a measurement of actual per-device speech RMS (not a noise-floor guess, not continuous auto-calibration, not a SileroVad swap), persisted keyed by device name. Uncalibrated devices fall back to the current static `0.02` (safer than a lower guess that trades in false positives). How the user triggers the measurement is deferred to [Decide whether the VAD fix is user-facing](issues/04-decide-if-detection-mode-is-user-facing.md).
- [Decide whether the VAD fix is user-facing](issues/04-decide-if-detection-mode-is-user-facing.md) — fully invisible, no Settings surface. On an empty streaming-VAD result, fall back to whole-utterance transcription; a successful recovery confirms genuine speech in the captured samples, which back-fills the per-device calibration. Self-heals from the next press with no dedicated calibration step.
- [Decide how to surface mid-recording device-unplug errors](issues/06-surface-device-not-available-error.md) — abort the in-flight recording immediately on `StreamError::DeviceNotAvailable`, discarding partial audio, via the existing generic `pipeline-state: error` path (already rendered by `Overlay.tsx`, already used for other mic errors). No dependency on next-press recovery — that already works per [Research: does cpal/CoreAudio device-change notification](issues/02-research-cpal-device-change-notification.md).

## Not yet specified

(none — confirmed moot during implementation: `start_streaming` looks up `mic_calibration` by `Recorder::current_device_name()` fresh on every recording, independent of `ensure_stream`'s own device-rebuild check; no migration path needed, a stale-device stream just rebuilds as it always did and calibration lookup follows whichever device is actually active.)

## Out of scope

- Cleanup-step (Ollama) latency investigation — a separate effort if pursued at all.
- Manual device-selection UI (pinning a specific mic in Settings, overriding the OS default) — not part of this map.
