/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

// MARK: GDK
pub const GDK_WINDOW_STATE_FULLSCREEN: u32 = 1 << 4;
#[repr(C)]
pub struct GdkDisplay([u8; 0]);
#[repr(C)]
pub struct GdkDragContext([u8; 0]);
#[repr(C)]
pub struct GdkEvent([u8; 0]);
#[repr(C)]
pub struct GdkRectangle {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
#[repr(C)]
pub struct GdkRGBA {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}
// GTK < 3.22
#[repr(C)]
pub struct GdkScreen([u8; 0]);

// GTK 3.22+
#[repr(C)]
pub struct GdkMonitor([u8; 0]);

#[repr(C)]
pub struct GdkKeymap([u8; 0]);
#[repr(C)]
pub struct GdkKeymapKey {
    pub keycode: u32,
    pub group: i32,
    pub level: i32,
}
unsafe extern "C" {
    pub fn gdk_window_get_state(window: *mut c_void) -> u32;
    pub fn gdk_keymap_get_for_display(display: *mut GdkDisplay) -> *mut GdkKeymap;
    pub fn gdk_keymap_get_entries_for_keyval(
        keymap: *mut GdkKeymap,
        keyval: u32,
        keys: *mut *mut GdkKeymapKey,
        count: *mut i32,
    ) -> i32;
    pub fn gdk_keymap_translate_keyboard_state(
        keymap: *mut GdkKeymap,
        keycode: u32,
        state: u32,
        group: i32,
        keyval: *mut u32,
        effective_group: *mut i32,
        level: *mut i32,
        consumed: *mut u32,
    ) -> i32;
    pub fn gdk_event_get_event_type(event: *const GdkEvent) -> i32;
    pub fn gdk_event_get_keycode(event: *const GdkEvent, code: *mut u16) -> i32;
    pub fn gdk_event_get_keyval(event: *const GdkEvent, keyval: *mut u32) -> i32;
    pub fn gdk_event_get_coords(event: *const GdkEvent, x: *mut f64, y: *mut f64) -> i32;
    pub fn gdk_event_get_state(event: *const GdkEvent, state: *mut u32) -> i32;
    pub fn gdk_event_get_button(event: *const GdkEvent, button: *mut u32) -> i32;
    pub fn gdk_event_get_scroll_direction(event: *const GdkEvent, direction: *mut i32) -> i32;
    pub fn gdk_event_get_scroll_deltas(event: *const GdkEvent, x: *mut f64, y: *mut f64) -> i32;
    pub fn gdk_keyval_to_unicode(keyval: u32) -> u32;
    pub fn gdk_display_get_default() -> *mut GdkDisplay;
    pub fn gdk_display_get_name(display: *mut GdkDisplay) -> *const c_char;

    // GTK < 3.22
    pub fn gdk_screen_get_default() -> *mut GdkScreen;
    pub fn gdk_screen_get_n_monitors(screen: *mut GdkScreen) -> i32;
    pub fn gdk_screen_get_primary_monitor(screen: *mut GdkScreen) -> i32;
    pub fn gdk_screen_get_monitor_geometry(
        screen: *mut GdkScreen,
        monitor_num: i32,
        dest: *mut GdkRectangle,
    );
    pub fn gdk_screen_get_monitor_scale_factor(screen: *mut GdkScreen, monitor_num: i32) -> i32;
    pub fn gdk_screen_get_monitor_plug_name(
        screen: *mut GdkScreen,
        monitor_num: i32,
    ) -> *mut c_char;

    // GTK 3.22+
    pub fn gdk_display_get_n_monitors(display: *mut GdkDisplay) -> i32;
    pub fn gdk_display_get_monitor(display: *mut GdkDisplay, monitor_num: i32) -> *mut GdkMonitor;
    pub fn gdk_display_get_primary_monitor(display: *mut GdkDisplay) -> *mut GdkMonitor;
    pub fn gdk_monitor_get_model(monitor: *mut GdkMonitor) -> *const c_char;
    pub fn gdk_monitor_get_geometry(monitor: *mut GdkMonitor, geometry: *mut GdkRectangle);
    pub fn gdk_monitor_get_scale_factor(monitor: *mut GdkMonitor) -> i32;
    pub fn gdk_monitor_is_primary(monitor: *mut GdkMonitor) -> bool;
}

/// Whether a display uses the Wayland backend.
///
/// # Safety
/// `display` must be a live GdkDisplay on its UI thread.
pub unsafe fn gdk_display_is_wayland(display: *mut GdkDisplay) -> bool {
    let type_ = unsafe { super::glib::g_type_from_name(c"GdkWaylandDisplay".as_ptr()) };
    type_ != 0 && unsafe { super::glib::g_type_check_instance_is_a(display.cast(), type_) != 0 }
}
