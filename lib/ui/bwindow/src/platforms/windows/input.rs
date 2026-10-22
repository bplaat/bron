/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::headers::{GetKeyState, GetKeyboardLayout, GetKeyboardState, ToUnicodeEx};
use crate::input::set1_key_code as physical_key;
use crate::{ButtonState, Key, KeyboardEvent, Modifiers, NamedKey};

/// Current Win32 keyboard modifiers on the calling UI thread.
pub(super) fn windows_modifiers() -> Modifiers {
    let mut modifiers = Modifiers::NONE;
    for (key, modifier) in [
        (0x10, Modifiers::SHIFT),
        (0x11, Modifiers::CONTROL),
        (0x12, Modifiers::ALT),
        (0x5b, Modifiers::SUPER),
        (0x5c, Modifiers::SUPER),
    ] {
        if unsafe { GetKeyState(key) } < 0 {
            modifiers |= modifier;
        }
    }
    modifiers
}

/// Translate WM_KEYDOWN/UP parameters on the window's UI thread.
/// This reports keys, not composed text or IME input.
pub(super) fn windows_key_event(key: u32, lparam: isize, state: ButtonState) -> KeyboardEvent {
    let scan = ((lparam >> 16) & 0xff) as u32;
    let named = match key {
        0x08 => Some(NamedKey::Backspace),
        0x09 => Some(NamedKey::Tab),
        0x0d => Some(NamedKey::Enter),
        0x1b => Some(NamedKey::Escape),
        0x21 => Some(NamedKey::PageUp),
        0x22 => Some(NamedKey::PageDown),
        0x23 => Some(NamedKey::End),
        0x24 => Some(NamedKey::Home),
        0x25 => Some(NamedKey::ArrowLeft),
        0x26 => Some(NamedKey::ArrowUp),
        0x27 => Some(NamedKey::ArrowRight),
        0x28 => Some(NamedKey::ArrowDown),
        0x2e => Some(NamedKey::Delete),
        0x70..=0x87 => Some(NamedKey::Function((key - 0x70 + 1) as u8)),
        _ => None,
    };
    let logical = if let Some(named) = named {
        Key::Named(named)
    } else {
        let mut keyboard = [0; 256];
        let mut text = [0; 16];
        let count = unsafe {
            if GetKeyboardState(keyboard.as_mut_ptr()) == 0 {
                0
            } else {
                // Do not change the OS dead-key state while observing a key event.
                ToUnicodeEx(
                    key,
                    scan,
                    keyboard.as_ptr(),
                    text.as_mut_ptr(),
                    text.len() as i32,
                    4,
                    GetKeyboardLayout(0),
                )
            }
        };
        if count > 0 && count as usize <= text.len() {
            let text = String::from_utf16_lossy(&text[..count as usize]);
            if text.chars().all(|c| !c.is_control()) {
                Key::Character(text)
            } else {
                Key::Unidentified
            }
        } else {
            Key::Unidentified
        }
    };
    KeyboardEvent {
        code: physical_key(scan, lparam & (1 << 24) != 0),
        key: logical,
        repeat: state == ButtonState::Pressed && lparam & (1 << 30) != 0,
        modifiers: windows_modifiers(),
    }
}

/// Translate a WebView2 accelerator using the application's native shortcut table.
/// Call on the owning UI thread with the live parent window handle.
#[allow(
    clippy::missing_const_for_fn,
    reason = "only const without the menu feature"
)]
pub fn windows_translate_accelerator(
    window: *mut std::ffi::c_void,
    key: u32,
    lparam: isize,
    system: bool,
) -> bool {
    #[cfg(feature = "menu")]
    {
        super::menu::translate_webview(window, key, lparam, system)
    }
    #[cfg(not(feature = "menu"))]
    {
        let _ = (window, key, lparam, system);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyCode;

    #[test]
    fn physical_keys_use_scan_codes_not_virtual_layout_keys() {
        assert_eq!(physical_key(0x13, false), Some(KeyCode::KeyR));
        assert_eq!(physical_key(0x4b, true), Some(KeyCode::ArrowLeft));
        assert_eq!(physical_key(0x4b, false), None);
    }
}
