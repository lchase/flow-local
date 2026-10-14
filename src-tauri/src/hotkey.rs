//! Raw global key listener backing the "hotkey" setting.
//!
//! Replaces `tauri-plugin-global-shortcut` (built on `global-hotkey`, which can only
//! register modifier+key combos — a bare modifier like "just the Option key" can't be
//! expressed as an OS-level hotkey) and an earlier `rdev`-based attempt (`rdev::listen`
//! resolves each keystroke's printed character via Text Input Source Manager, which
//! macOS hard-asserts must run on the main thread; since our listener runs on its own
//! thread, any real keypress crashed the whole app with `dispatch_assert_queue_fail` —
//! confirmed via crash report). We don't need the printed character, only the physical
//! keycode and modifier flags, so a direct `CGEventTap` (via `core-graphics`) sidesteps
//! TSM entirely.
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use core_foundation::runloop::CFRunLoop;
use core_graphics::event::{
    CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, CallbackResult, EventField, KeyCode,
};
use tauri::{AppHandle, Manager};

use crate::pipeline::Pipeline;

/// macOS virtual keycode (hardware position, independent of keyboard layout).
type Key = u16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModKey {
    Alt,
    Ctrl,
    Shift,
    Cmd,
}

impl ModKey {
    fn flag(self) -> CGEventFlags {
        match self {
            ModKey::Alt => CGEventFlags::CGEventFlagAlternate,
            ModKey::Ctrl => CGEventFlags::CGEventFlagControl,
            ModKey::Shift => CGEventFlags::CGEventFlagShift,
            ModKey::Cmd => CGEventFlags::CGEventFlagCommand,
        }
    }
}

const ALL_MOD_FLAGS: CGEventFlags = CGEventFlags::CGEventFlagAlternate
    .union(CGEventFlags::CGEventFlagControl)
    .union(CGEventFlags::CGEventFlagShift)
    .union(CGEventFlags::CGEventFlagCommand);

#[derive(Clone, Debug)]
pub struct HotkeyTarget {
    mods: Vec<ModKey>,
    /// `None` means a bare-modifier hotkey (push-to-talk on that single modifier alone).
    main: Option<Key>,
}

#[derive(Debug)]
pub struct HotkeyParseError(String);

impl std::fmt::Display for HotkeyParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for HotkeyParseError {}

pub fn parse_hotkey(binding: &str) -> Result<HotkeyTarget, HotkeyParseError> {
    let mut mods = Vec::new();
    let mut main = None;
    for tok in binding.split('+') {
        let tok = tok.trim();
        if tok.is_empty() {
            return Err(HotkeyParseError(format!("empty token in '{binding}'")));
        }
        match tok.to_lowercase().as_str() {
            "alt" | "option" => mods.push(ModKey::Alt),
            "ctrl" | "control" => mods.push(ModKey::Ctrl),
            "shift" => mods.push(ModKey::Shift),
            "cmd" | "command" | "super" => mods.push(ModKey::Cmd),
            _ if main.is_some() => {
                return Err(HotkeyParseError(format!("more than one main key in '{binding}'")));
            }
            _ => main = Some(parse_key(tok).ok_or_else(|| HotkeyParseError(format!("unsupported key '{tok}'")))?),
        }
    }
    if main.is_none() && mods.len() != 1 {
        return Err(HotkeyParseError(format!(
            "bare-modifier hotkey needs exactly one modifier, got '{binding}'"
        )));
    }
    Ok(HotkeyTarget { mods, main })
}

/// Main-key tokens mirror browser `KeyboardEvent.code` values, since the Settings UI's
/// key-capture field builds the stored string straight from `e.code`. Also accepts the
/// old single-letter/symbol shorthand ("d", "1") so existing stored settings keep working.
fn parse_key(code: &str) -> Option<Key> {
    Some(match code.to_uppercase().as_str() {
        "SPACE" => KeyCode::SPACE,
        "ENTER" | "RETURN" => KeyCode::RETURN,
        "TAB" => KeyCode::TAB,
        "ESCAPE" | "ESC" => KeyCode::ESCAPE,
        "BACKSPACE" => KeyCode::DELETE,
        "DELETE" => KeyCode::FORWARD_DELETE,
        "CAPSLOCK" => KeyCode::CAPS_LOCK,
        "ARROWUP" | "UP" => KeyCode::UP_ARROW,
        "ARROWDOWN" | "DOWN" => KeyCode::DOWN_ARROW,
        "ARROWLEFT" | "LEFT" => KeyCode::LEFT_ARROW,
        "ARROWRIGHT" | "RIGHT" => KeyCode::RIGHT_ARROW,
        "HOME" => KeyCode::HOME,
        "END" => KeyCode::END,
        "PAGEUP" => KeyCode::PAGE_UP,
        "PAGEDOWN" => KeyCode::PAGE_DOWN,
        "INSERT" => return None, // no equivalent physical key on Mac keyboards
        "DIGIT0" | "0" => KeyCode::ANSI_0,
        "DIGIT1" | "1" => KeyCode::ANSI_1,
        "DIGIT2" | "2" => KeyCode::ANSI_2,
        "DIGIT3" | "3" => KeyCode::ANSI_3,
        "DIGIT4" | "4" => KeyCode::ANSI_4,
        "DIGIT5" | "5" => KeyCode::ANSI_5,
        "DIGIT6" | "6" => KeyCode::ANSI_6,
        "DIGIT7" | "7" => KeyCode::ANSI_7,
        "DIGIT8" | "8" => KeyCode::ANSI_8,
        "DIGIT9" | "9" => KeyCode::ANSI_9,
        "KEYA" | "A" => KeyCode::ANSI_A,
        "KEYB" | "B" => KeyCode::ANSI_B,
        "KEYC" | "C" => KeyCode::ANSI_C,
        "KEYD" | "D" => KeyCode::ANSI_D,
        "KEYE" | "E" => KeyCode::ANSI_E,
        "KEYF" | "F" => KeyCode::ANSI_F,
        "KEYG" | "G" => KeyCode::ANSI_G,
        "KEYH" | "H" => KeyCode::ANSI_H,
        "KEYI" | "I" => KeyCode::ANSI_I,
        "KEYJ" | "J" => KeyCode::ANSI_J,
        "KEYK" | "K" => KeyCode::ANSI_K,
        "KEYL" | "L" => KeyCode::ANSI_L,
        "KEYM" | "M" => KeyCode::ANSI_M,
        "KEYN" | "N" => KeyCode::ANSI_N,
        "KEYO" | "O" => KeyCode::ANSI_O,
        "KEYP" | "P" => KeyCode::ANSI_P,
        "KEYQ" | "Q" => KeyCode::ANSI_Q,
        "KEYR" | "R" => KeyCode::ANSI_R,
        "KEYS" | "S" => KeyCode::ANSI_S,
        "KEYT" | "T" => KeyCode::ANSI_T,
        "KEYU" | "U" => KeyCode::ANSI_U,
        "KEYV" | "V" => KeyCode::ANSI_V,
        "KEYW" | "W" => KeyCode::ANSI_W,
        "KEYX" | "X" => KeyCode::ANSI_X,
        "KEYY" | "Y" => KeyCode::ANSI_Y,
        "KEYZ" | "Z" => KeyCode::ANSI_Z,
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "F11" => KeyCode::F11,
        "F12" => KeyCode::F12,
        "MINUS" | "-" => KeyCode::ANSI_MINUS,
        "EQUAL" | "=" => KeyCode::ANSI_EQUAL,
        "COMMA" | "," => KeyCode::ANSI_COMMA,
        "PERIOD" | "." => KeyCode::ANSI_PERIOD,
        "SLASH" | "/" => KeyCode::ANSI_SLASH,
        "SEMICOLON" | ";" => KeyCode::ANSI_SEMICOLON,
        "QUOTE" | "'" => KeyCode::ANSI_QUOTE,
        "BRACKETLEFT" | "[" => KeyCode::ANSI_LEFT_BRACKET,
        "BRACKETRIGHT" | "]" => KeyCode::ANSI_RIGHT_BRACKET,
        "BACKSLASH" | "\\" => KeyCode::ANSI_BACKSLASH,
        "BACKQUOTE" | "`" => KeyCode::ANSI_GRAVE,
        _ => return None,
    })
}

/// True while the configured target combo/bare-modifier is currently held.
fn is_active(target: &HotkeyTarget, held: &HashSet<Key>, flags: CGEventFlags) -> bool {
    let mods_down = target.mods.iter().all(|m| flags.contains(m.flag()));
    match target.main {
        Some(main_key) => mods_down && held.contains(&main_key),
        // ponytail: bare modifier only counts alone (no other modifier or regular key
        // down) so it doesn't fire mid-chord (e.g. Alt+Space shouldn't trigger a plain
        // "alt" hotkey). Upgrade to a more permissive check if that turns out to matter.
        None => held.is_empty() && mods_down && (flags & ALL_MOD_FLAGS) == target.mods[0].flag(),
    }
}

pub type SharedHotkeyTarget = Arc<Mutex<HotkeyTarget>>;

pub fn set_target(shared: &SharedHotkeyTarget, target: HotkeyTarget) {
    *shared.lock().unwrap() = target;
}

/// Spawns the dedicated listener thread (the event tap's run loop blocks) and returns
/// the shared target so callers can retarget it live via `set_target`.
pub fn spawn(app: AppHandle, initial: HotkeyTarget) -> SharedHotkeyTarget {
    let shared: SharedHotkeyTarget = Arc::new(Mutex::new(initial));
    let shared_for_thread = shared.clone();
    std::thread::spawn(move || {
        let held: RefCell<HashSet<Key>> = RefCell::new(HashSet::new());
        let active = Cell::new(false);
        let result = CGEventTap::with_enabled(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            vec![CGEventType::KeyDown, CGEventType::KeyUp, CGEventType::FlagsChanged],
            move |_proxy, event_type, event| {
                let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as Key;
                match event_type {
                    CGEventType::KeyDown => {
                        held.borrow_mut().insert(keycode);
                    }
                    CGEventType::KeyUp => {
                        held.borrow_mut().remove(&keycode);
                    }
                    _ => {}
                }
                let target = shared_for_thread.lock().unwrap().clone();
                let now_active = is_active(&target, &held.borrow(), event.get_flags());
                if now_active != active.get() {
                    active.set(now_active);
                    let pipeline = app.state::<Pipeline>();
                    pipeline.on_hotkey(&app, now_active);
                }
                CallbackResult::Keep
            },
            CFRunLoop::run_current,
        );
        if result.is_err() {
            log::error!("failed to install hotkey event tap (Accessibility permission missing?)");
        }
    });
    shared
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combo() {
        let t = parse_hotkey("alt+space").unwrap();
        assert_eq!(t.mods, vec![ModKey::Alt]);
        assert_eq!(t.main, Some(KeyCode::SPACE));
    }

    #[test]
    fn parses_bare_modifier() {
        let t = parse_hotkey("alt").unwrap();
        assert_eq!(t.mods, vec![ModKey::Alt]);
        assert_eq!(t.main, None);
    }

    #[test]
    fn rejects_unknown_key() {
        assert!(parse_hotkey("option key").is_err());
    }

    #[test]
    fn rejects_bare_chord() {
        // two modifiers with no main key isn't a supported bare-modifier hotkey
        assert!(parse_hotkey("alt+shift").is_err());
    }

    #[test]
    fn is_active_bare_modifier_only_alone() {
        let target = parse_hotkey("alt").unwrap();
        let mut held = HashSet::new();
        assert!(is_active(&target, &held, CGEventFlags::CGEventFlagAlternate));
        held.insert(KeyCode::ANSI_J);
        assert!(!is_active(&target, &held, CGEventFlags::CGEventFlagAlternate));
    }

    #[test]
    fn is_active_combo_requires_both() {
        let target = parse_hotkey("alt+space").unwrap();
        let mut held = HashSet::new();
        assert!(!is_active(&target, &held, CGEventFlags::CGEventFlagAlternate));
        held.insert(KeyCode::SPACE);
        assert!(is_active(&target, &held, CGEventFlags::CGEventFlagAlternate));
    }
}
