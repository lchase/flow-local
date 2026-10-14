# Ollama URL health-check on save

## What to build

When the user edits the Ollama URL field in Settings, validate connectivity immediately (debounced) rather than failing silently at dictation time. Show an inline indicator next to the field: a green check when the URL is reachable and the configured model is available, an amber warning when reachable but the model is missing, or a red error when the host is unreachable. The existing `check_ollama` Tauri command (already called on app load) should be reused or called on-demand.

## Acceptance criteria

- [ ] Editing the Ollama URL field triggers a debounced health check (≤500 ms debounce)
- [ ] Inline status indicator shows: reachable + model available → green; reachable + model missing → amber with "Pull `<model>` in Ollama"; unreachable → red with error hint
- [ ] Indicator clears/resets while the check is in flight (spinner or neutral state)
- [ ] No duplicate network requests when the user types quickly
- [ ] Existing behavior on app load (status dot on Home) is unchanged

## Blocked by

None — can start immediately.
