/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

use super::gdk::{GdkRGBA, GdkScreen};
use super::glib::{GError, GSList};

// MARK: GTK
#[repr(C)]
pub struct GtkApplication([u8; 0]);
#[repr(C)]
pub struct GtkWidget([u8; 0]);
#[repr(C)]
pub struct GtkWindow([u8; 0]);
#[repr(C)]
pub struct GtkSettings([u8; 0]);
#[repr(C)]
pub struct GtkSelectionData([u8; 0]);
#[repr(C)]
pub struct GtkDialog([u8; 0]);
pub const GTK_WINDOW_TOPLEVEL: i32 = 0;
pub const GTK_WIN_POS_CENTER: i32 = 1;
pub const GTK_STATE_FLAG_NORMAL: i32 = 0;
pub const GTK_DIALOG_MODAL: i32 = 1;
pub const GTK_DIALOG_DESTROY_WITH_PARENT: i32 = 2;
pub const GTK_MESSAGE_INFO: i32 = 0;
pub const GTK_MESSAGE_WARNING: i32 = 1;
pub const GTK_MESSAGE_ERROR: i32 = 3;
pub const GTK_BUTTONS_NONE: i32 = 0;
unsafe extern "C" {
    pub fn gtk_application_new(application_id: *const c_char, flags: u32) -> *mut GtkApplication;
    pub fn gtk_application_get_active_window(application: *mut GtkApplication) -> *mut GtkWindow;
    pub fn gtk_window_present(window: *mut GtkWindow);
    pub fn gtk_application_window_new(application: *mut GtkApplication) -> *mut GtkWindow;
    pub fn gtk_widget_destroy(widget: *mut GtkWidget);
    pub fn gtk_widget_get_allocated_width(widget: *mut GtkWidget) -> i32;
    pub fn gtk_widget_get_allocated_height(widget: *mut GtkWidget) -> i32;
    pub fn gtk_widget_get_scale_factor(widget: *mut GtkWidget) -> i32;
    pub fn gtk_window_new(r#type: i32) -> *mut GtkWindow;
    pub fn gtk_window_set_default_icon_name(name: *const c_char);
    pub fn gtk_widget_set_size_request(widget: *mut GtkWidget, width: i32, height: i32);
    pub fn gtk_window_fullscreen(window: *mut GtkWindow);
    pub fn gtk_container_add(container: *mut GtkWidget, widget: *mut GtkWidget);
    pub fn gtk_window_get_position(window: *mut GtkWindow, x: *mut i32, y: *mut i32);
    pub fn gtk_window_set_title(window: *mut GtkWindow, title: *const c_char);
    pub fn gtk_window_set_position(window: *mut GtkWindow, position: i32);
    pub fn gtk_window_move(window: *mut GtkWindow, x: i32, y: i32);
    pub fn gtk_window_get_size(window: *mut GtkWindow, width: *mut i32, height: *mut i32);
    pub fn gtk_window_set_default_size(window: *mut GtkWindow, width: i32, height: i32);
    pub fn gtk_window_set_resizable(window: *mut GtkWindow, resizable: bool);
    pub fn gtk_window_is_maximized(window: *mut GtkWindow) -> bool;
    pub fn gtk_window_maximize(window: *mut GtkWindow);
    pub fn gtk_widget_show(widget: *mut GtkWidget);
    pub fn gtk_widget_hide(widget: *mut GtkWidget);
    pub fn gtk_widget_show_all(window: *mut GtkWidget);
    pub fn gtk_drag_dest_set(
        widget: *mut GtkWidget,
        flags: i32,
        targets: *const c_void,
        count: i32,
        actions: i32,
    );
    pub fn gtk_drag_dest_add_uri_targets(widget: *mut GtkWidget);
    pub fn gtk_settings_get_default() -> *mut GtkSettings;
    pub fn gtk_widget_override_background_color(
        widget: *mut GtkWidget,
        state: i32,
        color: *const GdkRGBA,
    );
    pub fn gtk_drag_finish(
        context: *mut super::gdk::GdkDragContext,
        success: i32,
        delete: i32,
        time: u32,
    );
    pub fn gtk_selection_data_get_uris(selection_data: *const GtkSelectionData)
    -> *mut *mut c_char;

    // GTK < 3.22
    pub fn gtk_show_uri(
        screen: *mut GdkScreen,
        uri: *const c_char,
        timestamp: u32,
        error: *mut *mut GError,
    ) -> bool;

    // GTK 3.22+
    pub fn gtk_show_uri_on_window(
        parent: *mut GtkWindow,
        uri: *const c_char,
        timestamp: u32,
        error: *mut *mut GError,
    );
}

// MARK: GTK Message Dialog
unsafe extern "C" {
    pub fn gtk_dialog_run(dialog: *mut GtkDialog) -> i32;
    pub fn gtk_message_dialog_new(
        parent: *mut GtkWindow,
        flags: i32,
        r#type: i32,
        buttons: i32,
        message_format: *const c_char,
        ...
    ) -> *mut GtkWidget;
    pub fn gtk_dialog_add_button(
        dialog: *mut GtkDialog,
        button_text: *const c_char,
        response_id: i32,
    ) -> *mut GtkWidget;
    pub fn gtk_dialog_set_default_response(dialog: *mut GtkDialog, response_id: i32);
}

// MARK: GTK File Chooser
pub const GTK_FILE_CHOOSER_ACTION_OPEN: i32 = 0;
pub const GTK_FILE_CHOOSER_ACTION_SAVE: i32 = 1;
pub const GTK_RESPONSE_ACCEPT: i32 = -3;
pub const GTK_RESPONSE_CANCEL: i32 = -6;
#[repr(C)]
pub struct GtkFileFilter([u8; 0]);

// GTK 3.20+
#[repr(C)]
pub struct GtkFileChooserNative([u8; 0]);
#[repr(C)]
pub struct GtkNativeDialog([u8; 0]);

unsafe extern "C" {
    pub fn gtk_file_chooser_set_select_multiple(chooser: *mut c_void, select_multiple: bool);
    pub fn gtk_file_chooser_set_current_folder(chooser: *mut c_void, folder: *const c_char)
    -> bool;
    pub fn gtk_file_chooser_set_current_name(chooser: *mut c_void, name: *const c_char);
    pub fn gtk_file_chooser_get_filename(chooser: *mut c_void) -> *mut c_char;
    pub fn gtk_file_chooser_get_filenames(chooser: *mut c_void) -> *mut GSList;
    pub fn gtk_file_filter_new() -> *mut GtkFileFilter;
    pub fn gtk_file_filter_set_name(filter: *mut GtkFileFilter, name: *const c_char);
    pub fn gtk_file_filter_add_pattern(filter: *mut GtkFileFilter, pattern: *const c_char);
    pub fn gtk_file_chooser_add_filter(chooser: *mut c_void, filter: *mut GtkFileFilter);

    // GTK < 3.20
    pub fn gtk_file_chooser_dialog_new(
        title: *const c_char,
        parent: *mut GtkWindow,
        action: i32,
        first_button_text: *const c_char,
        ...
    ) -> *mut GtkWidget;

    // GTK 3.20+
    pub fn gtk_file_chooser_native_new(
        title: *const c_char,
        parent: *mut GtkWindow,
        action: i32,
        accept_label: *const c_char,
        cancel_label: *const c_char,
    ) -> *mut GtkFileChooserNative;
    pub fn gtk_native_dialog_run(dialog: *mut GtkNativeDialog) -> i32;
}
