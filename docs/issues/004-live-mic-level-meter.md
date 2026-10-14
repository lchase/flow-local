# Live mic level meter during recording

## What to build

While the pipeline is in the `Recording` state, show a small animated level indicator in the sidebar/overlay so the user has confirmation that the mic is capturing audio. The audio worker already processes f32 samples; it should compute a peak (or short-window RMS) level and emit a lightweight `mic-level` Tauri event (value 0.0–1.0) at ~15 Hz. The frontend renders this as a compact bar or waveform indicator that disappears when recording stops.

## Acceptance criteria

- [ ] A `mic-level` event is emitted from the audio worker at ~15 Hz during recording, carrying a normalized float (0.0–1.0)
- [ ] The sidebar/overlay displays a visible level indicator while `pipelineState === "recording"`
- [ ] The indicator animates in response to actual audio volume (louder speech → higher level)
- [ ] The indicator is absent (not rendered or zeroed) in all non-recording states
- [ ] Event emission adds no perceptible latency to the recording → STT handoff

## Blocked by

None — can start immediately.
