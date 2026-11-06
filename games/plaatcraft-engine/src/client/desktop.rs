/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Desktop controls shared by the consoles' USB keyboard and mouse backends.

use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering};

use super::input::{Button, Context, Pad};

const TEXT_CAPACITY: usize = 128;

/// Callback-safe input mailbox. Keyboard text has one producer and one consumer.
pub struct DesktopEvents {
    keys: [AtomicU32; 8],
    pressed: [AtomicU32; 8],
    caps: AtomicBool,
    mouse_connected: AtomicBool,
    buttons: AtomicU32,
    clicks: AtomicU32,
    dx: AtomicI32,
    dy: AtomicI32,
    wheel: AtomicI32,
    text: [AtomicU32; TEXT_CAPACITY],
    head: AtomicUsize,
    tail: AtomicUsize,
    repeat_code: AtomicU32,
    repeat_character: AtomicU32,
    repeat_serial: AtomicU32,
}

impl DesktopEvents {
    /// An empty mailbox, suitable for static storage used by native callbacks.
    pub const fn new() -> Self {
        Self {
            keys: [const { AtomicU32::new(0) }; 8],
            pressed: [const { AtomicU32::new(0) }; 8],
            caps: AtomicBool::new(false),
            mouse_connected: AtomicBool::new(false),
            buttons: AtomicU32::new(0),
            clicks: AtomicU32::new(0),
            dx: AtomicI32::new(0),
            dy: AtomicI32::new(0),
            wheel: AtomicI32::new(0),
            text: [const { AtomicU32::new(0) }; TEXT_CAPACITY],
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            repeat_code: AtomicU32::new(u32::MAX),
            repeat_character: AtomicU32::new(0),
            repeat_serial: AtomicU32::new(0),
        }
    }

    /// Records a USB HID key transition and optional native Unicode translation.
    pub fn key(&self, code: u8, down: bool, character: Option<char>) {
        let index = usize::from(code / 32);
        let mask = 1 << (code % 32);
        if !down {
            self.keys[index].fetch_and(!mask, Ordering::Relaxed);
            return;
        }
        let previous = self.keys[index].fetch_or(mask, Ordering::Relaxed);
        if previous & mask != 0 {
            return;
        }
        self.pressed[index].fetch_or(mask, Ordering::Relaxed);
        if code == 0x39 && previous & mask == 0 {
            self.caps.fetch_xor(true, Ordering::Relaxed);
        }
        let altgr = self.held(0xe6) && character.is_some_and(|ch| !ch.is_control());
        let control =
            self.held(0xe3) || self.held(0xe7) || (self.held(0xe0) || self.held(0xe4)) && !altgr;
        // Native translation may use AltGr to produce a printable character.
        let alt = (self.held(0xe2) || self.held(0xe6)) && character.is_none();
        let character = character
            .or_else(|| self.character(code))
            .filter(|ch| !ch.is_control());
        if !control && !alt && (character.is_some() || matches!(code, 0x2a | 0x4f..=0x52)) {
            self.repeat_character
                .store(character.map_or(0, |ch| ch as u32), Ordering::Relaxed);
            self.repeat_code.store(u32::from(code), Ordering::Relaxed);
            self.repeat_serial.fetch_add(1, Ordering::Release);
        }
        if !control
            && !alt
            && let Some(ch) = character
        {
            let head = self.head.load(Ordering::Relaxed);
            let next = (head + 1) % TEXT_CAPACITY;
            if next != self.tail.load(Ordering::Acquire) {
                self.text[head].store(ch as u32, Ordering::Relaxed);
                self.head.store(next, Ordering::Release);
            }
        }
    }

    fn held(&self, code: u8) -> bool {
        self.keys[usize::from(code / 32)].load(Ordering::Relaxed) & (1 << (code % 32)) != 0
    }

    fn character(&self, code: u8) -> Option<char> {
        let shift = self.held(0xe1) || self.held(0xe5);
        if (4..=29).contains(&code) {
            let letter = b'a' + code - 4;
            return Some(char::from(if shift != self.caps.load(Ordering::Relaxed) {
                letter.to_ascii_uppercase()
            } else {
                letter
            }));
        }
        let (normal, shifted) = match code {
            0x1e..=0x27 => (
                b"1234567890"[(code - 0x1e) as usize],
                b"!@#$%^&*()"[(code - 0x1e) as usize],
            ),
            0x2c => (b' ', b' '),
            0x2d => (b'-', b'_'),
            0x2e => (b'=', b'+'),
            0x2f => (b'[', b'{'),
            0x30 => (b']', b'}'),
            0x31 => (b'\\', b'|'),
            0x33 => (b';', b':'),
            0x34 => (b'\'', b'"'),
            0x35 => (b'`', b'~'),
            0x36 => (b',', b'<'),
            0x37 => (b'.', b'>'),
            0x38 => (b'/', b'?'),
            _ => return None,
        };
        Some(char::from(if shift { shifted } else { normal }))
    }

    /// Clears held keys when the keyboard disconnects.
    pub fn disconnect_keyboard(&self) {
        for key in self.keys.iter().chain(&self.pressed) {
            key.store(0, Ordering::Relaxed);
        }
    }

    /// Records a mouse report: left/right/middle bits, relative movement and wheel steps.
    pub fn mouse(&self, buttons: u8, dx: i32, dy: i32, wheel: i32) {
        self.mouse_connected.store(true, Ordering::Relaxed);
        let buttons = u32::from(buttons & 7);
        let previous = self.buttons.swap(buttons, Ordering::Relaxed);
        self.clicks.fetch_or(buttons & !previous, Ordering::Relaxed);
        self.dx.fetch_add(dx, Ordering::Relaxed);
        self.dy.fetch_add(dy, Ordering::Relaxed);
        self.wheel.fetch_add(wheel, Ordering::Relaxed);
    }

    /// Decodes a standard boot-mouse report, including its common optional wheel byte.
    pub fn boot_mouse(&self, report: &[u8]) -> bool {
        if report.len() < 3 {
            return false;
        }
        self.mouse(
            report[0],
            i32::from(report[1] as i8),
            i32::from(report[2] as i8),
            report.get(3).map_or(0, |value| i32::from(*value as i8)),
        );
        true
    }

    /// Clears held buttons and hides the pointer when the mouse disconnects.
    pub fn disconnect_mouse(&self) {
        self.mouse_connected.store(false, Ordering::Relaxed);
        self.buttons.store(0, Ordering::Relaxed);
        self.clicks.store(0, Ordering::Relaxed);
        self.dx.store(0, Ordering::Relaxed);
        self.dy.store(0, Ordering::Relaxed);
        self.wheel.store(0, Ordering::Relaxed);
    }
}

impl Default for DesktopEvents {
    fn default() -> Self {
        Self::new()
    }
}

/// One frame of USB input, mapped to the desktop game's P1 controls.
pub struct Desktop {
    keys: [u32; 8],
    pressed: [u32; 8],
    buttons: u32,
    clicks: u32,
    motion: [f32; 2],
    wheel: i32,
    position: [f32; 2],
    size: [f32; 2],
    connected: bool,
    text: String,
    repeat: Option<(u32, f64)>,
}

impl Desktop {
    /// Creates a desktop pointer centered in P1's screen space.
    pub fn new(size: [f32; 2]) -> Self {
        Self {
            keys: [0; 8],
            pressed: [0; 8],
            buttons: 0,
            clicks: 0,
            motion: [0.0; 2],
            wheel: 0,
            position: size.map(|v| v / 2.0),
            size,
            connected: false,
            text: String::new(),
            repeat: None,
        }
    }

    /// Takes the callbacks' accumulated input once per frame.
    pub fn poll(&mut self, events: &DesktopEvents, now: f64) {
        for i in 0..8 {
            self.keys[i] = events.keys[i].load(Ordering::Relaxed);
            self.pressed[i] = events.pressed[i].swap(0, Ordering::Relaxed);
        }
        self.buttons = events.buttons.load(Ordering::Relaxed);
        self.clicks = events.clicks.swap(0, Ordering::Relaxed);
        self.motion = [
            events.dx.swap(0, Ordering::Relaxed) as f32,
            events.dy.swap(0, Ordering::Relaxed) as f32,
        ];
        self.wheel = events.wheel.swap(0, Ordering::Relaxed);
        self.connected = events.mouse_connected.load(Ordering::Relaxed);
        for axis in 0..2 {
            self.position[axis] =
                (self.position[axis] + self.motion[axis]).clamp(0.0, self.size[axis]);
        }
        self.text.clear();
        let mut tail = events.tail.load(Ordering::Relaxed);
        let head = events.head.load(Ordering::Acquire);
        while tail != head {
            if let Some(ch) = char::from_u32(events.text[tail].load(Ordering::Relaxed)) {
                self.text.push(ch);
            }
            tail = (tail + 1) % TEXT_CAPACITY;
        }
        events.tail.store(tail, Ordering::Release);
        let serial = events.repeat_serial.load(Ordering::Acquire);
        let code = events.repeat_code.load(Ordering::Relaxed);
        let modifiers = self.keys[7];
        let altgr = modifiers & 0x40 != 0 && events.repeat_character.load(Ordering::Relaxed) != 0;
        let control = modifiers & 0x88 != 0 || modifiers & 0x11 != 0 && !altgr;
        if code < 256 && !control && self.keys[code as usize / 32] & (1 << (code % 32)) != 0 {
            match self.repeat {
                Some((previous, next)) if previous == serial && now >= next => {
                    if let Some(ch) =
                        char::from_u32(events.repeat_character.load(Ordering::Relaxed))
                            .filter(|ch| !ch.is_control())
                    {
                        self.text.push(ch);
                    } else {
                        self.pressed[code as usize / 32] |= 1 << (code % 32);
                    }
                    self.repeat = Some((serial, now + 0.05));
                }
                Some((previous, _)) if previous == serial => {}
                _ => self.repeat = Some((serial, now + 0.4)),
            }
        } else {
            self.repeat = None;
        }
    }

    /// Translated characters typed in this frame.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The mouse's menu position, when connected.
    pub const fn pointer(&self) -> Option<[f32; 2]> {
        if self.connected {
            Some(self.position)
        } else {
            None
        }
    }

    /// The same keys and mouse buttons as the desktop game.
    pub fn pad(&self, context: Context) -> Pad {
        let key = |code: usize| {
            (
                self.keys[code / 32] & (1 << (code % 32)) != 0,
                self.pressed[code / 32] & (1 << (code % 32)) != 0,
            )
        };
        let mouse = |mask| (self.buttons & mask != 0, self.clicks & mask != 0);
        let mut pad = Pad::default();
        pad.scroll = self.wheel;
        for slot in 0..9 {
            if key(0x1e + slot).1 {
                pad.hotbar = Some(slot);
            }
        }
        match context {
            Context::Play => {
                pad.direction = [
                    f32::from(u8::from(key(7).0)) - f32::from(u8::from(key(4).0)),
                    f32::from(u8::from(key(26).0)) - f32::from(u8::from(key(22).0)),
                ];
                pad.mouse_motion = self.motion;
                for (action, value) in [
                    (Button::Jump, key(0x2c)),
                    (Button::Sneak, combine(key(0xe1), key(0xe5))),
                    (Button::Fast, combine(key(0xe0), key(0xe4))),
                    (Button::Break, mouse(1)),
                    (Button::Place, mouse(2)),
                    (Button::Pick, mouse(4)),
                    (Button::Pause, key(0x29)),
                    (Button::Inventory, key(8)),
                    (Button::Chat, combine(key(23), key(0x28))),
                    (Button::Command, key(0x38)),
                ] {
                    pad.set(action, value);
                }
            }
            Context::Menu | Context::Inventory => {
                if self.motion != [0.0; 2]
                    || self.buttons != 0
                    || self.clicks != 0
                    || pad.hotbar.is_some()
                {
                    pad.pointer = self.pointer().map(|[x, y]| [x, y, 0.0]);
                }
                for (action, value) in [
                    (Button::Up, key(0x52)),
                    (Button::Down, key(0x51)),
                    (Button::Left, key(0x50)),
                    (Button::Right, key(0x4f)),
                    (Button::Select, combine(mouse(1), key(0x28))),
                    (Button::Confirm, key(0x28)),
                    (Button::Back, key(0x2a)),
                    (Button::Cancel, key(0x29)),
                ] {
                    pad.set(action, value);
                }
                if !self.text.is_empty() {
                    pad.set(Button::Text, (false, true));
                }
                if context == Context::Inventory {
                    pad.set(Button::Back, combine(key(8), key(0x29)));
                }
            }
        }
        pad
    }
}

const fn combine(a: (bool, bool), b: (bool, bool)) -> (bool, bool) {
    (a.0 || b.0, a.1 || b.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_navigation_repeat_without_retriggering_game_shortcuts() {
        let events = DesktopEvents::new();
        let mut desktop = Desktop::new([640.0, 480.0]);
        events.key(8, true, None);
        desktop.poll(&events, 0.0);
        assert!(desktop.pad(Context::Play).pressed(Button::Inventory));
        events.key(8, true, None);
        desktop.poll(&events, 0.41);
        assert!(!desktop.pad(Context::Play).pressed(Button::Inventory));
        assert!(!desktop.pad(Context::Inventory).pressed(Button::Back));
        assert_eq!(desktop.text(), "e");
        events.key(8, false, None);
        events.key(0x51, true, None);
        desktop.poll(&events, 1.0);
        assert!(desktop.pad(Context::Menu).pressed(Button::Down));
        desktop.poll(&events, 1.2);
        assert!(!desktop.pad(Context::Menu).pressed(Button::Down));
        desktop.poll(&events, 1.41);
        assert!(desktop.pad(Context::Menu).pressed(Button::Down));
        events.key(0x51, false, None);
        desktop.poll(&events, 2.0);
        assert!(!desktop.pad(Context::Menu).pressed(Button::Down));
    }

    #[test]
    fn desktop_controls_match_wasd_modifiers_buttons_numbers_and_relative_look() {
        let events = DesktopEvents::new();
        let mut desktop = Desktop::new([640.0, 480.0]);
        for code in [26, 4, 0x2c, 0xe1, 0xe0, 0x20] {
            events.key(code, true, None);
        }
        events.mouse(1, 12, -8, 1);
        desktop.poll(&events, 0.0);
        let pad = desktop.pad(Context::Play);
        assert_eq!(pad.direction, [-1.0, 1.0]);
        assert_eq!(pad.walk, [0.0; 2]);
        assert_eq!(pad.mouse_motion, [12.0, -8.0]);
        assert_eq!(pad.hotbar, Some(2));
        assert_eq!(pad.scroll, 1);
        assert_eq!(pad.pointer, None);
        for action in [Button::Jump, Button::Sneak, Button::Fast, Button::Break] {
            assert!(pad.held(action));
        }
        assert!(pad.pressed(Button::Break));
        desktop.poll(&events, 0.0);
        let pad = desktop.pad(Context::Play);
        assert!(pad.held(Button::Break));
        assert!(!pad.pressed(Button::Break));
        assert_eq!(pad.mouse_motion, [0.0; 2]);
        assert_eq!(pad.scroll, 0);
        assert_eq!(pad.hotbar, None);
    }

    #[test]
    fn usb_text_supports_shift_caps_native_unicode_and_short_presses() {
        let events = DesktopEvents::new();
        let mut desktop = Desktop::new([640.0, 480.0]);
        events.key(4, true, None);
        events.key(4, false, None);
        events.key(0xe1, true, None);
        events.key(4, true, None);
        events.key(4, false, None);
        events.key(0x1e, true, None);
        events.key(0x1e, false, None);
        events.key(0xe1, false, None);
        events.key(0x39, true, None);
        events.key(0x39, false, None);
        events.key(5, true, None);
        events.key(5, false, None);
        events.key(8, true, Some('é'));
        events.key(8, false, None);
        desktop.poll(&events, 0.0);
        assert_eq!(desktop.text(), "aA!Bé");
        assert!(desktop.pad(Context::Menu).pressed(Button::Text));
        desktop.poll(&events, 0.0);
        assert_eq!(desktop.text(), "");
    }

    #[test]
    fn native_altgr_text_is_not_discarded_as_a_control_shortcut() {
        let events = DesktopEvents::new();
        let mut desktop = Desktop::new([640.0, 480.0]);
        events.key(0xe0, true, Some('\0'));
        events.key(0xe6, true, Some('\0'));
        events.key(0x24, true, Some('{'));
        desktop.poll(&events, 0.0);
        assert_eq!(desktop.text(), "{");
        desktop.poll(&events, 0.41);
        assert_eq!(desktop.text(), "{");
        events.key(0x24, false, Some('\0'));
        events.key(0xe6, false, Some('\0'));
        events.key(4, true, Some('a'));
        desktop.poll(&events, 0.0);
        assert_eq!(desktop.text(), "");
    }

    #[test]
    fn boot_mouse_reports_decode_signed_motion_and_release_on_disconnect() {
        let events = DesktopEvents::new();
        let mut desktop = Desktop::new([640.0, 480.0]);
        assert!(!events.boot_mouse(&[0, 1]));
        assert!(events.boot_mouse(&[1, 255, 3]));
        assert!(events.boot_mouse(&[0, 2, 253, 255]));
        desktop.poll(&events, 0.0);
        assert_eq!(desktop.pointer(), Some([321.0, 240.0]));
        assert_eq!(desktop.pad(Context::Play).mouse_motion, [1.0, 0.0]);
        assert_eq!(desktop.pad(Context::Play).scroll, -1);
        assert!(desktop.pad(Context::Menu).pressed(Button::Select));
        events.key(26, true, None);
        events.mouse(3, 10_000, -10_000, 0);
        desktop.poll(&events, 0.0);
        assert_eq!(desktop.pointer(), Some([640.0, 0.0]));
        events.disconnect_keyboard();
        events.disconnect_mouse();
        desktop.poll(&events, 0.0);
        assert_eq!(desktop.pointer(), None);
        assert_eq!(desktop.pad(Context::Play).direction, [0.0; 2]);
        assert!(!desktop.pad(Context::Play).held(Button::Break));
        assert!(!desktop.pad(Context::Play).held(Button::Place));
    }
}
