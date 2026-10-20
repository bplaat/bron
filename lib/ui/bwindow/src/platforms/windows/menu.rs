/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;

use super::headers::*;
use crate::{Accelerator, Event, KeyCode, MenuBarBuilder, Modifiers};

thread_local! {
    static NATIVE: Cell<(HWND, HANDLE)> = const { Cell::new((null_mut(), null_mut())) };
    static ACTIONS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct Accelerators(HANDLE);

impl Accelerators {
    pub(super) fn new(window: HWND, menu: Option<&MenuBarBuilder>) -> Self {
        let mut actions = Vec::new();
        let entries = menu
            .into_iter()
            .flat_map(MenuBarBuilder::items)
            .filter_map(|item| {
                let accelerator = item.accelerator?;
                assert!(
                    !accelerator.modifiers.contains(Modifiers::META),
                    "Windows shortcuts do not support the Super key; use PRIMARY"
                );
                actions.push(item.action.clone());
                Some(ACCEL {
                    fVirt: 1
                        | if accelerator.modifiers.contains(Modifiers::SHIFT) {
                            4
                        } else {
                            0
                        }
                        | if accelerator.modifiers.contains(Modifiers::CONTROL) {
                            8
                        } else {
                            0
                        }
                        | if accelerator.modifiers.contains(Modifiers::ALT) {
                            16
                        } else {
                            0
                        },
                    key: virtual_key(accelerator),
                    cmd: u16::try_from(actions.len()).expect("too many shortcuts"),
                })
            })
            .collect::<Vec<_>>();
        ACTIONS.with(|stored| *stored.borrow_mut() = actions);
        let table = if entries.is_empty() {
            null_mut()
        } else {
            let table = unsafe { CreateAcceleratorTableW(entries.as_ptr(), entries.len() as i32) };
            assert!(!table.is_null(), "could not create accelerator table");
            table
        };
        NATIVE.set((window, table));
        Self(table)
    }
}

impl Drop for Accelerators {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { DestroyAcceleratorTable(self.0) };
        }
        NATIVE.set((null_mut(), null_mut()));
        ACTIONS.with(|actions| actions.borrow_mut().clear());
    }
}

pub(super) unsafe fn translate(message: *mut MSG) -> bool {
    let (window, table) = NATIVE.get();
    // Modal dialogs have their own keyboard handling.
    if table.is_null() || !super::window::is_app_window(unsafe { GetActiveWindow() }) {
        return false;
    }
    unsafe { TranslateAcceleratorW(window, table, message) != 0 }
}

pub(super) fn activate(id: usize) {
    let action = ACTIONS.with(|actions| actions.borrow().get(id.wrapping_sub(1)).cloned());
    if let Some(action) = action {
        super::event_loop::send_event(Event::Action(action));
    }
}

fn virtual_key(accelerator: Accelerator) -> u16 {
    use KeyCode::*;
    let key = accelerator.key;
    if (KeyA as u16..=KeyZ as u16).contains(&(key as u16)) {
        return b'A' as u16 + key as u16 - KeyA as u16;
    }
    if (Digit0 as u16..=Digit9 as u16).contains(&(key as u16)) {
        return b'0' as u16 + key as u16 - Digit0 as u16;
    }
    if (F1 as u16..=F24 as u16).contains(&(key as u16)) {
        return 0x70 + key as u16 - F1 as u16;
    }
    match key {
        Backspace => 0x08,
        Tab => 0x09,
        Enter => 0x0d,
        Escape => 0x1b,
        Space => 0x20,
        PageUp => 0x21,
        PageDown => 0x22,
        End => 0x23,
        Home => 0x24,
        ArrowLeft => 0x25,
        ArrowUp => 0x26,
        ArrowRight => 0x27,
        ArrowDown => 0x28,
        Delete => 0x2e,
        key => {
            let character = match key {
                Backquote => b'`',
                Backslash => b'\\',
                BracketLeft => b'[',
                BracketRight => b']',
                Comma => b',',
                Equal => b'=',
                Minus => b'-',
                Period => b'.',
                Quote => b'\'',
                Semicolon => b';',
                Slash => b'/',
                _ => unreachable!(),
            };
            let scan = unsafe { VkKeyScanW(character as u16) };
            assert!(
                scan != -1,
                "shortcut key unavailable in current keyboard layout"
            );
            scan as u16 & 0xff
        }
    }
}

pub(super) fn translate_webview(window: HWND, key: u32, lparam: isize, system: bool) -> bool {
    let mut message = MSG {
        hwnd: window,
        message: if system { 0x0104 } else { 0x0100 },
        wParam: key as usize,
        lParam: lparam,
        ..unsafe { std::mem::zeroed() }
    };
    unsafe { translate(&mut message) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virtual_keys_cover_letters_digits_function_and_editing_keys() {
        for (key, expected) in [
            (KeyCode::KeyA, 0x41),
            (KeyCode::KeyZ, 0x5a),
            (KeyCode::Digit0, 0x30),
            (KeyCode::Digit9, 0x39),
            (KeyCode::F1, 0x70),
            (KeyCode::F24, 0x87),
            (KeyCode::Enter, 0x0d),
            (KeyCode::Backspace, 0x08),
        ] {
            assert_eq!(
                virtual_key(Accelerator::new(Modifiers::CONTROL, key)),
                expected
            );
        }
    }
}
