# Research: does cpal/CoreAudio support push-based default-input-device-change notification?

Type: research
Status: closed
Blocked by: (none)
Assignee: lawrence.chase

## Question

Today, `ensure_stream` in `src-tauri/src/audio.rs` only checks `host.default_input_device()` when a hotkey press or prewarm triggers `Cmd::Start`/`Cmd::Prewarm` — there's no continuous monitoring between presses. Does cpal expose a callback/event for default input device changes, or does that require dropping to CoreAudio directly (e.g. a `kAudioHardwarePropertyDefaultInputDevice` listener via `coreaudio-rs` or raw objc)? What's the cost/complexity difference vs. the current poll-on-hotkey approach — worth it, or is polling-on-demand already sufficient once the name-comparison bug (if any) is fixed?

Produce a short markdown summary as a linked asset.

## Resolution

Full findings: [assets/02-cpal-device-change-notification-research.md](assets/02-cpal-device-change-notification-research.md).

cpal 0.16.0 (pinned version) exposes no device-change/hotplug callback anywhere in its public `HostTrait`/`DeviceTrait` surface — confirmed against docs.rs and open cpal issues (#559, #224) showing this is a requested-not-shipped feature. CoreAudio itself does support push notification for `kAudioHardwarePropertyDefaultInputDevice` via `AudioObjectAddPropertyListener` (per Apple's `AudioHardware.h`), and `objc2-core-audio` (already pulled in transitively via cpal/coreaudio-rs) exposes the raw binding — cpal's own internal `property_listener.rs` shows the integration shape (RAII register/remove around an `unsafe extern "C"` callback on a CoreAudio HAL notification thread, requiring a cross-thread handoff into the `!Send` stream/worker design).

**Recommendation: don't add push-based monitoring.** The existing poll-on-hotkey-press design in `ensure_stream` already re-reads `default_input_device()` fresh on every `Start`, so it's correct-at-the-moment-of-recording by construction — the only gap a listener would close is a default-device swap *while already recording* (old device stays connected, e.g. mid-session switch), which self-corrects on the very next press and costs at most one utterance on the wrong mic. Not worth a new direct FFI dependency + unsafe callback lifecycle for that.

Genuinely worthwhile, much cheaper follow-up surfaced by this research: cpal's own disconnect listener already delivers `StreamError::DeviceNotAvailable` to the error callback this repo wires up (`audio.rs:325`) when the in-use device is actually unplugged mid-recording — today that's just `log::error!`'d, not surfaced to the pipeline. Split out to a new ticket since it's a distinct, non-blocking improvement (real unplug-mid-recording handling, not push-notification of default-device changes).
