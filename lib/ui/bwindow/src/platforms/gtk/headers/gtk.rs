/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

use super::gdk::GdkRGBA;
use super::glib::GSList;

// MARK: GTK
#[repr(C)]
pub(crate) struct GtkApplication([u8; 0]);
#[repr(C)]
pub(crate) struct GtkWidget([u8; 0]);
#[repr(C)]
pub(crate) struct GtkWindow([u8; 0]);
#[repr(C)]
pub(crate) struct GtkSettings([u8; 0]);
#[repr(C)]
pub(crate) struct GtkSelectionData([u8; 0]);
#[repr(C)]
pub(crate) struct GtkDialog([u8; 0]);
pub(crate) const GTK_WIN_POS_CENTER: i32 = 1;
pub(crate) const GTK_STATE_FLAG_NORMAL: i32 = 0;
pub(crate) const GTK_DIALOG_MODAL: i32 = 1;
pub(crate) const GTK_DIALOG_DESTROY_WITH_PARENT: i32 = 2;
pub(crate) const GTK_MESSAGE_INFO: i32 = 0;
pub(crate) const GTK_MESSAGE_WARNING: i32 = 1;
pub(crate) const GTK_MESSAGE_ERROR: i32 = 3;
pub(crate) const GTK_BUTTONS_NONE: i32 = 0;
unsafe extern "C" {
    #[cfg(test)]
    pub(crate) fn gtk_application_window_get_show_menubar(window: *mut GtkWindow) -> i32;
    #[cfg(test)]
    pub(crate) fn gtk_application_get_menubar(
        application: *mut GtkApplication,
    ) -> *mut super::glib::GMenuModel;
    pub(crate) fn gtk_application_new(
        application_id: *const c_char,
        flags: u32,
    ) -> *mut GtkApplication;
    pub(crate) fn gtk_application_get_active_window(
        application: *mut GtkApplication,
    ) -> *mut GtkWindow;
    pub(crate) fn gtk_application_set_accels_for_action(
        application: *mut GtkApplication,
        action: *const c_char,
        accels: *const *const c_char,
    );
    pub(crate) fn gtk_application_get_windows(
        application: *mut GtkApplication,
    ) -> *mut super::glib::GList;
    #[cfg(test)]
    pub(crate) fn gtk_entry_new() -> *mut GtkWidget;
    #[cfg(test)]
    pub(crate) fn gtk_entry_set_text(entry: *mut GtkWidget, text: *const c_char);
    #[cfg(test)]
    pub(crate) fn gtk_entry_get_text(entry: *mut GtkWidget) -> *const c_char;
    #[cfg(test)]
    pub(crate) fn gtk_window_set_focus(window: *mut GtkWindow, focus: *mut GtkWidget);
    pub(crate) fn gtk_window_close(window: *mut GtkWindow);
    pub(crate) fn gtk_window_get_focus(window: *mut GtkWindow) -> *mut GtkWidget;
    pub(crate) fn gtk_window_iconify(window: *mut GtkWindow);
    pub(crate) fn gtk_window_unmaximize(window: *mut GtkWindow);
    pub(crate) fn gtk_window_unfullscreen(window: *mut GtkWindow);
    pub(crate) fn gtk_window_get_resizable(window: *mut GtkWindow) -> i32;
    pub(crate) fn gtk_widget_get_window(widget: *mut GtkWidget) -> *mut c_void;
    pub(crate) fn gtk_widget_get_action_group(
        widget: *mut GtkWidget,
        prefix: *const c_char,
    ) -> *mut c_void;
    pub(crate) fn gtk_widget_insert_action_group(
        widget: *mut GtkWidget,
        prefix: *const c_char,
        group: *mut c_void,
    );
    pub(crate) fn gtk_editable_select_region(editable: *mut GtkWidget, start: i32, end: i32);
    pub(crate) fn gtk_editable_get_editable(editable: *mut GtkWidget) -> i32;
    pub(crate) fn gtk_text_view_get_editable(view: *mut GtkWidget) -> i32;
    pub(crate) fn gtk_show_about_dialog(parent: *mut GtkWindow, first_property: *const c_char, ...);
    pub(crate) fn gtk_window_present(window: *mut GtkWindow);
    pub(crate) fn gtk_application_set_menubar(
        application: *mut GtkApplication,
        menu: *mut super::glib::GMenuModel,
    );
    pub(crate) fn gtk_application_window_new(application: *mut GtkApplication) -> *mut GtkWindow;
    pub(crate) fn gtk_application_window_set_show_menubar(window: *mut GtkWindow, show: i32);
    pub(crate) fn gtk_accelerator_parse(
        accelerator: *const c_char,
        key: *mut u32,
        modifiers: *mut u32,
    );
    pub(crate) fn gtk_accelerator_name(key: u32, modifiers: u32) -> *mut c_char;
    pub(crate) fn gtk_widget_destroy(widget: *mut GtkWidget);
    pub(crate) fn gtk_widget_get_allocated_width(widget: *mut GtkWidget) -> i32;
    pub(crate) fn gtk_widget_get_allocated_height(widget: *mut GtkWidget) -> i32;
    pub(crate) fn gtk_widget_get_scale_factor(widget: *mut GtkWidget) -> i32;
    pub(crate) fn gtk_window_set_default_icon_name(name: *const c_char);
    pub(crate) fn gtk_widget_set_size_request(widget: *mut GtkWidget, width: i32, height: i32);
    pub(crate) fn gtk_window_fullscreen(window: *mut GtkWindow);
    pub(crate) fn gtk_container_add(container: *mut GtkWidget, widget: *mut GtkWidget);
    pub(crate) fn gtk_window_get_position(window: *mut GtkWindow, x: *mut i32, y: *mut i32);
    pub(crate) fn gtk_window_set_title(window: *mut GtkWindow, title: *const c_char);
    pub(crate) fn gtk_window_set_position(window: *mut GtkWindow, position: i32);
    pub(crate) fn gtk_window_move(window: *mut GtkWindow, x: i32, y: i32);
    pub(crate) fn gtk_window_get_size(window: *mut GtkWindow, width: *mut i32, height: *mut i32);
    pub(crate) fn gtk_window_set_default_size(window: *mut GtkWindow, width: i32, height: i32);
    pub(crate) fn gtk_window_set_resizable(window: *mut GtkWindow, resizable: bool);
    pub(crate) fn gtk_window_is_maximized(window: *mut GtkWindow) -> bool;
    pub(crate) fn gtk_window_maximize(window: *mut GtkWindow);
    pub(crate) fn gtk_widget_show(widget: *mut GtkWidget);
    pub(crate) fn gtk_widget_show_all(window: *mut GtkWidget);
    pub(crate) fn gtk_drag_dest_set(
        widget: *mut GtkWidget,
        flags: i32,
        targets: *const c_void,
        count: i32,
        actions: i32,
    );
    pub(crate) fn gtk_drag_dest_add_uri_targets(widget: *mut GtkWidget);
    pub(crate) fn gtk_settings_get_default() -> *mut GtkSettings;
    pub(crate) fn gtk_widget_override_background_color(
        widget: *mut GtkWidget,
        state: i32,
        color: *const GdkRGBA,
    );
    pub(crate) fn gtk_selection_data_get_uris(
        selection_data: *const GtkSelectionData,
    ) -> *mut *mut c_char;

    // GTK < 3.22

    // GTK 3.22+
}

// MARK: GTK Message Dialog
unsafe extern "C" {
    pub(crate) fn gtk_dialog_run(dialog: *mut GtkDialog) -> i32;
    pub(crate) fn gtk_message_dialog_new(
        parent: *mut GtkWindow,
        flags: i32,
        r#type: i32,
        buttons: i32,
        message_format: *const c_char,
        ...
    ) -> *mut GtkWidget;
    pub(crate) fn gtk_dialog_add_button(
        dialog: *mut GtkDialog,
        button_text: *const c_char,
        response_id: i32,
    ) -> *mut GtkWidget;
    pub(crate) fn gtk_dialog_set_default_response(dialog: *mut GtkDialog, response_id: i32);
}

// MARK: GTK File Chooser
pub(crate) const GTK_FILE_CHOOSER_ACTION_OPEN: i32 = 0;
pub(crate) const GTK_FILE_CHOOSER_ACTION_SAVE: i32 = 1;
pub(crate) const GTK_RESPONSE_ACCEPT: i32 = -3;
pub(crate) const GTK_RESPONSE_CANCEL: i32 = -6;
#[repr(C)]
pub(crate) struct GtkFileFilter([u8; 0]);

// GTK 3.20+
#[repr(C)]
pub(crate) struct GtkFileChooserNative([u8; 0]);
#[repr(C)]
pub(crate) struct GtkNativeDialog([u8; 0]);

unsafe extern "C" {
    pub(crate) fn gtk_file_chooser_set_select_multiple(chooser: *mut c_void, select_multiple: bool);
    pub(crate) fn gtk_file_chooser_set_current_folder(
        chooser: *mut c_void,
        folder: *const c_char,
    ) -> bool;
    pub(crate) fn gtk_file_chooser_set_current_name(chooser: *mut c_void, name: *const c_char);
    pub(crate) fn gtk_file_chooser_get_filename(chooser: *mut c_void) -> *mut c_char;
    pub(crate) fn gtk_file_chooser_get_filenames(chooser: *mut c_void) -> *mut GSList;
    pub(crate) fn gtk_file_filter_new() -> *mut GtkFileFilter;
    pub(crate) fn gtk_file_filter_set_name(filter: *mut GtkFileFilter, name: *const c_char);
    pub(crate) fn gtk_file_filter_add_pattern(filter: *mut GtkFileFilter, pattern: *const c_char);
    pub(crate) fn gtk_file_chooser_add_filter(chooser: *mut c_void, filter: *mut GtkFileFilter);

    // GTK < 3.20
    pub(crate) fn gtk_file_chooser_dialog_new(
        title: *const c_char,
        parent: *mut GtkWindow,
        action: i32,
        first_button_text: *const c_char,
        ...
    ) -> *mut GtkWidget;

    // GTK 3.20+
    pub(crate) fn gtk_file_chooser_native_new(
        title: *const c_char,
        parent: *mut GtkWindow,
        action: i32,
        accept_label: *const c_char,
        cancel_label: *const c_char,
    ) -> *mut GtkFileChooserNative;
    pub(crate) fn gtk_native_dialog_run(dialog: *mut GtkNativeDialog) -> i32;
}
