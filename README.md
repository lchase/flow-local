# Flow Local

Local-first dictation for macOS, in the spirit of [Wispr Flow](https://wisprflow.ai) but fully on-device: hold a hotkey anywhere, speak, release — clean text appears at your cursor. Speech-to-text runs on-device (NVIDIA Parakeet via ONNX), and the transcript is polished by a local LLM through [Ollama](https://ollama.com) (filler removal, punctuation, self-corrections, personal dictionary, app-aware formatting).

## Stack

- **Shell**: Tauri v2 — Rust core (audio → STT → LLM cleanup → paste injection), React + TypeScript + Tailwind UI
- **STT**: Parakeet TDT 0.6B v3 (int8 ONNX) via [`transcribe-rs`](https://crates.io/crates/transcribe-rs); auto-downloaded on first run (~456 MB)
- **Cleanup**: Ollama `/api/chat`, default model `qwen3:4b-instruct-2507`, configurable
- **Injection**: clipboard save → paste (Cmd+V via `enigo`) → clipboard restore

## Prerequisites

- macOS 12+, Apple Silicon recommended
- [Rust](https://rustup.rs), Node 20+, pnpm
- [Ollama](https://ollama.com) running locally with a small instruct model pulled:
  ```sh
  ollama pull qwen3:4b-instruct-2507
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

## Build

```sh
pnpm tauri build
```

## Permissions used

| Permission | Why |
|---|---|
| Microphone | capture dictation audio (processed on-device) |
| Accessibility | synthesize Cmd+V to insert text into the focused app |

No audio or text ever leaves the machine.
