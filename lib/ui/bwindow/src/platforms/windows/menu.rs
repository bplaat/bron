/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ptr::null_mut;

use super::headers::*;
use crate::{Accelerator, KeyCode, MenuBarBuilder, Modifiers};

thread_local! {
    static NATIVE: Cell<(HWND, HANDLE)> = const { Cell::new((null_mut(), null_mut())) };
    static ACTIONS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static ENTRIES: RefCell<Vec<ACCEL>> = const { RefCell::new(Vec::new()) };
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
        ENTRIES.with(|stored| *stored.borrow_mut() = entries);
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
        ENTRIES.with(|entries| entries.borrow_mut().clear());
    }
}

pub(super) unsafe fn translate(message: *mut MSG) -> bool {
    let (window, table) = NATIVE.get();
    // Modal dialogs have their own keyboard handling.
    if table.is_null() || !super::window::is_app_window(unsafe { GetActiveWindow() }) {
        return false;
    }
    let message_ref = unsafe { &*message };
    if !matches!(message_ref.message, 0x0100 | 0x0104) {
        return false;
    }
    let modifiers = [(0x10, 4), (0x11, 8), (0x12, 16)]
        .into_iter()
        .fold(0, |flags, (key, flag)| {
            flags
                | if unsafe { GetKeyState(key) } < 0 {
                    flag
                } else {
                    0
                }
        });
    // Disabled commands must leave the key for the focused control, rather than consume it
    // in TranslateAcceleratorW and discard the command afterward.
    if !shortcut_enabled(message_ref.wParam, modifiers) {
        return false;
    }
    unsafe { TranslateAcceleratorW(window, table, message) != 0 }
}

fn shortcut_enabled(key: usize, modifiers: u8) -> bool {
    ENTRIES.with_borrow(|entries| {
        ACTIONS.with_borrow(|actions| {
            entries
                .iter()
                .find(|entry| usize::from(entry.key) == key && entry.fVirt & 28 == modifiers)
                .and_then(|entry| actions.get(usize::from(entry.cmd) - 1))
                .is_some_and(|action| crate::menu::is_menu_item_enabled(action))
        })
    })
}

pub(super) fn activate(id: usize) {
    let action = ACTIONS.with(|actions| actions.borrow().get(id.wrapping_sub(1)).cloned());
    if let Some(event) = action.and_then(crate::menu::action_event) {
        super::event_loop::send_event(event);
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
    fn disabled_shortcuts_leave_keys_for_native_controls() {
        use crate::{MenuBuilder, MenuItem};

        let menu = MenuBarBuilder::new().menu(
            MenuBuilder::new("File")
                .item(
                    MenuItem::new("Copy", "test.copy")
                        .accelerator(Accelerator::new(Modifiers::CONTROL, KeyCode::KeyC)),
                )
                .item(
                    MenuItem::new("Save As", "test.save_as").accelerator(Accelerator::new(
                        Modifiers::CONTROL | Modifiers::SHIFT,
                        KeyCode::KeyS,
                    )),
                ),
        );
        let _accelerators = Accelerators::new(null_mut(), Some(&menu));
        assert!(shortcut_enabled(b'C' as usize, 8));
        assert!(shortcut_enabled(b'S' as usize, 12));
        assert!(!shortcut_enabled(b'S' as usize, 8));
        crate::set_menu_item_enabled("test.copy", false);
        assert!(!shortcut_enabled(b'C' as usize, 8));
        assert!(shortcut_enabled(b'S' as usize, 12));
        crate::set_menu_item_enabled("test.copy", true);
        assert!(shortcut_enabled(b'C' as usize, 8));
    }

    #[test]
    fn windows_translates_save_as_and_passes_disabled_shortcuts_through() {
        use crate::{MenuBuilder, MenuItem, WindowBuilder, WindowInterface};

        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetKeyboardState(state: *mut u8) -> i32;
            fn SetKeyboardState(state: *const u8) -> i32;
            fn SetActiveWindow(window: HWND) -> HWND;
        }

        let builder = WindowBuilder::new().title("Shortcut translation test");
        let host = crate::content::ContentHost::new(builder.window_id, None, false);
        let mut window = super::super::window::PlatformWindow::new(&builder, host);
        let hwnd = window.0.hwnd;
        let menu = MenuBarBuilder::new().menu(MenuBuilder::new("File").item(
            MenuItem::new("Save As", "test.native_save_as").accelerator(Accelerator::new(
                Modifiers::CONTROL | Modifiers::SHIFT,
                KeyCode::KeyS,
            )),
        ));
        let _accelerators = Accelerators::new(hwnd, Some(&menu));
        let mut previous = [0u8; 256];
        let mut pressed = [0u8; 256];
        pressed[0x10] = 0x80;
        pressed[0x11] = 0x80;
        unsafe {
            GetKeyboardState(previous.as_mut_ptr());
            SetKeyboardState(pressed.as_ptr());
            SetActiveWindow(hwnd);
        }
        let mut message = MSG {
            hwnd,
            message: 0x0100,
            wParam: b'S' as usize,
            ..unsafe { std::mem::zeroed() }
        };
        let enabled = unsafe { translate(&mut message) };
        crate::set_menu_item_enabled("test.native_save_as", false);
        let disabled = unsafe { translate(&mut message) };
        crate::set_menu_item_enabled("test.native_save_as", true);
        let reenabled = unsafe { translate(&mut message) };
        unsafe { SetKeyboardState(previous.as_ptr()) };
        window.close();
        crate::dispatch::shutdown(&crate::mailbox::Mailbox::new());
        assert!(enabled, "Ctrl+Shift+S must work without a visible menu");
        assert!(
            !disabled,
            "disabled shortcuts must reach the focused control"
        );
        assert!(reenabled, "reenabling an action must restore its shortcut");
    }

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
