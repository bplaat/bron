/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;

use super::headers::*;
use crate::{
    Key, KeyboardEvent, LogicalPoint, Modifiers, MouseButton, NamedKey, ScrollDelta, WindowEvent,
};

/// Translate GDK modifier flags.
pub const fn gtk_modifiers(state: u32) -> Modifiers {
    let mut bits = 0;
    if state & 1 != 0 {
        bits |= 8;
    }
    if state & 4 != 0 {
        bits |= 2;
    }
    if state & 8 != 0 {
        bits |= 4;
    }
    if state & ((1 << 26) | (1 << 28)) != 0 {
        bits |= 1;
    }
    Modifiers(bits)
}

/// Translate pointer, scroll, and key events from GDK.
/// GDK hardware codes outside the standard XKB/evdev range stay unidentified.
///
/// # Safety
/// `event` must be a live GdkEvent on the GTK UI thread.
pub unsafe fn gtk_input_event(event: *const c_void, repeat: bool) -> Option<WindowEvent> {
    let event = event.cast::<GdkEvent>();
    let kind = unsafe { gdk_event_get_event_type(event) };
    let mut state = 0;
    unsafe { gdk_event_get_state(event, &mut state) };
    let modifiers = gtk_modifiers(state);
    let (mut x, mut y) = (0.0, 0.0);
    unsafe { gdk_event_get_coords(event, &mut x, &mut y) };
    let position = LogicalPoint::new(x as f32, y as f32);
    Some(match kind {
        3 => WindowEvent::MouseMove {
            position,
            modifiers,
        },
        4 | 7 => {
            let mut button = 0;
            unsafe { gdk_event_get_button(event, &mut button) };
            let event = crate::MouseEvent {
                button: match button {
                    1 => MouseButton::Left,
                    2 => MouseButton::Middle,
                    3 => MouseButton::Right,
                    other => MouseButton::Other(other.min(u16::MAX.into()) as u16),
                },
                position,
                modifiers,
            };
            if kind == 4 {
                WindowEvent::MouseDown(event)
            } else {
                WindowEvent::MouseUp(event)
            }
        }
        8 | 9 => {
            let mut keyval = 0;
            let mut code = 0;
            unsafe {
                gdk_event_get_keyval(event, &mut keyval);
                gdk_event_get_keycode(event, &mut code);
            }
            let event = KeyboardEvent {
                code: gtk_physical_key(code),
                key: logical_key(keyval),
                repeat: kind == 8 && repeat,
                modifiers,
            };
            if kind == 8 {
                WindowEvent::KeyDown(event)
            } else {
                WindowEvent::KeyUp(event)
            }
        }
        11 => WindowEvent::MouseLeave,
        31 => {
            let mut direction = 0;
            unsafe { gdk_event_get_scroll_direction(event, &mut direction) };
            let delta = match direction {
                0 => ScrollDelta::Lines(0.0, -1.0),
                1 => ScrollDelta::Lines(0.0, 1.0),
                2 => ScrollDelta::Lines(-1.0, 0.0),
                3 => ScrollDelta::Lines(1.0, 0.0),
                _ => {
                    let (mut x, mut y) = (0.0, 0.0);
                    unsafe { gdk_event_get_scroll_deltas(event, &mut x, &mut y) };
                    // GDK smooth deltas are fractional wheel units, not pixels.
                    ScrollDelta::Lines(x, y)
                }
            };
            WindowEvent::Wheel(delta, modifiers)
        }
        _ => return None,
    })
}

fn logical_key(keyval: u32) -> Key {
    let named = match keyval {
        0xff08 => NamedKey::Backspace,
        0xff09 | 0xfe20 => NamedKey::Tab,
        0xff0d | 0xff8d => NamedKey::Enter,
        0xff1b => NamedKey::Escape,
        0xffff => NamedKey::Delete,
        0xff50 => NamedKey::Home,
        0xff57 => NamedKey::End,
        0xff51 => NamedKey::ArrowLeft,
        0xff52 => NamedKey::ArrowUp,
        0xff53 => NamedKey::ArrowRight,
        0xff54 => NamedKey::ArrowDown,
        0xff55 => NamedKey::PageUp,
        0xff56 => NamedKey::PageDown,
        0xffbe..=0xffd5 => NamedKey::Function((keyval - 0xffbe + 1) as u8),
        _ => {
            return char::from_u32(unsafe { gdk_keyval_to_unicode(keyval) })
                .filter(|c| !c.is_control())
                .map_or(Key::Unidentified, |c| Key::Character(c.to_string()));
        }
    };
    Key::Named(named)
}

const fn gtk_physical_key(code: u16) -> Option<crate::KeyCode> {
    let Some(evdev) = code.checked_sub(8) else {
        return None;
    };
    let scan = match evdev {
        102 => 0x47,
        103 => 0x48,
        104 => 0x49,
        105 => 0x4b,
        106 => 0x4d,
        107 => 0x4f,
        108 => 0x50,
        109 => 0x51,
        111 => 0x53,
        value => value as u32,
    };
    crate::input::set1_key_code(scan, evdev >= 102)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyCode;

    #[test]
    fn maps_xkb_positions_and_modifiers() {
        assert_eq!(gtk_physical_key(27), Some(KeyCode::KeyR));
        assert_eq!(gtk_physical_key(113), Some(KeyCode::ArrowLeft));
        assert_eq!(gtk_physical_key(0), None);
        assert_eq!(gtk_modifiers(5), Modifiers::SHIFT | Modifiers::CONTROL);
    }
}
