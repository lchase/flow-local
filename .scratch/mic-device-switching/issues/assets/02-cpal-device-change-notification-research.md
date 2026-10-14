# Research: does anything push-notify us of a default input device change?

Repo pin: `cpal = "0.16"` in `src-tauri/Cargo.toml:27`, locked to **cpal 0.16.0**
(`src-tauri/Cargo.lock:799`). Transitively pulls **coreaudio-rs 0.13.0**
(`Cargo.lock:785`) and **objc2-core-audio** (version resolved in
`Cargo.lock:3043-3053`, pulled in by both `cpal` and `coreaudio-rs`). The repo
does not depend on `coreaudio-rs` or `objc2-core-audio` directly — they only
appear as `cpal`'s own dependencies.

## 1. Does cpal expose a "default input device changed" callback API?

No. Checked docs.rs for `cpal` 0.16.0:

- `https://docs.rs/cpal/0.16.0/cpal/traits/index.html` — the `traits` module
  defines exactly three traits: `DeviceTrait`, `HostTrait`, `StreamTrait`.
- `https://docs.rs/cpal/0.16.0/cpal/traits/trait.HostTrait.html` —
  `HostTrait`'s full method set is `is_available()`, `devices()`,
  `default_input_device()`, `default_output_device()`, plus the provided
  `input_devices()`/`output_devices()` filters. All of these are one-shot
  queries; none takes a callback or returns a subscription/handle.
- `https://docs.rs/cpal/0.16.0/cpal/traits/trait.DeviceTrait.html` —
  `DeviceTrait`'s methods are `name()`, `supported_input_configs()`,
  `supported_output_configs()`, `default_input_config()`,
  `default_output_config()`, `build_input_stream_raw()`/`build_output_stream_raw()`
  (+ typed wrappers), and `supports_input()`/`supports_output()`. No
  device-change/hotplug hook.
- `StreamTrait` only has `play()`/`pause()`.

So there is no `*_devices_changed`-style hook anywhere in cpal's public API
surface for any platform, macOS included.

That said, cpal's macOS backend *does* register one CoreAudio property
listener internally, but it is not what the question is asking about:

- `https://github.com/RustAudio/cpal/blob/v0.16.0/src/host/coreaudio/macos/mod.rs`
  (`add_disconnect_listener`, ~line 455-476) listens for
  `kAudioDevicePropertyDeviceIsAlive` **on the specific device ID the stream
  was already built for**, and fires the stream's user-supplied error
  callback with `StreamError::DeviceNotAvailable` if that device goes away
  (e.g. unplugged). It does **not** listen on `kAudioHardwarePropertyDefaultInputDevice`
  on the system object, so it fires only on disconnect of the in-use device,
  never on "the OS default input pointer moved to a different, still-present
  device" (e.g. user picks a new mic in System Settings while the old one
  stays plugged in).
- The generic listener plumbing cpal uses for this
  (`src/host/coreaudio/macos/property_listener.rs`, see §2 below) is a
  private module (`mod property_listener;`), not re-exported — an app using
  cpal cannot hook additional selectors through it.
- Confirmed via GitHub search that hotplug/default-device-change support for
  cpal has been a *requested*, not shipped, feature: `RustAudio/cpal#559`
  ("neither PortAudio nor RtAudio support hotplug, but cpal could" — linking
  to issue #373), and `RustAudio/cpal#224` which independently confirms the
  macOS backend uses `AudioObjectAddPropertyListener`-family APIs (unavailable
  on iOS, which is why iOS needed a separate code path).

**Bottom line for Q1: no push API to subscribe to in cpal itself.** The only
thing an app automatically gets "for free" is the existing disconnect
callback for the device already in use (which this repo's `build_stream`
already wires up via its `error_callback` argument, `src-tauri/src/audio.rs:325`).

## 2. Does CoreAudio itself support this, and what would wiring it up look like?

Yes, at the OS level. `kAudioHardwarePropertyDefaultInputDevice` is a
standard, listenable `AudioObjectPropertySelector` on the system audio
object.

- Apple's Core Audio header (`AudioHardware.h`, confirmed via the mirrored
  SDK header
  `phracker/MacOSX-SDKs/.../CoreAudio.framework/Headers/AudioHardware.h`,
  lines ~919-924): `kAudioHardwarePropertyDefaultInputDevice` is documented
  as "The AudioDeviceID of the default input AudioDevice," alongside
  `kAudioHardwarePropertyDefaultOutputDevice` and
  `kAudioHardwarePropertyDefaultSystemOutputDevice`. Apple's current
  Developer Documentation page mirrors this
  (`developer.apple.com/documentation/coreaudio/kaudiohardwarepropertydefaultinputdevice`,
  role "Global Variable", type `AudioObjectPropertySelector`).
- The same header (lines ~519-538) documents the listener API:
  ```c
  typedef OSStatus
  (*AudioObjectPropertyListenerProc)( AudioObjectID inObjectID,
                                       UInt32 inNumberAddresses,
                                       const AudioObjectPropertyAddress inAddresses[],
                                       void *inClientData);

  extern OSStatus
  AudioObjectAddPropertyListener( AudioObjectID inObjectID,
                                  const AudioObjectPropertyAddress *inAddress,
                                  AudioObjectPropertyListenerProc inListener,
                                  void *inClientData);
  ```
  You'd call this once against `kAudioObjectSystemObject`, with an
  `AudioObjectPropertyAddress{ mSelector: kAudioHardwarePropertyDefaultInputDevice,
  mScope: kAudioObjectPropertyScopeGlobal, mElement: kAudioObjectPropertyElementMaster }`.
  The callback fires on an internal CoreAudio HAL notification thread — not
  guaranteed to be your app's main thread or the thread that registered it —
  so any Rust integration has to treat it like any other cross-thread signal
  (channel/flag), the same way cpal's own error callback already is.

### Do coreaudio-rs / objc2-core-audio expose this?

- **coreaudio-rs 0.13.0** (docs.rs `coreaudio-rs/0.13.0`): its public modules
  are `audio_unit`, `audio_unit::audio_format`, `audio_unit::macos_helpers`,
  `audio_unit::render_callback`, `audio_unit::sample_format`,
  `audio_unit::stream_format`, plus `error`/`types`. The only property
  functions are AudioUnit-level `get_property`/`set_property` (no listener
  registration). `macos_helpers` does expose two purpose-built listener
  structs — `AliveListener` (device-disconnect) and `RateListener`
  (sample-rate-change) — but nothing for "default device changed." So
  coreaudio-rs's high-level surface doesn't cover this property either.
- **objc2-core-audio** (the crate actually pinned in this repo's lock file,
  version resolved at `Cargo.lock:3043`) does expose the raw CoreAudio HAL
  bindings directly: `AudioObjectAddPropertyListener`,
  `AudioObjectAddPropertyListenerBlock`, `AudioObjectRemovePropertyListener`,
  `AudioObjectRemovePropertyListenerBlock` (confirmed via its docs.rs
  functions list), alongside the older per-object variants
  (`AudioHardwareAddPropertyListener`, `AudioDeviceAddPropertyListener`, etc.,
  all long-deprecated in favor of the `AudioObject*` form).
- Concretely, cpal itself is the best "how would you wire it up" reference,
  because it already does this for a different selector using exactly this
  crate: `src-tauri`'s locked cpal 0.16.0 source,
  `src/host/coreaudio/macos/property_listener.rs`
  (`https://github.com/RustAudio/cpal/blob/v0.16.0/src/host/coreaudio/macos/property_listener.rs`),
  wraps `objc2_core_audio::AudioObjectAddPropertyListener` behind a small
  `AudioObjectPropertyListener` RAII type: register with an
  `unsafe extern "C-unwind" fn` shim matching `AudioObjectPropertyListenerProc`,
  stash a boxed `FnMut()` closure as the `void*` client-data pointer, and
  `AudioObjectRemovePropertyListener` it on `Drop`.

Shape of a hand-rolled integration for `kAudioHardwarePropertyDefaultInputDevice`
specifically: add a direct `objc2-core-audio` dependency (cpal doesn't
re-export its internal listener module), call
`AudioObjectAddPropertyListener(kAudioObjectSystemObject, &address, callback, ctx)`
once at startup with `mSelector: kAudioHardwarePropertyDefaultInputDevice`,
have the `unsafe extern "C"` callback just set an `AtomicBool`/send on an
`mpsc` channel (never touch cpal's `!Send` stream from that thread — the
callback fires on CoreAudio's own notification thread), and have `audio.rs`'s
worker thread poll/receive that signal to decide whether to rebuild the
stream early. This is a nontrivial addition: new direct FFI dependency,
`unsafe` callback registration/teardown lifecycle, and a cross-thread handoff
into the existing worker-thread/`mpsc` design.

## 3. Is it worth it, given how this repo actually uses the device?

Read `src-tauri/src/audio.rs:171-288` and `src-tauri/src/pipeline.rs:143-189`.

Current behavior: `ensure_stream` (`audio.rs:218`) is only invoked from the
worker loop's `Cmd::Prewarm` and `Cmd::Start` branches (`audio.rs:182-201`) —
i.e. app launch (prewarm) and every hotkey-press-to-start
(`pipeline.rs:163-189` → `Recorder::start()`). It calls
`host.default_input_device()` fresh each time, compares the returned device's
`name()` against the name the currently-open stream was built for, and only
tears down/rebuilds on a mismatch (`audio.rs:230-235`). There is **no** call
to `ensure_stream`, and no device re-check at all, on `Cmd::Stop`
(`audio.rs:202-210`) or anywhere during an active recording — the stream
that was already `play()`-ing just keeps running until `stop()` takes the
buffer.

Consequences for a device swap that happens **while the hotkey is held down
and a recording is in progress**:

- If the user changes the *default* input device in System Settings (old
  device stays connected, e.g. switching from built-in mic to an AirPods
  input) mid-recording: cpal's stream keeps capturing from the *original*
  device — this repo never re-queries `default_input_device()` until the
  *next* `Start`. The disconnect listener cpal registers internally
  (`kAudioDevicePropertyDeviceIsAlive` on the specific device) won't fire
  either, because that device hasn't gone away, it's just no longer default.
  So today, an in-recording device swap is silently ignored until the
  *following* dictation.
- If the in-use device is actually unplugged mid-recording: cpal's own
  internal disconnect listener does fire, delivering
  `StreamError::DeviceNotAvailable` to the error callback this repo already
  passes to `build_input_stream` (`audio.rs:325`, currently just
  `log::error!`). That path already exists and needs no new dependency — it's
  just not surfaced to the user/pipeline today (worth a small follow-up on
  its own, unrelated to hotplug notifications).
- Either way, the *next* hotkey press always sees the truth: `ensure_stream`
  re-reads `default_input_device()` and rebuilds if the name changed, so a
  stale device selection can survive at most one dictation.

Given the app's actual requirement — "know the current default input device
at the moment the hotkey is pressed," not continuously — the existing
poll-on-hotkey-press design already satisfies the requirement by
construction: every `Start` is a fresh read of ground truth. Adding
CoreAudio's push notification would only buy correctness for the narrow
mid-recording-default-device-swap case above, and recording sessions here
are short push-to-talk utterances (the whole point of the feature), so the
exposure window is small and the cost of missing it is "this one utterance
used the previous device," not silence or a crash.

## Recommendation

Don't add push-based CoreAudio monitoring. The poll-on-hotkey-press approach
in `ensure_stream` already gives correct-at-the-moment-of-recording behavior,
which is all the product needs; a `kAudioHardwarePropertyDefaultInputDevice`
listener would only close a narrow, low-stakes mid-recording-swap gap at the
cost of a new direct `objc2-core-audio` dependency, `unsafe extern "C"`
callback FFI, listener lifecycle management (register at startup, remove on
shutdown), and a new cross-thread signal path to bridge into cpal's `!Send`
stream/worker-thread design — complexity disproportionate to "one dictation
occasionally uses the previous mic." The one genuinely worthwhile, much
cheaper follow-up already sitting in the code: `build_stream`'s error
callback (`audio.rs:325`) already receives cpal's disconnect notification
(`StreamError::DeviceNotAvailable`) for real unplug-mid-recording events but
just logs it — wiring that into the pipeline's error/finish path would be a
same-file, no-new-dependency change, and would cover the actual failure mode
(device disappears) rather than the cosmetic one (default pointer moves to a
still-present device).
