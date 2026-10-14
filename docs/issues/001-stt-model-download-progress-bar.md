# STT model download progress bar

## What to build

When the user triggers a model download (from Settings or on first run), show a visual progress bar in the UI instead of the current plain-text "Downloading model 42 MB / 512 MB" string. The bar should reflect the existing `DownloadProgress` event stages: `downloading`, `verifying`, `extracting`, `loading`. Once the model is loaded, the bar disappears and the status dot turns green.

## Acceptance criteria

- [ ] A progress bar is visible in the Home status section while any download stage is active
- [ ] The bar advances smoothly as `downloaded_bytes` increases relative to `total_bytes`
- [ ] Stage labels ("Verifying…", "Extracting…", "Loading…") display below the bar for non-download stages
- [ ] The bar disappears and the status dot turns green when `sttStatus.loaded` becomes true
- [ ] No visual regression on the happy path (model already loaded)

## Blocked by

None — can start immediately.
