/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString, c_void};
use std::ptr::{null, null_mut};

use super::headers::*;
use crate::{Accelerator, Event, KeyCode, MenuBarBuilder, MenuBuilderEntry, Modifiers};

pub(super) unsafe fn register(application: *mut GtkApplication, menu: Option<&MenuBarBuilder>) {
    let Some(menu) = menu else { return };
    unsafe {
        let menubar = g_menu_new();
        let mut index = 0;
        for group in &menu.menus {
            let submenu = g_menu_new();
            let mut section = g_menu_new();
            for entry in &group.entries {
                match entry {
                    MenuBuilderEntry::Separator => {
                        g_menu_append_section(submenu, null(), section.cast());
                        g_object_unref(section.cast());
                        section = g_menu_new();
                    }
                    MenuBuilderEntry::Item(item) => {
                        let name = CString::new(format!("command{index}")).expect("invalid action");
                        let detailed =
                            CString::new(format!("app.command{index}")).expect("invalid action");
                        index += 1;
                        let action = g_simple_action_new(name.as_ptr(), null());
                        let data = Box::into_raw(Box::new(item.action.clone())).cast::<c_void>();
                        g_signal_connect_data(
                            action.cast(),
                            c"activate".as_ptr(),
                            activate as *const c_void,
                            data,
                            release_action as *const c_void,
                            G_CONNECT_DEFAULT,
                        );
                        g_action_map_add_action(application.cast(), action);
                        g_object_unref(action.cast());
                        let title =
                            CString::new(item.title.as_str()).expect("menu title contains NUL");
                        let menu_item = g_menu_item_new(title.as_ptr(), detailed.as_ptr());
                        if let Some(accelerator) = item.accelerator {
                            let key = native_accelerator(accelerator);
                            g_menu_item_set_attribute(
                                menu_item,
                                c"accel".as_ptr(),
                                c"s".as_ptr(),
                                key.as_ptr(),
                            );
                            gtk_application_set_accels_for_action(
                                application,
                                detailed.as_ptr(),
                                [key.as_ptr(), null()].as_ptr(),
                            );
                        }
                        g_menu_append_item(section, menu_item);
                        g_object_unref(menu_item.cast());
                    }
                }
            }
            g_menu_append_section(submenu, null(), section.cast());
            g_object_unref(section.cast());
            let title = CString::new(group.title.as_str()).expect("menu title contains NUL");
            g_menu_append_submenu(menubar, title.as_ptr(), submenu.cast());
            g_object_unref(submenu.cast());
        }
        gtk_application_set_menubar(application, menubar.cast());
        g_object_unref(menubar.cast());
    }
}

extern "C" fn activate(_: *mut GSimpleAction, _: *mut GVariant, data: *const String) {
    let action = unsafe { &*data }.clone();
    super::event_loop::send_event(Event::Action(action));
}

extern "C" fn release_action(data: *mut c_void, _: *mut c_void) {
    drop(unsafe { Box::from_raw(data.cast::<String>()) });
}

fn native_accelerator(accelerator: Accelerator) -> CString {
    let name = CString::new(accelerator_name(accelerator)).expect("invalid accelerator");
    if !accelerator.modifiers.contains(Modifiers::SHIFT) {
        return name;
    }
    unsafe {
        let (mut key, mut modifiers) = (0, 0);
        gtk_accelerator_parse(name.as_ptr(), &mut key, &mut modifiers);
        if !char::from_u32(key).is_some_and(|c| c.is_ascii_punctuation() || c.is_ascii_digit()) {
            return name;
        }
        let keymap = gdk_keymap_get_for_display(gdk_display_get_default());
        let (mut keys, mut count) = (null_mut(), 0);
        if gdk_keymap_get_entries_for_keyval(keymap, key, &mut keys, &mut count) == 0 {
            return name;
        }
        let mut shifted = None;
        for entry in std::slice::from_raw_parts(keys, count as usize) {
            if entry.level == 0 {
                let (mut translated, mut group, mut level, mut consumed) = (0, 0, 0, 0);
                if gdk_keymap_translate_keyboard_state(
                    keymap,
                    entry.keycode,
                    1,
                    entry.group,
                    &mut translated,
                    &mut group,
                    &mut level,
                    &mut consumed,
                ) != 0
                    && consumed & 1 != 0
                {
                    shifted = Some(translated);
                    break;
                }
            }
        }
        g_free(keys.cast());
        if let Some(key) = shifted {
            // GTK matches shifted punctuation by its resulting keysym (e.g. question),
            // whereas AppKit and Win32 keep the base key plus Shift.
            let native = gtk_accelerator_name(key, modifiers & !1);
            let name = CStr::from_ptr(native).to_owned();
            g_free(native.cast());
            return name;
        }
    }
    name
}

fn accelerator_name(accelerator: Accelerator) -> String {
    let mut name = String::new();
    for (modifier, tag) in [
        (Modifiers::CONTROL, "<Control>"),
        (Modifiers::META, "<Super>"),
        (Modifiers::ALT, "<Alt>"),
        (Modifiers::SHIFT, "<Shift>"),
    ] {
        if accelerator.modifiers.contains(modifier) {
            name.push_str(tag);
        }
    }
    let key = match accelerator.key {
        KeyCode::Backquote => "grave",
        KeyCode::Backslash => "backslash",
        KeyCode::BracketLeft => "bracketleft",
        KeyCode::BracketRight => "bracketright",
        KeyCode::Comma => "comma",
        KeyCode::Equal => "equal",
        KeyCode::Minus => "minus",
        KeyCode::Period => "period",
        KeyCode::Quote => "apostrophe",
        KeyCode::Semicolon => "semicolon",
        KeyCode::Slash => "slash",
        KeyCode::Space => "space",
        KeyCode::Tab => "Tab",
        KeyCode::Backspace => "BackSpace",
        KeyCode::Enter => "Return",
        KeyCode::ArrowUp => "Up",
        KeyCode::ArrowDown => "Down",
        KeyCode::ArrowLeft => "Left",
        KeyCode::ArrowRight => "Right",
        KeyCode::PageUp => "Page_Up",
        KeyCode::PageDown => "Page_Down",
        key => {
            let key = format!("{key:?}");
            name.push_str(
                key.strip_prefix("Key")
                    .or_else(|| key.strip_prefix("Digit"))
                    .unwrap_or(&key),
            );
            return name;
        }
    };
    name.push_str(key);
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accelerator_syntax_handles_modifiers_and_named_keys() {
        assert_eq!(
            accelerator_name(Accelerator::new(
                Modifiers::CONTROL | Modifiers::SHIFT,
                KeyCode::KeyS
            )),
            "<Control><Shift>S"
        );
        assert_eq!(
            accelerator_name(Accelerator::new(Modifiers::CONTROL, KeyCode::Enter)),
            "<Control>Return"
        );
        assert_eq!(
            accelerator_name(Accelerator::new(Modifiers::ALT, KeyCode::Backspace)),
            "<Alt>BackSpace"
        );
        assert_eq!(
            accelerator_name(Accelerator::new(Modifiers::NONE, KeyCode::F12)),
            "F12"
        );
        assert_eq!(
            accelerator_name(Accelerator::new(Modifiers::CONTROL, KeyCode::Digit1)),
            "<Control>1"
        );
    }
}
