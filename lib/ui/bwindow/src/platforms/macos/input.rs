/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::null_mut;

use objc2::msg_send;
use objc2::runtime::{AnyObject as Object, Bool};

use super::headers::*;
use crate::{
    ButtonState, Key, KeyCode, KeyboardEvent, LogicalPoint, Modifiers, MouseButton, NamedKey,
    ScrollDelta, WindowEvent,
};

/// Translate native modifier flags.
pub(super) const fn macos_modifiers(flags: u64) -> Modifiers {
    let mut bits = 0;
    if flags & NS_EVENT_MODIFIER_FLAG_COMMAND != 0 {
        bits |= 1;
    }
    if flags & NS_EVENT_MODIFIER_FLAG_CONTROL != 0 {
        bits |= 2;
    }
    if flags & NS_EVENT_MODIFIER_FLAG_OPTION != 0 {
        bits |= 4;
    }
    if flags & NS_EVENT_MODIFIER_FLAG_SHIFT != 0 {
        bits |= 8;
    }
    Modifiers(bits)
}

/// Translate a native pointer event in a flipped content view.
///
/// # Safety
/// Both pointers must refer to live AppKit objects of the stated types on the main thread.
pub(super) unsafe fn macos_pointer_event(
    view: *mut Object,
    event: *mut Object,
    state: Option<ButtonState>,
) -> WindowEvent {
    let point: NSPoint = unsafe { msg_send![event, locationInWindow] };
    let point: NSPoint =
        unsafe { msg_send![view, convertPoint:point, fromView:null_mut::<Object>()] };
    let flags: u64 = unsafe { msg_send![event, modifierFlags] };
    let position = LogicalPoint::new(point.x as f32, point.y as f32);
    let modifiers = macos_modifiers(flags);
    if let Some(state) = state {
        let number: i64 = unsafe { msg_send![event, buttonNumber] };
        let button = match number {
            0 => MouseButton::Left,
            1 => MouseButton::Right,
            2 => MouseButton::Middle,
            number => MouseButton::Other(number.clamp(0, u16::MAX.into()) as u16),
        };
        let event = crate::MouseEvent {
            button,
            position,
            modifiers,
        };
        match state {
            ButtonState::Pressed => WindowEvent::MouseDown(event),
            ButtonState::Released => WindowEvent::MouseUp(event),
        }
    } else {
        WindowEvent::MouseMove {
            position,
            modifiers,
        }
    }
}

/// Translate an AppKit scroll event.
///
/// # Safety
/// `event` must be a live NSEvent on the main thread.
pub(super) unsafe fn macos_scroll_event(event: *mut Object) -> WindowEvent {
    let precise: Bool = unsafe { msg_send![event, hasPreciseScrollingDeltas] };
    let x: f64 = unsafe { msg_send![event, scrollingDeltaX] };
    let y: f64 = unsafe { msg_send![event, scrollingDeltaY] };
    let flags: u64 = unsafe { msg_send![event, modifierFlags] };
    let delta = if precise.as_bool() {
        ScrollDelta::Pixels(-x, -y)
    } else {
        ScrollDelta::Lines(-x, -y)
    };
    WindowEvent::Wheel(delta, macos_modifiers(flags))
}

/// Translate an AppKit key event. Text composition is not performed here.
///
/// # Safety
/// `event` must be a live NSEvent key event on the main thread.
pub(super) unsafe fn macos_key_event(event: *mut Object, state: ButtonState) -> KeyboardEvent {
    let code: u16 = unsafe { msg_send![event, keyCode] };
    let flags: u64 = unsafe { msg_send![event, modifierFlags] };
    let repeat: Bool = unsafe { msg_send![event, isARepeat] };
    let key = match code {
        36 | 76 => Key::Named(NamedKey::Enter),
        48 => Key::Named(NamedKey::Tab),
        51 => Key::Named(NamedKey::Backspace),
        53 => Key::Named(NamedKey::Escape),
        117 => Key::Named(NamedKey::Delete),
        115 => Key::Named(NamedKey::Home),
        119 => Key::Named(NamedKey::End),
        116 => Key::Named(NamedKey::PageUp),
        121 => Key::Named(NamedKey::PageDown),
        123 => Key::Named(NamedKey::ArrowLeft),
        124 => Key::Named(NamedKey::ArrowRight),
        125 => Key::Named(NamedKey::ArrowDown),
        126 => Key::Named(NamedKey::ArrowUp),
        _ => {
            let chars: *mut Object = unsafe { msg_send![event, characters] };
            if chars.is_null() {
                Key::Unidentified
            } else {
                let chars: NSString = unsafe { msg_send![chars, copy] };
                let text = chars.to_string();
                match text.chars().next() {
                    Some(c) if ('\u{f704}'..='\u{f71b}').contains(&c) => {
                        Key::Named(NamedKey::Function((c as u32 - 0xf704 + 1) as u8))
                    }
                    Some(c) if !c.is_control() => Key::Character(text),
                    _ => Key::Unidentified,
                }
            }
        }
    };
    KeyboardEvent {
        code: physical_key(code),
        key,
        repeat: state == ButtonState::Pressed && repeat.as_bool(),
        modifiers: macos_modifiers(flags),
    }
}

const fn physical_key(code: u16) -> Option<KeyCode> {
    Some(match code {
        0 => KeyCode::KeyA,
        1 => KeyCode::KeyS,
        2 => KeyCode::KeyD,
        3 => KeyCode::KeyF,
        4 => KeyCode::KeyH,
        5 => KeyCode::KeyG,
        6 => KeyCode::KeyZ,
        7 => KeyCode::KeyX,
        8 => KeyCode::KeyC,
        9 => KeyCode::KeyV,
        11 => KeyCode::KeyB,
        12 => KeyCode::KeyQ,
        13 => KeyCode::KeyW,
        14 => KeyCode::KeyE,
        15 => KeyCode::KeyR,
        16 => KeyCode::KeyY,
        17 => KeyCode::KeyT,
        31 => KeyCode::KeyO,
        32 => KeyCode::KeyU,
        34 => KeyCode::KeyI,
        35 => KeyCode::KeyP,
        37 => KeyCode::KeyL,
        38 => KeyCode::KeyJ,
        40 => KeyCode::KeyK,
        45 => KeyCode::KeyN,
        46 => KeyCode::KeyM,
        36 => KeyCode::Enter,
        48 => KeyCode::Tab,
        49 => KeyCode::Space,
        51 => KeyCode::Backspace,
        53 => KeyCode::Escape,
        123 => KeyCode::ArrowLeft,
        124 => KeyCode::ArrowRight,
        125 => KeyCode::ArrowDown,
        126 => KeyCode::ArrowUp,
        18 => KeyCode::Digit1,
        19 => KeyCode::Digit2,
        20 => KeyCode::Digit3,
        21 => KeyCode::Digit4,
        23 => KeyCode::Digit5,
        22 => KeyCode::Digit6,
        26 => KeyCode::Digit7,
        28 => KeyCode::Digit8,
        25 => KeyCode::Digit9,
        29 => KeyCode::Digit0,
        117 => KeyCode::Delete,
        115 => KeyCode::Home,
        119 => KeyCode::End,
        116 => KeyCode::PageUp,
        121 => KeyCode::PageDown,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_keys_and_modifiers_without_layout_assumptions() {
        assert_eq!(physical_key(15), Some(KeyCode::KeyR));
        assert_eq!(physical_key(123), Some(KeyCode::ArrowLeft));
        assert_eq!(physical_key(u16::MAX), None);
        assert_eq!(
            macos_modifiers(NS_EVENT_MODIFIER_FLAG_COMMAND | NS_EVENT_MODIFIER_FLAG_SHIFT),
            Modifiers::SUPER | Modifiers::SHIFT,
        );
    }
}
