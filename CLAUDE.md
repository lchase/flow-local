# Flow Local — agent guide

macOS-only local dictation app (Wispr Flow clone): hold a global hotkey → on-device STT → optional Ollama LLM cleanup → paste into the focused app. Tauri v2; all latency-critical work in Rust, UI in React + TS + Tailwind v4.

## Commands

```sh
pnpm tauri dev                  # run app (auto-rebuilds Rust + HMR frontend)
pnpm exec tsc --noEmit          # frontend typecheck
pnpm build                      # frontend build (tsc + vite)
cd src-tauri && cargo check     # Rust check
cd src-tauri && cargo test --test stt_e2e -- --ignored --nocapture
                                # STT e2e vs real model; first generate audio:
                                # say -o /tmp/stt_test.wav --data-format=LEI16@16000 "..."
```

Build prereqs beyond Rust/Node: `brew install cmake` (whisper.cpp), rustfmt component.

## Architecture (src-tauri/src/)

Pipeline flow: `hotkey (tauri-plugin-global-shortcut, Pressed/Released) → pipeline.rs state machine (Idle/Recording/Processing) → audio.rs (cpal worker thread → 16kHz mono f32) → stt.rs Transcriber (Parakeet or Whisper via transcribe-rs) → cleanup.rs (Ollama /api/chat, best-effort: falls back to raw) → inject.rs (clipboard save → enigo Cmd+V → restore) → store.rs (history + achievements)`.

- `pipeline.rs` — coordinator; captures frontmost app at keypress; emits `pipeline-state`, `dictation-complete`, `achievements-unlocked` events
- `stt.rs` — `MODELS` registry (Parakeet v3 int8 default, Whisper turbo/small); download (sha256-verified, from blob.handy.computer) + hot-swap via `set_stt_model`
- `cleanup.rs` — system prompt = base rules + dictionary + per-app mode fragment (`builtin_mode()` maps bundle ids → code/messaging/email/prose; user `app_profiles` override)
- `context.rs` — frontmost app via NSWorkspace (no permission needed)
- `settings.rs` — JSON at app_config_dir/settings.json; `store.rs` — SQLite at app_data_dir/flow-local.db
- Frontend: `src/App.tsx` sidebar shell + `src/pages/{Home,History,Dictionary,Settings}.tsx`; settings save instantly (no Save button); types mirrored in `src/types.ts`

## Conventions

- Keep the hot path (hotkey→paste) entirely in Rust; the webview only renders state from events.
- Settings struct changes need: settings.rs (+default), src/types.ts, and the Settings page.
- UI theme: warm-paper light theme, CSS vars in src/index.css (`--color-accent` emerald, Bricolage Grotesque display font, locally bundled via @fontsource). Don't introduce dark-gray "dashboard" styling.
- Tray icon must stay a monochrome template PNG (colorful icons render blank with `icon_as_template`).

## Gotchas (hard-won)

- **`time` crate is pinned to 0.3.47** in Cargo.lock — 0.3.48 breaks tauri-utils/cookie (E0119). Don't `cargo update -p time`.
- Ollama default model tag is `qwen3:4b-instruct` (`qwen3:4b-instruct-2507` does NOT exist in the registry).
- Parakeet applies inverse text normalization ("three" → "3") — tests must expect digits.
- The tray icon can sit at x=-1001 (off-screen) when the menu bar is full — that's why the app keeps a Dock icon and handles `RunEvent::Reopen`.
- cpal streams are !Send: `audio.rs` owns the stream in a dedicated worker thread; worker must send `Started` so `start()` doesn't block (was a 1.5s latency bug).
- macOS permissions: Microphone (auto-prompt) + Accessibility (for enigo paste). TCC grants are per-signature; dev builds inherit the terminal's grants.
- Window count via System Events reads 0 for Accessory-policy apps even when windows exist.
