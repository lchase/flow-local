# App picker for per-app profiles

## What to build

Replace the raw bundle ID text input in Settings → per-app overrides with a picker that lists running applications sourced from NSWorkspace. The user selects an app by name; the bundle ID is resolved and stored automatically. Typing a bundle ID manually remains possible as a fallback for apps that aren't currently running.

A new Tauri command (`list_running_apps`) returns a list of `{ name: string, bundle_id: string, icon?: string }` for all running non-background apps. The frontend shows this as a searchable dropdown (combobox) when the user adds a new per-app override.

## Acceptance criteria

- [ ] `list_running_apps` command returns name + bundle_id for all foreground-visible running apps
- [ ] Settings → add per-app override shows a searchable combobox populated from that list
- [ ] Selecting an app from the list auto-fills the bundle ID; the name is shown as a label
- [ ] User can still type a bundle ID directly if the target app isn't running
- [ ] Existing saved per-app overrides (stored as bundle IDs) continue to display and function correctly

## Blocked by

None — can start immediately.
