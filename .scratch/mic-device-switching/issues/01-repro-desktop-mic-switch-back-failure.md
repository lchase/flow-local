# Reproduce & log the desktop-mic switch-back failure

Type: task
Status: closed
Blocked by: (none)
Assignee: lawrence.chase

## Question

When AirPods disconnect (or macOS otherwise falls back to the desktop mic) mid-session, does the `audio: default input device changed to {device_name:?}, rebuilding stream` log line in `src-tauri/src/audio.rs` fire? What device names does cpal report for the currently-built stream vs. the new default, immediately before and after the switch?

Repro steps:

1. `RUST_LOG=info pnpm tauri dev`
2. Hold the hotkey once while on AirPods — confirm it transcribes correctly.
3. Switch back to the desktop mic (unplug/disconnect AirPods, or reselect via System Settings).
4. Hold the hotkey again — capture the full log around this press.
5. Report: did the rebuild log line appear? What were the two device names? Did the resulting transcription actually come from the desktop mic or still from AirPods (silence/wrong audio)?

## Resolution

Repro'd 2026-07-14. Log (`RUST_LOG=info`):

```
[14:49:51] audio: building stream for "Dad Airpod Pros" @ 24000Hz x1 (F32)
...
[14:50:20] audio: default input device changed to "Sennheiser Profile", rebuilding stream
[14:50:20] audio: building stream for "Sennheiser Profile" @ 48000Hz x1 (F32)
[14:50:23] STT (141ms): Transcription.          <- first press post-switch: worked
[14:50:28] pipeline failed: Nothing transcribed  <- chunk duration 0.02s
[14:50:43] pipeline failed: Nothing transcribed  <- chunk duration 0.00s
```

**The rebuild/name-comparison logic in `ensure_stream` is not the bug** — it fired correctly, named both devices correctly, and the very first press after the switch transcribed correctly. The failure is downstream: subsequent presses on the *same already-rebuilt* Sennheiser stream produce near-zero-sample VAD chunks despite the user confirming they spoke normally into the mic each time ("Nothing transcribed" — not silence, not misdetection of the switch).

User also reports this only became a problem after moving to the streaming/`VadChunked` mechanism — worked fine under the prior (non-streaming) transcription path.

Root-cause hypothesis (not yet confirmed): `STREAMING_VAD_RMS_THRESHOLD = 0.02` in `src-tauri/src/stt.rs:24` is a single hardcoded value, explicitly flagged in-code as `// ponytail: starting guess, not derived`. It's calibrated against whatever mic was on hand during development and never varies per input device. If a different mic (e.g. this USB "Sennheiser Profile") outputs lower RMS for the same speech, `EnergyVad` never crosses onset threshold — `VadChunked` sees only silence and emits empty chunks. This would explain: correct rebuild + correct device switch + genuine speech + "Nothing transcribed."

This reframes the map's destination: the bug is likely **not** in device-switch detection (which already works) but in a **fixed VAD sensitivity threshold that doesn't adapt to per-device gain**. See new ticket for confirming this before the detection-strategy decision (03) is made — 03 now also depends on it.
