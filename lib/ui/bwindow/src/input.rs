/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ops::{BitOr, BitOrAssign};

pub(crate) const fn set1_key_code(scan: u32, extended: bool) -> Option<KeyCode> {
    Some(match (scan, extended) {
        (0x01, _) => KeyCode::Escape,
        (0x0e, _) => KeyCode::Backspace,
        (0x0f, _) => KeyCode::Tab,
        (0x1c, _) => KeyCode::Enter,
        (0x39, _) => KeyCode::Space,
        (0x47, true) => KeyCode::Home,
        (0x48, true) => KeyCode::ArrowUp,
        (0x49, true) => KeyCode::PageUp,
        (0x4b, true) => KeyCode::ArrowLeft,
        (0x4d, true) => KeyCode::ArrowRight,
        (0x4f, true) => KeyCode::End,
        (0x50, true) => KeyCode::ArrowDown,
        (0x51, true) => KeyCode::PageDown,
        (0x53, true) => KeyCode::Delete,
        (0x10, false) => KeyCode::KeyQ,
        (0x11, false) => KeyCode::KeyW,
        (0x12, false) => KeyCode::KeyE,
        (0x13, false) => KeyCode::KeyR,
        (0x14, false) => KeyCode::KeyT,
        (0x15, false) => KeyCode::KeyY,
        (0x16, false) => KeyCode::KeyU,
        (0x17, false) => KeyCode::KeyI,
        (0x18, false) => KeyCode::KeyO,
        (0x19, false) => KeyCode::KeyP,
        (0x1e, false) => KeyCode::KeyA,
        (0x1f, false) => KeyCode::KeyS,
        (0x20, false) => KeyCode::KeyD,
        (0x21, false) => KeyCode::KeyF,
        (0x22, false) => KeyCode::KeyG,
        (0x23, false) => KeyCode::KeyH,
        (0x24, false) => KeyCode::KeyJ,
        (0x25, false) => KeyCode::KeyK,
        (0x26, false) => KeyCode::KeyL,
        (0x2c, false) => KeyCode::KeyZ,
        (0x2d, false) => KeyCode::KeyX,
        (0x2e, false) => KeyCode::KeyC,
        (0x2f, false) => KeyCode::KeyV,
        (0x30, false) => KeyCode::KeyB,
        (0x31, false) => KeyCode::KeyN,
        (0x32, false) => KeyCode::KeyM,
        (2, false) => KeyCode::Digit1,
        (3, false) => KeyCode::Digit2,
        (4, false) => KeyCode::Digit3,
        (5, false) => KeyCode::Digit4,
        (6, false) => KeyCode::Digit5,
        (7, false) => KeyCode::Digit6,
        (8, false) => KeyCode::Digit7,
        (9, false) => KeyCode::Digit8,
        (10, false) => KeyCode::Digit9,
        (11, false) => KeyCode::Digit0,
        _ => return None,
    })
}

/// Keyboard modifiers, shared by native input and menu shortcuts
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers(pub(crate) u8);

impl Modifiers {
    /// No modifiers
    pub const NONE: Self = Self(0);
    /// Meta key: Command on macOS, Windows/Super on Windows and Linux.
    pub const META: Self = Self(1 << 0);
    /// Command key (alias for Meta).
    pub const COMMAND: Self = Self::META;
    /// Command on macOS, Control on other platforms.
    pub const PRIMARY: Self = if cfg!(target_os = "macos") {
        Self::COMMAND
    } else {
        Self::CONTROL
    };
    /// Control key
    pub const CONTROL: Self = Self(1 << 1);
    /// Alt key (Option on macOS).
    pub const ALT: Self = Self(1 << 2);
    /// Option key (alias for Alt).
    pub const OPTION: Self = Self::ALT;
    /// Super/Meta key (alias for Command on macOS)
    pub const SUPER: Self = Self::META;
    /// Shift key
    pub const SHIFT: Self = Self(1 << 3);

    /// Returns whether every modifier in `other` is set
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether Shift is held, equivalent to DOM `shiftKey`.
    pub const fn shift_key(self) -> bool {
        self.contains(Self::SHIFT)
    }
    /// Whether Control is held, equivalent to DOM `ctrlKey`.
    pub const fn ctrl_key(self) -> bool {
        self.contains(Self::CONTROL)
    }
    /// Whether Alt/Option is held, equivalent to DOM `altKey`.
    pub const fn alt_key(self) -> bool {
        self.contains(Self::ALT)
    }
    /// Whether Meta is held, equivalent to DOM `metaKey`.
    pub const fn meta_key(self) -> bool {
        self.contains(Self::META)
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Modifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// A physical keyboard key, named after DOM `KeyboardEvent.code` values.
/// This is not the deprecated numeric DOM `keyCode` property.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A key
    KeyA,
    /// B key
    KeyB,
    /// C key
    KeyC,
    /// D key
    KeyD,
    /// E key
    KeyE,
    /// F key
    KeyF,
    /// G key
    KeyG,
    /// H key
    KeyH,
    /// I key
    KeyI,
    /// J key
    KeyJ,
    /// K key
    KeyK,
    /// L key
    KeyL,
    /// M key
    KeyM,
    /// N key
    KeyN,
    /// O key
    KeyO,
    /// P key
    KeyP,
    /// Q key
    KeyQ,
    /// R key
    KeyR,
    /// S key
    KeyS,
    /// T key
    KeyT,
    /// U key
    KeyU,
    /// V key
    KeyV,
    /// W key
    KeyW,
    /// X key
    KeyX,
    /// Y key
    KeyY,
    /// Z key
    KeyZ,
    /// Top-row 0 key
    Digit0,
    /// Top-row 1 key
    Digit1,
    /// Top-row 2 key
    Digit2,
    /// Top-row 3 key
    Digit3,
    /// Top-row 4 key
    Digit4,
    /// Top-row 5 key
    Digit5,
    /// Top-row 6 key
    Digit6,
    /// Top-row 7 key
    Digit7,
    /// Top-row 8 key
    Digit8,
    /// Top-row 9 key
    Digit9,
    /// F1 key
    F1,
    /// F2 key
    F2,
    /// F3 key
    F3,
    /// F4 key
    F4,
    /// F5 key
    F5,
    /// F6 key
    F6,
    /// F7 key
    F7,
    /// F8 key
    F8,
    /// F9 key
    F9,
    /// F10 key
    F10,
    /// F11 key
    F11,
    /// F12 key
    F12,
    /// F13 key
    F13,
    /// F14 key
    F14,
    /// F15 key
    F15,
    /// F16 key
    F16,
    /// F17 key
    F17,
    /// F18 key
    F18,
    /// F19 key
    F19,
    /// F20 key
    F20,
    /// F21 key
    F21,
    /// F22 key
    F22,
    /// F23 key
    F23,
    /// F24 key
    F24,
    /// Backquote key
    Backquote,
    /// Backslash key
    Backslash,
    /// Left bracket key
    BracketLeft,
    /// Right bracket key
    BracketRight,
    /// Comma key
    Comma,
    /// Equal key
    Equal,
    /// Minus key
    Minus,
    /// Period key
    Period,
    /// Quote key
    Quote,
    /// Semicolon key
    Semicolon,
    /// Slash key
    Slash,
    /// Space key
    Space,
    /// Tab key
    Tab,
    /// Backspace key
    Backspace,
    /// Forward-delete key
    Delete,
    /// Enter/Return key
    Enter,
    /// Escape key
    Escape,
    /// Up arrow key
    ArrowUp,
    /// Down arrow key
    ArrowDown,
    /// Left arrow key
    ArrowLeft,
    /// Right arrow key
    ArrowRight,
    /// Home key
    Home,
    /// End key
    End,
    /// Page Up key
    PageUp,
    /// Page Down key
    PageDown,
}

/// Whether an input button is pressed or released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonState {
    /// Button pressed.
    Pressed,
    /// Button released.
    Released,
}

/// A pointer button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// Primary button.
    Left,
    /// Secondary button.
    Right,
    /// Middle button.
    Middle,
    /// Additional native button number.
    Other(u16),
}

/// A non-text logical key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamedKey {
    /// Return or enter.
    Enter,
    /// Tab.
    Tab,
    /// Backspace.
    Backspace,
    /// Forward delete.
    Delete,
    /// Escape.
    Escape,
    /// Left arrow.
    ArrowLeft,
    /// Right arrow.
    ArrowRight,
    /// Up arrow.
    ArrowUp,
    /// Down arrow.
    ArrowDown,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Function key number.
    Function(u8),
}

/// A layout-dependent key. This is not an IME text-input API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Key {
    /// Text produced by the active keyboard layout.
    Character(String),
    /// Non-text key.
    Named(NamedKey),
    /// No portable mapping.
    Unidentified,
}

/// Mouse button event data. The enclosing `MouseDown` or `MouseUp` variant gives its state.
#[derive(Clone, Copy, Debug)]
pub struct MouseEvent {
    /// Button that changed, not a bitmask of all held buttons.
    pub button: MouseButton,
    /// Position in logical client-area pixels, analogous to DOM `clientX`/`clientY`.
    pub position: crate::LogicalPoint,
    /// Active keyboard modifiers.
    pub modifiers: Modifiers,
}

/// Native keyboard data. The enclosing `KeyDown` or `KeyUp` variant gives its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyboardEvent {
    /// Physical key, analogous to DOM `code`, when a portable mapping is available.
    pub code: Option<KeyCode>,
    /// Layout-dependent logical key.
    pub key: Key,
    /// Whether this is an auto-repeat press.
    pub repeat: bool,
    /// Active modifiers.
    pub modifiers: Modifiers,
}

/// Scroll distance. Positive values scroll right and down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollDelta {
    /// Logical pixels from a high-resolution pointing device.
    Pixels(f64, f64),
    /// Lines from a discrete wheel.
    Lines(f64, f64),
}

#[cfg(test)]
mod tests {
    use super::Modifiers;

    #[test]
    fn modifier_aliases_and_combinations() {
        let modifiers = Modifiers::CONTROL | Modifiers::SHIFT;
        assert!(modifiers.contains(Modifiers::CONTROL));
        assert!(!modifiers.contains(Modifiers::ALT));
        assert_eq!(Modifiers::COMMAND, Modifiers::SUPER);
        assert_eq!(Modifiers::COMMAND, Modifiers::META);
        assert_eq!(Modifiers::OPTION, Modifiers::ALT);
        assert_eq!(Modifiers::default(), Modifiers::NONE);
        assert!(modifiers.ctrl_key());
        assert!(modifiers.shift_key());
        assert!(!modifiers.alt_key());
        assert!(!modifiers.meta_key());
        let modifiers = Modifiers::ALT | Modifiers::META;
        assert!(modifiers.alt_key());
        assert!(modifiers.meta_key());
        assert!(!modifiers.ctrl_key());
        assert!(!modifiers.shift_key());
    }
}
