/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::{
    Key, KeyCode, KeyboardEvent, LogicalPoint, Modifiers, MouseButton, MouseEvent, NamedKey,
    ScrollDelta, Touch, TouchEvent, WindowEvent,
};

// The masked AMotionEvent actions.
const ACTION_DOWN: i32 = 0;
const ACTION_UP: i32 = 1;
const ACTION_MOVE: i32 = 2;
const ACTION_CANCEL: i32 = 3;
const ACTION_HOVER_MOVE: i32 = 7;
const ACTION_SCROLL: i32 = 8;
const ACTION_HOVER_EXIT: i32 = 10;

/// Translate a motion event of the first pointer, touches act like a mouse that leaves when
/// the finger lifts. Scroll axis values are positive to the right and down.
pub(super) fn motion_events(
    action: i32,
    position: LogicalPoint,
    mouse: bool,
    (scroll_x, scroll_y): (f32, f32),
) -> Vec<WindowEvent> {
    let button = MouseEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::NONE,
    };
    let moved = WindowEvent::MouseMove {
        position,
        movement: LogicalPoint::default(),
        modifiers: Modifiers::NONE,
    };
    match action {
        ACTION_DOWN => vec![moved, WindowEvent::MouseDown(button)],
        ACTION_MOVE | ACTION_HOVER_MOVE => vec![moved],
        ACTION_UP | ACTION_CANCEL if mouse => vec![WindowEvent::MouseUp(button)],
        ACTION_UP | ACTION_CANCEL => vec![WindowEvent::MouseUp(button), WindowEvent::MouseLeave],
        ACTION_HOVER_EXIT => vec![WindowEvent::MouseLeave],
        ACTION_SCROLL => vec![WindowEvent::Wheel(
            ScrollDelta::Lines(f64::from(scroll_x), f64::from(scroll_y)),
            Modifiers::NONE,
        )],
        _ => Vec::new(),
    }
}

/// Translate changed fingers, using the action's pointer index for down/up events.
pub(super) fn touch_event(action: i32, touches: Vec<Touch>) -> Option<WindowEvent> {
    let kind = action & 0xff;
    let changed_touches = match kind {
        ACTION_DOWN | ACTION_UP | 5 | 6 => vec![*touches.get((action >> 8) as usize)?],
        ACTION_MOVE | ACTION_CANCEL => touches,
        _ => return None,
    };
    let event = TouchEvent {
        changed_touches,
        ..TouchEvent::default()
    };
    Some(match kind {
        ACTION_DOWN | 5 => WindowEvent::TouchStart(event),
        ACTION_UP | 6 => WindowEvent::TouchEnd(event),
        ACTION_MOVE => WindowEvent::TouchMove(event),
        _ => WindowEvent::TouchCancel(event),
    })
}

/// Translate an Android key event.
pub(super) fn key_event(
    down: bool,
    key_code: i32,
    unicode: i32,
    repeat: i32,
    meta_state: i32,
) -> WindowEvent {
    let named = named_key(key_code);
    let key = match (named, char::from_u32(unicode as u32)) {
        (Some(named), _) => Key::Named(named),
        (None, Some(character)) if unicode > 0 && !character.is_control() => {
            Key::Character(character.to_string())
        }
        _ => Key::Unidentified,
    };
    let event = KeyboardEvent {
        code: key_code_from_android(key_code),
        key,
        repeat: repeat > 0,
        modifiers: modifiers(meta_state),
    };
    if down {
        WindowEvent::KeyDown(event)
    } else {
        WindowEvent::KeyUp(event)
    }
}

const fn modifiers(meta_state: i32) -> Modifiers {
    let mut modifiers = Modifiers::NONE.0;
    if meta_state & 0x1 != 0 {
        modifiers |= Modifiers::SHIFT.0;
    }
    if meta_state & 0x2 != 0 {
        modifiers |= Modifiers::ALT.0;
    }
    if meta_state & 0x1000 != 0 {
        modifiers |= Modifiers::CONTROL.0;
    }
    if meta_state & 0x10000 != 0 {
        modifiers |= Modifiers::META.0;
    }
    Modifiers(modifiers)
}

const fn named_key(key_code: i32) -> Option<NamedKey> {
    Some(match key_code {
        66 | 160 => NamedKey::Enter,
        61 => NamedKey::Tab,
        67 => NamedKey::Backspace,
        112 => NamedKey::Delete,
        111 => NamedKey::Escape,
        19 => NamedKey::ArrowUp,
        20 => NamedKey::ArrowDown,
        21 => NamedKey::ArrowLeft,
        22 => NamedKey::ArrowRight,
        122 => NamedKey::Home,
        123 => NamedKey::End,
        92 => NamedKey::PageUp,
        93 => NamedKey::PageDown,
        131..=142 => NamedKey::Function((key_code - 130) as u8),
        _ => return None,
    })
}

const fn key_code_from_android(key_code: i32) -> Option<KeyCode> {
    const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyI,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
    ];
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    const FUNCTIONS: [KeyCode; 12] = [
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
    ];
    Some(match key_code {
        29..=54 => LETTERS[(key_code - 29) as usize],
        7..=16 => DIGITS[(key_code - 7) as usize],
        131..=142 => FUNCTIONS[(key_code - 131) as usize],
        68 => KeyCode::Backquote,
        69 => KeyCode::Minus,
        70 => KeyCode::Equal,
        71 => KeyCode::BracketLeft,
        72 => KeyCode::BracketRight,
        73 => KeyCode::Backslash,
        74 => KeyCode::Semicolon,
        75 => KeyCode::Quote,
        76 => KeyCode::Slash,
        55 => KeyCode::Comma,
        56 => KeyCode::Period,
        62 => KeyCode::Space,
        61 => KeyCode::Tab,
        67 => KeyCode::Backspace,
        112 => KeyCode::Delete,
        66 | 160 => KeyCode::Enter,
        111 => KeyCode::Escape,
        19 => KeyCode::ArrowUp,
        20 => KeyCode::ArrowDown,
        21 => KeyCode::ArrowLeft,
        22 => KeyCode::ArrowRight,
        122 => KeyCode::Home,
        123 => KeyCode::End,
        92 => KeyCode::PageUp,
        93 => KeyCode::PageDown,
        _ => return None,
    })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multitouch_actions_use_stable_ids_and_the_changed_pointer_index() {
        let touches = vec![
            Touch {
                identifier: 7,
                position: LogicalPoint::new(10.0, 20.0),
            },
            Touch {
                identifier: 42,
                position: LogicalPoint::new(30.0, 40.0),
            },
        ];
        let Some(WindowEvent::TouchStart(start)) = touch_event(0x0105, touches.clone()) else {
            panic!("expected secondary touch start");
        };
        assert_eq!(start.changed_touches, touches[1..]);
        let Some(WindowEvent::TouchEnd(end)) = touch_event(0x0106, touches.clone()) else {
            panic!("expected secondary touch end");
        };
        assert_eq!(end.changed_touches, touches[1..]);
        let Some(WindowEvent::TouchMove(moved)) = touch_event(ACTION_MOVE, touches.clone()) else {
            panic!("expected touch move");
        };
        assert_eq!(moved.changed_touches, touches);
        let Some(WindowEvent::TouchCancel(cancel)) = touch_event(ACTION_CANCEL, touches.clone())
        else {
            panic!("expected touch cancel");
        };
        assert_eq!(cancel.changed_touches, touches);
        assert!(touch_event(0x0205, touches.clone()).is_none());
        assert!(touch_event(ACTION_HOVER_MOVE, touches).is_none());
    }

    #[test]
    fn touches_leave_after_lifting_but_mice_stay() {
        let position = LogicalPoint::new(10.0, 20.0);
        let no_scroll = (0.0, 0.0);
        assert!(matches!(
            motion_events(ACTION_DOWN, position, false, no_scroll).as_slice(),
            [
                WindowEvent::MouseMove { .. },
                WindowEvent::MouseDown(MouseEvent {
                    button: MouseButton::Left,
                    ..
                })
            ]
        ));
        assert!(matches!(
            motion_events(ACTION_CANCEL, position, false, no_scroll).as_slice(),
            [WindowEvent::MouseUp(_), WindowEvent::MouseLeave]
        ));
        assert!(matches!(
            motion_events(ACTION_UP, position, true, no_scroll).as_slice(),
            [WindowEvent::MouseUp(_)]
        ));
        assert!(matches!(
            motion_events(ACTION_HOVER_EXIT, position, true, no_scroll).as_slice(),
            [WindowEvent::MouseLeave]
        ));
        assert!(matches!(
            motion_events(ACTION_SCROLL, position, true, (1.0, -2.0)).as_slice(),
            [WindowEvent::Wheel(ScrollDelta::Lines(1.0, -2.0), _)]
        ));
    }

    #[test]
    fn keys_map_to_codes_characters_and_modifiers() {
        let WindowEvent::KeyDown(event) = key_event(true, 46, 'R' as i32, 0, 0x1001) else {
            panic!("expected key down");
        };
        assert_eq!(event.code, Some(KeyCode::KeyR));
        assert_eq!(event.key, Key::Character("R".to_string()));
        assert_eq!(event.modifiers, Modifiers::SHIFT | Modifiers::CONTROL);
        assert!(!event.repeat);

        let WindowEvent::KeyUp(event) = key_event(false, 21, 0, 2, 0) else {
            panic!("expected key up");
        };
        assert_eq!(event.code, Some(KeyCode::ArrowLeft));
        assert_eq!(event.key, Key::Named(NamedKey::ArrowLeft));
        assert!(event.repeat);

        let WindowEvent::KeyDown(event) = key_event(true, 135, 0, 0, 0) else {
            panic!("expected key down");
        };
        assert_eq!(event.code, Some(KeyCode::F5));
        assert_eq!(event.key, Key::Named(NamedKey::Function(5)));
        let WindowEvent::KeyDown(event) = key_event(true, 9999, 0, 0, 0) else {
            panic!("expected key down");
        };
        assert_eq!((event.code, event.key), (None, Key::Unidentified));
    }
}
