# History export (JSON / CSV)

## What to build

Add an Export button to the History page that writes the user's dictation history to a file they choose via a native save dialog. Support two formats: JSON (array of history records) and CSV (flat rows). The export should respect the current search filter — if the user has filtered history, only the filtered results are exported. A new Tauri command (`export_history`) accepts a file path and format, queries the SQLite store, and writes the file.

## Acceptance criteria

- [ ] History page has an Export button (or menu) offering JSON and CSV format options
- [ ] Clicking export opens a native save dialog with a sensible default filename (e.g. `flow-local-history-2026-06-15.json`)
- [ ] The exported file contains all fields visible in the History UI (timestamp, app name, raw text, final text, word count, STT ms, LLM ms)
- [ ] If a search filter is active, only matching records are exported
- [ ] Export with an empty history produces a valid empty file (empty array / header-only CSV), not an error

## Blocked by

None — can start immediately.
