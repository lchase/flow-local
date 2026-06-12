use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Result};
use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Enigo is created lazily: constructing it on macOS requires the
/// Accessibility grant, which the user may not have given at launch.
pub struct Injector {
    enigo: Mutex<Option<Enigo>>,
}

impl Injector {
    pub fn new() -> Self {
        Self {
            enigo: Mutex::new(None),
        }
    }

    /// Insert text at the cursor of the frontmost app: save clipboard →
    /// write text → synthetic Cmd+V → restore clipboard. Paste (not direct
    /// typing) because CGEventKeyboardSetUnicodeString truncates long text.
    pub fn paste(&self, app: &AppHandle, text: &str) -> Result<()> {
        let mut guard = self.enigo.lock().unwrap();
        if guard.is_none() {
            *guard = Some(
                Enigo::new(&Settings::default())
                    .map_err(|e| anyhow!("input system init failed (Accessibility permission?): {e}"))?,
            );
        }
        let enigo = guard.as_mut().unwrap();

        let clipboard = app.clipboard();
        let saved = clipboard.read_text().ok();

        clipboard
            .write_text(text.to_string())
            .map_err(|e| anyhow!("clipboard write failed: {e}"))?;
        // Give the pasteboard a beat to settle before the synthetic keystroke.
        std::thread::sleep(Duration::from_millis(60));

        send_cmd_v(enigo)?;
        std::thread::sleep(Duration::from_millis(120));

        if let Some(saved) = saved {
            let _ = clipboard.write_text(saved);
        }
        Ok(())
    }
}

fn send_cmd_v(enigo: &mut Enigo) -> Result<()> {
    // Key::Other(9) is the macOS virtual keycode for 'V' (layout-independent
    // for paste purposes); same approach Handy uses.
    enigo
        .key(Key::Meta, Direction::Press)
        .map_err(|e| anyhow!("key press failed: {e}"))?;
    enigo
        .key(Key::Other(9), Direction::Click)
        .map_err(|e| anyhow!("key click failed: {e}"))?;
    std::thread::sleep(Duration::from_millis(50));
    enigo
        .key(Key::Meta, Direction::Release)
        .map_err(|e| anyhow!("key release failed: {e}"))?;
    Ok(())
}
