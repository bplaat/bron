/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::ffi::{CStr, CString, c_void};
use std::ptr::{null, null_mut};

use super::headers::*;
use crate::{
    Accelerator, KeyCode, MenuBarBuilder, MenuBuilder, MenuBuilderEntry, MenuItem, Modifiers,
};

type DefaultEntry = Option<(&'static str, &'static CStr, Option<Accelerator>)>;

const fn control(key: KeyCode) -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::CONTROL, key))
}

const DEFAULT_MENUS: &[(&str, &[DefaultEntry])] = {
    use KeyCode::*;
    &[
        (
            "File",
            &[
                Some(("Close Window", c"bwindow-close", control(KeyW))),
                Some(("Quit", c"bwindow-quit", control(KeyQ))),
            ],
        ),
        (
            "Edit",
            &[
                Some(("Undo", c"bwindow-undo", control(KeyZ))),
                Some((
                    "Redo",
                    c"bwindow-redo",
                    Some(Accelerator::new(
                        Modifiers(Modifiers::CONTROL.0 | Modifiers::SHIFT.0),
                        KeyZ,
                    )),
                )),
                None,
                Some(("Cut", c"bwindow-cut", control(KeyX))),
                Some(("Copy", c"bwindow-copy", control(KeyC))),
                Some(("Paste", c"bwindow-paste", control(KeyV))),
                Some(("Select All", c"bwindow-select-all", control(KeyA))),
            ],
        ),
        (
            "View",
            &[Some((
                "Fullscreen",
                c"bwindow-fullscreen",
                Some(Accelerator::new(Modifiers::NONE, F11)),
            ))],
        ),
        (
            "Window",
            &[
                Some(("Minimize", c"bwindow-minimize", None)),
                Some(("Maximize", c"bwindow-maximize", None)),
            ],
        ),
        ("Help", &[Some(("About", c"bwindow-about", None))]),
    ]
};

fn menus(custom: Option<MenuBarBuilder>) -> MenuBarBuilder {
    let mut menus = MenuBarBuilder::new();
    for (title, entries) in DEFAULT_MENUS {
        let mut menu = MenuBuilder::new(*title);
        for entry in *entries {
            menu.entries.push(match entry {
                Some((title, command, accelerator)) => MenuBuilderEntry::Item(MenuItem {
                    title: (*title).into(),
                    action: command.to_str().expect("ASCII action").into(),
                    accelerator: *accelerator,
                }),
                None => MenuBuilderEntry::Separator,
            });
        }
        menus.menus.push(menu);
    }
    for group in custom.into_iter().flat_map(|menu| menu.menus) {
        let index = menus
            .menus
            .iter()
            .position(|menu| menu.title == group.title)
            .unwrap_or_else(|| {
                let index = menus
                    .menus
                    .iter()
                    .position(|menu| menu.title == "Window")
                    .unwrap_or(menus.menus.len());
                menus.menus.insert(index, MenuBuilder::new(group.title));
                index
            });
        let mut insertion = 0;
        for entry in group.entries {
            if let MenuBuilderEntry::Item(item) = &entry
                && let Some(accelerator) = item.accelerator
            {
                for entry in menus.menus.iter_mut().flat_map(|menu| &mut menu.entries) {
                    if let MenuBuilderEntry::Item(item) = entry
                        && item.accelerator == Some(accelerator)
                    {
                        item.accelerator = None;
                    }
                }
            }
            let entries = &mut menus.menus[index].entries;
            let existing = entries.iter().position(|existing| matches!((&entry, existing),
                (MenuBuilderEntry::Item(item), MenuBuilderEntry::Item(existing)) if item.title == existing.title));
            if let Some(existing) = existing {
                entries[existing] = entry;
            } else {
                entries.insert(insertion, entry);
                insertion += 1;
            }
        }
    }
    menus
}

pub(super) unsafe fn register(
    application: *mut GtkApplication,
    mut custom: Option<MenuBarBuilder>,
) {
    let mut actions = HashMap::new();
    for entry in custom
        .iter_mut()
        .flat_map(|bar| &mut bar.menus)
        .flat_map(|menu| &mut menu.entries)
    {
        if let MenuBuilderEntry::Item(item) = entry {
            let name = format!("command{}", actions.len());
            actions.insert(name.clone(), std::mem::replace(&mut item.action, name));
        }
    }
    unsafe {
        let menubar = g_menu_new();
        for group in menus(custom).menus {
            let submenu = g_menu_new();
            let mut section = g_menu_new();
            for entry in group.entries {
                let MenuBuilderEntry::Item(item) = entry else {
                    if g_menu_model_get_n_items(section.cast()) > 0 {
                        g_menu_append_section(submenu, null(), section.cast());
                        g_object_unref(section.cast());
                        section = g_menu_new();
                    }
                    continue;
                };
                let custom = actions.remove(&item.action);
                let name = CString::new(item.action).expect("invalid action");
                let detailed =
                    CString::new(format!("app.{}", name.to_str().expect("ASCII action")))
                        .expect("invalid action");
                let toggle = matches!(name.to_bytes(), b"bwindow-fullscreen" | b"bwindow-maximize");
                let action = if toggle {
                    g_simple_action_new_stateful(name.as_ptr(), null(), g_variant_new_boolean(0))
                } else {
                    g_simple_action_new(name.as_ptr(), null())
                };
                if let Some(custom) = custom {
                    g_signal_connect_data(
                        action.cast(),
                        c"activate".as_ptr(),
                        activate_custom as *const c_void,
                        Box::into_raw(Box::new(custom)).cast(),
                        release_action as *const c_void,
                        G_CONNECT_DEFAULT,
                    );
                } else {
                    g_signal_connect_data(
                        action.cast(),
                        if toggle {
                            c"change-state".as_ptr()
                        } else {
                            c"activate".as_ptr()
                        },
                        activate as *const c_void,
                        null(),
                        null(),
                        G_CONNECT_DEFAULT,
                    );
                }
                g_action_map_add_action(application.cast(), action);
                g_object_unref(action.cast());
                let title = CString::new(item.title).expect("menu title contains NUL");
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
            if g_menu_model_get_n_items(section.cast()) > 0 {
                g_menu_append_section(submenu, null(), section.cast());
            }
            g_object_unref(section.cast());
            let title = CString::new(group.title).expect("menu title contains NUL");
            g_menu_append_submenu(menubar, title.as_ptr(), submenu.cast());
            g_object_unref(submenu.cast());
        }
        gtk_application_set_menubar(application, menubar.cast());
        g_object_unref(menubar.cast());
        g_signal_connect_data(
            application.cast(),
            c"notify::active-window".as_ptr(),
            actions_changed as *const c_void,
            null(),
            null(),
            G_CONNECT_DEFAULT,
        );
    }
    update_actions();
}

extern "C" fn release_action(data: *mut c_void, _: *mut c_void) {
    drop(unsafe { Box::from_raw(data.cast::<String>()) });
}

extern "C" fn activate_custom(_: *mut GSimpleAction, _: *mut GVariant, _data: *const String) {
    #[cfg(feature = "menu")]
    super::event_loop::send_event(crate::Event::Action(unsafe { &*_data }.clone()));
}

extern "C" fn activate(action: *mut GSimpleAction, state: *mut GVariant, _: *mut c_void) {
    let name = unsafe { CStr::from_ptr(g_action_get_name(action)) };
    let application = super::event_loop::application();
    let window = unsafe { gtk_application_get_active_window(application) };
    unsafe {
        if !window.is_null() {
            g_object_ref(window.cast());
        }
        match name.to_bytes() {
            b"bwindow-quit" => {
                let mut entry = gtk_application_get_windows(application);
                let mut windows = Vec::new();
                while !entry.is_null() {
                    windows.push(g_object_ref((*entry).data.cast()).cast::<GtkWindow>());
                    entry = (*entry).next;
                }
                for window in windows {
                    gtk_window_close(window);
                    g_object_unref(window.cast());
                }
            }
            b"bwindow-about" => gtk_show_about_dialog(
                window,
                c"program-name".as_ptr(),
                g_get_application_name(),
                c"destroy-with-parent".as_ptr(),
                1i32,
                null::<c_void>(),
            ),
            _ if window.is_null() => {}
            b"bwindow-close" => gtk_window_close(window),
            b"bwindow-minimize" => gtk_window_iconify(window),
            b"bwindow-maximize" => {
                if g_variant_get_boolean(state) != 0 {
                    gtk_window_maximize(window);
                } else {
                    gtk_window_unmaximize(window);
                }
            }
            b"bwindow-fullscreen" => {
                if g_variant_get_boolean(state) != 0 {
                    gtk_window_fullscreen(window);
                } else {
                    gtk_window_unfullscreen(window);
                }
            }
            _ => {
                if let Some(command) = edit_command(name) {
                    edit(window, command);
                }
            }
        }
        if !window.is_null() {
            g_object_unref(window.cast());
        }
    }
    update_actions();
}

const fn edit_command(name: &CStr) -> Option<&'static CStr> {
    Some(match name.to_bytes() {
        b"bwindow-undo" => c"Undo",
        b"bwindow-redo" => c"Redo",
        b"bwindow-cut" => c"Cut",
        b"bwindow-copy" => c"Copy",
        b"bwindow-paste" => c"Paste",
        b"bwindow-select-all" => c"SelectAll",
        _ => return None,
    })
}

fn is_type(widget: *mut GtkWidget, name: &CStr) -> bool {
    if widget.is_null() {
        return false;
    }
    let type_ = unsafe { g_type_from_name(name.as_ptr()) };
    type_ != 0 && unsafe { g_type_check_instance_is_a(widget.cast(), type_) != 0 }
}

fn edit_group(focus: *mut GtkWidget) -> *mut c_void {
    if focus.is_null() {
        return null_mut();
    }
    unsafe {
        let group = gtk_widget_get_action_group(focus, c"bwindow-edit".as_ptr());
        if !group.is_null()
            && g_object_get_data(group.cast(), c"bwindow-menu-connected".as_ptr()).is_null()
        {
            extern "C" fn changed(
                _: *mut c_void,
                _: *const std::ffi::c_char,
                _: i32,
                _: *mut c_void,
            ) {
                update_actions();
            }
            g_object_set_data(
                group.cast(),
                c"bwindow-menu-connected".as_ptr(),
                std::ptr::dangling_mut::<c_void>(),
            );
            g_signal_connect_data(
                group.cast(),
                c"action-enabled-changed".as_ptr(),
                changed as *const c_void,
                null(),
                null(),
                G_CONNECT_DEFAULT,
            );
        }
        group
    }
}

fn can_edit(window: *mut GtkWindow, command: &CStr) -> bool {
    if window.is_null() {
        return false;
    }
    let focus = unsafe { gtk_window_get_focus(window) };
    let editable = is_type(focus, c"GtkEditable");
    let text_view = is_type(focus, c"GtkTextView");
    match command.to_bytes() {
        b"Copy" | b"SelectAll" if editable || text_view => return true,
        b"Cut" | b"Paste" if editable => {
            return unsafe { gtk_editable_get_editable(focus) != 0 };
        }
        b"Cut" | b"Paste" if text_view => {
            return unsafe { gtk_text_view_get_editable(focus) != 0 };
        }
        _ => {}
    }
    let group = edit_group(focus);
    !group.is_null() && unsafe { g_action_group_get_action_enabled(group, command.as_ptr()) != 0 }
}

pub(super) fn edit(window: *mut GtkWindow, command: &CStr) {
    if !can_edit(window, command) {
        return;
    }
    let focus = unsafe { gtk_window_get_focus(window) };
    unsafe {
        if !focus.is_null() {
            g_object_ref(focus.cast());
        }
        if !matches!(command.to_bytes(), b"Undo" | b"Redo")
            && (is_type(focus, c"GtkEditable") || is_type(focus, c"GtkTextView"))
        {
            match command.to_bytes() {
                b"SelectAll" if is_type(focus, c"GtkEditable") => {
                    gtk_editable_select_region(focus, 0, -1)
                }
                b"SelectAll" => g_signal_emit_by_name(focus.cast(), c"select-all".as_ptr(), 1i32),
                b"Cut" => g_signal_emit_by_name(focus.cast(), c"cut-clipboard".as_ptr()),
                b"Copy" => g_signal_emit_by_name(focus.cast(), c"copy-clipboard".as_ptr()),
                b"Paste" => g_signal_emit_by_name(focus.cast(), c"paste-clipboard".as_ptr()),
                _ => {}
            }
        } else {
            let group = edit_group(focus);
            if !group.is_null() {
                g_action_group_activate_action(group, command.as_ptr(), null_mut());
            }
        }
        if !focus.is_null() {
            g_object_unref(focus.cast());
        }
    }
}

fn is_fullscreen(window: *mut GtkWindow) -> bool {
    if window.is_null() {
        return false;
    }
    let native = unsafe { gtk_widget_get_window(window.cast()) };
    !native.is_null() && unsafe { gdk_window_get_state(native) & GDK_WINDOW_STATE_FULLSCREEN != 0 }
}

pub(crate) fn update_actions() {
    let application = super::event_loop::application();
    if application.is_null() {
        return;
    }
    let window = unsafe { gtk_application_get_active_window(application) };
    for (_, name, _) in DEFAULT_MENUS
        .iter()
        .flat_map(|(_, entries)| *entries)
        .flatten()
    {
        let action = unsafe { g_action_map_lookup_action(application.cast(), name.as_ptr()) };
        if action.is_null() {
            continue;
        }
        let enabled = match name.to_bytes() {
            b"bwindow-about" => true,
            b"bwindow-quit" => unsafe { !gtk_application_get_windows(application).is_null() },
            _ if let Some(command) = edit_command(name) => can_edit(window, command),
            b"bwindow-maximize" => {
                !window.is_null() && unsafe { gtk_window_get_resizable(window) != 0 }
            }
            _ => !window.is_null(),
        };
        unsafe {
            g_simple_action_set_enabled(action, enabled as i32);
            if matches!(name.to_bytes(), b"bwindow-fullscreen" | b"bwindow-maximize") {
                let state = if name.to_bytes() == b"bwindow-fullscreen" {
                    is_fullscreen(window)
                } else {
                    !window.is_null() && gtk_window_is_maximized(window)
                };
                g_simple_action_set_state(action, g_variant_new_boolean(state as i32));
            }
        }
    }
}

extern "C" fn actions_changed(_: *mut GObject, _: *mut c_void, _: *mut c_void) {
    update_actions();
}
extern "C" fn state_changed(_: *mut GtkWindow, _: *mut c_void, _: *mut c_void) -> i32 {
    update_actions();
    0
}

pub(super) unsafe fn connect_window(window: *mut GtkWindow) {
    unsafe {
        for signal in [c"set-focus", c"notify::resizable"] {
            g_signal_connect_data(
                window.cast(),
                signal.as_ptr(),
                actions_changed as *const c_void,
                null(),
                null(),
                G_CONNECT_AFTER,
            );
        }
        g_signal_connect_data(
            window.cast(),
            c"window-state-event".as_ptr(),
            state_changed as *const c_void,
            null(),
            null(),
            G_CONNECT_AFTER,
        );
    }
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
    fn custom_items_replace_defaults_and_claim_shortcuts_once() {
        use crate::{MenuBuilder, MenuItem};
        let chord = Accelerator::new(Modifiers::CONTROL, KeyCode::KeyQ);
        let menus = menus(Some(
            MenuBarBuilder::new()
                .menu(
                    MenuBuilder::new("Edit").item(
                        MenuItem::new("Copy", "copy.document")
                            .accelerator(Accelerator::new(Modifiers::CONTROL, KeyCode::KeyC)),
                    ),
                )
                .menu(
                    MenuBuilder::new("Tools")
                        .item(MenuItem::new("Quick Action", "quick").accelerator(chord)),
                ),
        ));
        assert_eq!(
            menus
                .menus
                .iter()
                .map(|menu| menu.title.as_str())
                .collect::<Vec<_>>(),
            ["File", "Edit", "View", "Tools", "Window", "Help"]
        );
        let items = menus.items().collect::<Vec<_>>();
        assert_eq!(items.iter().filter(|item| item.title == "Copy").count(), 1);
        assert!(items.iter().any(|item| item.action == "copy.document"));
        assert!(!items.iter().any(|item| item.action == "bwindow-copy"));
        let edit_items = menus.menus[1]
            .entries
            .iter()
            .filter_map(|entry| match entry {
                MenuBuilderEntry::Item(item) => Some(item.title.as_str()),
                MenuBuilderEntry::Separator => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            edit_items,
            ["Undo", "Redo", "Cut", "Copy", "Paste", "Select All"]
        );
        assert!(
            items
                .iter()
                .any(|item| item.action == "bwindow-quit" && item.accelerator.is_none())
        );
        assert_eq!(
            items
                .iter()
                .filter(|item| item.accelerator == Some(chord))
                .count(),
            1
        );
    }

    #[test]
    fn repeated_custom_groups_do_not_duplicate_items_or_shortcuts() {
        use crate::{MenuBuilder, MenuItem};
        let chord = Accelerator::new(Modifiers::CONTROL, KeyCode::KeyN);
        let menus = menus(Some(
            MenuBarBuilder::new()
                .menu(
                    MenuBuilder::new("File").item(MenuItem::new("New", "first").accelerator(chord)),
                )
                .menu(
                    MenuBuilder::new("File")
                        .item(MenuItem::new("New", "replacement").accelerator(chord)),
                ),
        ));
        let items = menus.items().collect::<Vec<_>>();
        assert_eq!(items.iter().filter(|item| item.title == "New").count(), 1);
        assert!(!items.iter().any(|item| item.action == "first"));
        assert_eq!(
            items
                .iter()
                .filter(|item| item.accelerator == Some(chord))
                .count(),
            1
        );
        assert!(items.iter().any(|item| item.action == "bwindow-close"));
    }

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
