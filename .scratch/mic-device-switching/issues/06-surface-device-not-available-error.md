# Decide how to surface mid-recording device-unplug errors

Type: grilling
Status: closed
Blocked by: (none)
Assignee: lawrence.chase

## Question

[Research: does cpal/CoreAudio device-change notification](02-research-cpal-device-change-notification.md) found that cpal already delivers `StreamError::DeviceNotAvailable` to the error callback wired up in `src-tauri/src/audio.rs:325` when the in-use input device is actually unplugged mid-recording — today that's just `log::error!`'d and never reaches the pipeline or the user. Should this be surfaced (e.g. an event to the frontend, a pipeline-state transition, a retry-on-next-press), and if so what should the user see/experience when their mic disappears mid-dictation?

## Resolution

**Abort the in-flight recording immediately** on `StreamError::DeviceNotAvailable`, rather than letting the user keep talking into a dead stream until they release the key (today's behavior — `recording` stays `true`, the stream just goes silent, and they get a stale/empty result afterward).

Reuse the existing generic error-surfacing path rather than inventing a new one: `emit_state(app, "error", Some(msg))` (`pipeline.rs:304`), already rendered by `Overlay.tsx`'s `isError` state and already used for other mic failures (e.g. "Mic error: {e}"), auto-clearing after 3s (`pipeline.rs:307-310`). Partial audio captured before the disconnect is discarded, consistent with how other error states don't attempt a partial transcription.

Confirmed no dependency on next-press recovery: [Research: does cpal/CoreAudio device-change notification](02-research-cpal-device-change-notification.md) already established `ensure_stream` rebuilds correctly against whatever `default_input_device()` returns on the very next `Start` — that path needs no new work. This ticket is only about the current, already-dead recording.

Left as implementation shape (not a further decision, per the map's Notes that this is a decision spec only): today's error callback (`audio.rs:325`) is untyped and has no path back to the pipeline — closing this out needs the callback to distinguish `StreamError::DeviceNotAvailable` from other stream errors and signal it through the existing worker `mpsc` channel (`WorkerMsg`) so the already-running ~200ms streaming poll task in `pipeline.rs::start_streaming` can notice it and trigger the abort.

**Implementation note (deviated from the shape above):** used a dedicated `Arc<Mutex<Option<String>>>` (`Recorder::stream_error`, mirroring the existing `level` field) instead of the `WorkerMsg` channel. That channel is a synchronous request/response protocol — `start()`/`stop()` block on `recv_timeout` expecting a specific reply — and pushing an async error onto it risked the poll task's `try_recv()` racing a concurrent `stop()`'s blocking receive and stealing its `Done` message. The decision itself (abort immediately, reuse `emit_state(app, "error", ...)`, discard partial audio) is implemented as specified.
