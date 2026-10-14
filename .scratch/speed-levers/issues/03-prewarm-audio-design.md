# Design: prewarm audio capture architecture

Type: grilling
Status: closed
Blocked by: 01
Assignee: agent (claude)

## Question

`audio.rs::start()` currently spawns a new worker thread and builds a fresh `cpal::Stream` (host/device enumeration, stream config, stream construction) on every single recording — real cold-start cost each hotkey press. FluidVoice keeps its Core Audio capture always-on and gates recording with an atomic flag (`DirectCoreAudioInput` + `prewarmAudioEngineIfPossible`).

Decide the architecture for Flow Local:

- Always-on stream, gated by an atomic recording flag (callback runs constantly, discards frames when idle) — vs. speculative pre-build ahead of an anticipated hotkey press, vs. other approaches.
- Mic-indicator / privacy-light UX implications of a permanently-open input stream (macOS shows a mic-in-use indicator whenever a stream is active, even if not recording).
- Battery/CPU cost of a permanently-running audio callback.
- Interaction with existing device hot-swap/route-change handling in `pipeline.rs`.
- Trigger point for prewarming: app startup? after mic permission is granted? on Settings window close?

Resolve to a concrete design ready to implement (a follow-up task ticket will build it).

## Answer

**Always-on stream, atomic-gated, built at app startup.** Decided with the user across four questions (approach, trigger, mic-indicator tradeoff, device-change handling):

- **Approach**: `Recorder`'s worker thread + `cpal::Stream` are built once and live for the app's lifetime, not rebuilt per recording. A new `recording: Arc<AtomicBool>` gate is added to the existing stream callback in `build_stream` (`audio.rs`): when false, the callback returns immediately (skip downmix + RMS entirely) instead of pushing samples — idle CPU cost is a no-op branch check per audio buffer, not the full processing path.
- **Trigger**: prewarm at app startup, right after `app.manage(Recorder::new())` in `lib.rs`'s setup. If mic permission isn't granted yet, the initial `build_input_stream`/`stream.play()` attempt errors; log it and don't surface to the UI — `start()` falls back to building on-demand that one time (today's behavior), so first dictation right after a permission grant still works, just without the prewarm win for that single call.
- **`start()`**: no longer builds a stream unconditionally. It cheaply compares the current `host.default_input_device()` name against the device the open stream was built for. Same device (the common case): just clear the sample buffer and flip `recording = true` — near-zero cost. Different device (user plugged in/switched input mid-session) or no stream yet: tear down and rebuild against the new default, paying today's ~170ms cost only in that edge case.
- **`stop()`**: flip `recording = false`, take + resample the buffer as today. Stream and worker thread are never torn down.
- **Mic indicator**: accepted — the always-lit mic-in-use dot for the app's whole runtime is the traded-off cost of the latency win. No mitigation built; worth a one-line mention in Settings/about copy later, not blocking this ticket.
- **Device-change handling**: this design *introduces* a gap that doesn't exist today (per-recording rebuild always tracked the current default device; an always-on stream built once would go stale). Resolved by the default-device check inside `start()` above — no live CoreAudio device-change listener, just a cheap comparison paid once per recording attempt.

This closes the design question; implementation is split out to [Implement prewarm audio capture](07-prewarm-audio-implement.md).
