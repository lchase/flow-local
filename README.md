# Flow Local

Local-first dictation for macOS, in the spirit of [Wispr Flow](https://wisprflow.ai) but fully on-device: hold a hotkey anywhere, speak, release — clean text appears at your cursor. Speech-to-text runs on-device (NVIDIA Parakeet via ONNX), and the transcript is polished by a local LLM through [Ollama](https://ollama.com) (filler removal, punctuation, self-corrections, personal dictionary, app-aware formatting).

## Stack

- **Shell**: Tauri v2 — Rust core (audio → STT → LLM cleanup → paste injection), React + TypeScript + Tailwind UI
- **STT**: Parakeet TDT 0.6B v3 (int8 ONNX) via [`transcribe-rs`](https://crates.io/crates/transcribe-rs); auto-downloaded on first run (~456 MB)
- **Cleanup**: Ollama `/api/chat`, default model `qwen3:4b-instruct`, configurable
- **Injection**: clipboard save → paste (Cmd+V via `enigo`) → clipboard restore

## Prerequisites

- macOS 12+, Apple Silicon recommended
- [Rust](https://rustup.rs), Node 20+, pnpm
- [Ollama](https://ollama.com) running locally with a small instruct model pulled:
  ```sh
  ollama pull qwen3:4b-instruct
  ```

## Develop

```sh
pnpm install
pnpm tauri dev
```

On first run:

1. Grant **Microphone** access when prompted (first recording).
2. Grant **Accessibility** access from the settings window (needed to insert text into other apps).
3. Wait for the Parakeet model download to finish (progress shown in the settings window).

Then hold **⌥ Space** in any app, speak, and release.

## Build a distributable app

One-time build prerequisites: `brew install cmake` (whisper.cpp) and `rustup component add rustfmt`.

```sh
pnpm tauri build
```

Artifacts land in `src-tauri/target/release/bundle/`:

- `macos/Flow Local.app` — drag into `/Applications`
- `dmg/Flow Local_0.1.0_aarch64.dmg` — shareable disk image

The first launch on any machine downloads the Parakeet model (~456 MB) and prompts for Microphone + Accessibility. **Ollama is a separate install** on each machine (`brew install ollama`, then `ollama pull qwen3:4b-instruct`) — without it the app still works, inserting raw transcripts.

### Sharing inside the company (demo-stage)

The build is currently **unsigned/un-notarized**, so Gatekeeper will block it on other Macs with "can't be opened". Recipients can bypass it one of two ways:

- Right-click the app → **Open** → Open (once), or
- `xattr -dr com.apple.quarantine "/Applications/Flow Local.app"`

That's fine for demos to a few colleagues.

**Homebrew tap (recommended for company-internal distribution).** A private tap sidesteps the Gatekeeper dance — `--no-quarantine` installs skip the quarantine attribute, so even unsigned builds open normally:

1. Create a repo named `homebrew-tools` under the org and copy `packaging/homebrew/flow-local.rb` into `Casks/`
2. Host the dmg somewhere URL-accessible (GitHub Releases on the app repo is easiest), fill in `url` and `sha256` (`shasum -a 256 <dmg>`)
3. Colleagues run:
   ```sh
   brew tap <org>/tools
   brew install --cask --no-quarantine flow-local
   ```
4. Releases become: build → upload dmg → bump `version`/`sha256` in the cask → users `brew upgrade --cask flow-local`

This requires the artifact to be hosted somewhere, so it kicks in once you put the project in a repo. Until then, AirDrop/Slack the dmg with the right-click → Open instruction.

For fully polished distribution (no `--no-quarantine` needed, works for non-brew users too):

1. An **Apple Developer ID** certificate (Apple Developer Program, $99/yr — possibly via the company's Apple Business account)
2. Set `bundle.macOS.signingIdentity` in `src-tauri/tauri.conf.json` and configure notarization (`APPLE_ID`/`APPLE_PASSWORD` or App Store Connect API key env vars — `tauri build` handles submission)
3. Optionally the tauri-plugin-updater for auto-updates from an internal URL

Apple Silicon only as configured; Intel Macs would need a `--target universal-apple-darwin` build.

## Demo script (2 minutes)

1. Open Slack or TextEdit, hold **⌥Space**: "um so basically let's meet at 2 actually 3pm thanks so much for um setting this up" → release. Filler words gone, correction applied.
2. Open VS Code, hold **⌥Space**: "set the user settings object to null" → inserts `userSettings = null` (code mode kicked in from the frontmost app).
3. Show the **History** tab (raw vs. polished diff), **Home** stats/achievements, and the point that sells it: **everything ran on this Mac — no audio or text left the machine.**

## Permissions used

| Permission | Why |
|---|---|
| Microphone | capture dictation audio (processed on-device) |
| Accessibility | synthesize Cmd+V to insert text into the focused app |

No audio or text ever leaves the machine.
