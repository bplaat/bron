/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(missing_docs)]
#![allow(unused)]

use std::ffi::{c_char, c_void};

// MARK: GDK
pub(crate) const GDK_WINDOW_STATE_FULLSCREEN: u32 = 1 << 4;
#[repr(C)]
pub(crate) struct GdkDisplay([u8; 0]);
#[repr(C)]
pub(crate) struct GdkDragContext([u8; 0]);
#[repr(C)]
pub(crate) struct GdkEvent([u8; 0]);
#[repr(C)]
#[derive(Default)]
pub(crate) struct GdkRectangle {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}
#[repr(C)]
pub(crate) struct GdkRGBA {
    pub(crate) red: f64,
    pub(crate) green: f64,
    pub(crate) blue: f64,
    pub(crate) alpha: f64,
}
// GTK < 3.22
#[repr(C)]
pub(crate) struct GdkScreen([u8; 0]);

// GTK 3.22+
#[repr(C)]
pub(crate) struct GdkMonitor([u8; 0]);

#[repr(C)]
pub(crate) struct GdkKeymap([u8; 0]);
#[repr(C)]
pub(crate) struct GdkKeymapKey {
    pub(crate) keycode: u32,
    pub(crate) group: i32,
    pub(crate) level: i32,
}
unsafe extern "C" {
    pub(crate) fn gdk_window_get_state(window: *mut c_void) -> u32;
    pub(crate) fn gdk_keymap_get_for_display(display: *mut GdkDisplay) -> *mut GdkKeymap;
    pub(crate) fn gdk_keymap_get_entries_for_keyval(
        keymap: *mut GdkKeymap,
        keyval: u32,
        keys: *mut *mut GdkKeymapKey,
        count: *mut i32,
    ) -> i32;
    pub(crate) fn gdk_keymap_translate_keyboard_state(
        keymap: *mut GdkKeymap,
        keycode: u32,
        state: u32,
        group: i32,
        keyval: *mut u32,
        effective_group: *mut i32,
        level: *mut i32,
        consumed: *mut u32,
    ) -> i32;
    pub(crate) fn gdk_event_get_event_type(event: *const GdkEvent) -> i32;
    pub(crate) fn gdk_event_get_keycode(event: *const GdkEvent, code: *mut u16) -> i32;
    pub(crate) fn gdk_event_get_keyval(event: *const GdkEvent, keyval: *mut u32) -> i32;
    pub(crate) fn gdk_event_get_coords(event: *const GdkEvent, x: *mut f64, y: *mut f64) -> i32;
    pub(crate) fn gdk_event_get_state(event: *const GdkEvent, state: *mut u32) -> i32;
    pub(crate) fn gdk_event_get_button(event: *const GdkEvent, button: *mut u32) -> i32;
    pub(crate) fn gdk_event_get_scroll_direction(
        event: *const GdkEvent,
        direction: *mut i32,
    ) -> i32;
    pub(crate) fn gdk_event_get_scroll_deltas(
        event: *const GdkEvent,
        x: *mut f64,
        y: *mut f64,
    ) -> i32;
    pub(crate) fn gdk_keyval_to_unicode(keyval: u32) -> u32;
    pub(crate) fn gdk_display_get_default() -> *mut GdkDisplay;

    // GTK < 3.22
    pub(crate) fn gdk_screen_get_default() -> *mut GdkScreen;
    pub(crate) fn gdk_screen_get_n_monitors(screen: *mut GdkScreen) -> i32;
    pub(crate) fn gdk_screen_get_primary_monitor(screen: *mut GdkScreen) -> i32;
    pub(crate) fn gdk_screen_get_monitor_geometry(
        screen: *mut GdkScreen,
        monitor_num: i32,
        dest: *mut GdkRectangle,
    );
    pub(crate) fn gdk_screen_get_monitor_scale_factor(
        screen: *mut GdkScreen,
        monitor_num: i32,
    ) -> i32;
    pub(crate) fn gdk_screen_get_monitor_plug_name(
        screen: *mut GdkScreen,
        monitor_num: i32,
    ) -> *mut c_char;

    // GTK 3.22+
    pub(crate) fn gdk_display_get_n_monitors(display: *mut GdkDisplay) -> i32;
    pub(crate) fn gdk_display_get_monitor(
        display: *mut GdkDisplay,
        monitor_num: i32,
    ) -> *mut GdkMonitor;
    pub(crate) fn gdk_display_get_primary_monitor(display: *mut GdkDisplay) -> *mut GdkMonitor;
    pub(crate) fn gdk_monitor_get_model(monitor: *mut GdkMonitor) -> *const c_char;
    pub(crate) fn gdk_monitor_get_geometry(monitor: *mut GdkMonitor, geometry: *mut GdkRectangle);
    pub(crate) fn gdk_monitor_get_scale_factor(monitor: *mut GdkMonitor) -> i32;
    pub(crate) fn gdk_monitor_is_primary(monitor: *mut GdkMonitor) -> bool;
}

/// Whether a display uses the Wayland backend.
///
/// # Safety
/// `display` must be a live GdkDisplay on its UI thread.
pub(crate) unsafe fn gdk_display_is_wayland(display: *mut GdkDisplay) -> bool {
    let type_ = unsafe { super::glib::g_type_from_name(c"GdkWaylandDisplay".as_ptr()) };
    type_ != 0 && unsafe { super::glib::g_type_check_instance_is_a(display.cast(), type_) != 0 }
}
