/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! GTK, GDK and GLib declarations, bwindow links their libraries.

use std::ffi::{c_char, c_void};

pub(crate) const GDK_CONTROL_MASK: u32 = 1 << 2;
pub(crate) const GDK_BUTTON1_MOTION_MASK: i32 = 1 << 5;
pub(crate) const GDK_BUTTON_PRESS_MASK: i32 = 1 << 8;
pub(crate) const GDK_BUTTON_RELEASE_MASK: i32 = 1 << 9;
pub(crate) const GDK_SCROLL_MASK: i32 = 1 << 21;
pub(crate) const GDK_SMOOTH_SCROLL_MASK: i32 = 1 << 23;
pub(crate) const GDK_TOUCHPAD_GESTURE_MASK: i32 = 1 << 24;
pub(crate) const GDK_SCROLL_UP: i32 = 0;
pub(crate) const GDK_SCROLL_DOWN: i32 = 1;
pub(crate) const G_SIGNAL_MATCH_DATA: i32 = 1 << 4;

#[repr(C)]
#[derive(Default)]
pub(crate) struct GdkRgba {
    pub(crate) red: f64,
    pub(crate) green: f64,
    pub(crate) blue: f64,
    pub(crate) alpha: f64,
}

unsafe extern "C" {
    pub(crate) fn g_object_ref(object: *mut c_void) -> *mut c_void;
    pub(crate) fn g_object_ref_sink(object: *mut c_void) -> *mut c_void;
    pub(crate) fn g_object_unref(object: *mut c_void);
    pub(crate) fn g_signal_connect_data(
        instance: *mut c_void,
        signal: *const c_char,
        handler: *const c_void,
        data: *const c_void,
        destroy: *const c_void,
        flags: i32,
    ) -> u64;
    pub(crate) fn g_signal_handlers_disconnect_matched(
        instance: *mut c_void,
        mask: i32,
        signal: u32,
        detail: u32,
        closure: *mut c_void,
        handler: *mut c_void,
        data: *const c_void,
    ) -> u32;
    pub(crate) fn g_timeout_add_full(
        priority: i32,
        interval: u32,
        function: extern "C" fn(*mut c_void) -> i32,
        data: *mut c_void,
        destroy: extern "C" fn(*mut c_void),
    ) -> u32;
    pub(crate) fn g_source_remove(id: u32) -> i32;

    pub(crate) fn gtk_scrolled_window_new(h: *mut c_void, v: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_scrolled_window_get_hadjustment(window: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_scrolled_window_get_vadjustment(window: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_drawing_area_new() -> *mut c_void;
    pub(crate) fn gtk_container_add(container: *mut c_void, widget: *mut c_void);
    pub(crate) fn gtk_widget_set_size_request(widget: *mut c_void, width: i32, height: i32);
    pub(crate) fn gtk_widget_get_allocated_width(widget: *mut c_void) -> i32;
    pub(crate) fn gtk_widget_get_allocated_height(widget: *mut c_void) -> i32;
    pub(crate) fn gtk_widget_get_scale_factor(widget: *mut c_void) -> i32;
    pub(crate) fn gtk_widget_add_events(widget: *mut c_void, events: i32);
    pub(crate) fn gtk_widget_set_can_focus(widget: *mut c_void, can_focus: i32);
    pub(crate) fn gtk_widget_grab_focus(widget: *mut c_void);
    pub(crate) fn gtk_widget_queue_draw(widget: *mut c_void);
    pub(crate) fn gtk_widget_get_window(widget: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_widget_get_display(widget: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_widget_get_style_context(widget: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_style_context_get_state(context: *mut c_void) -> i32;
    pub(crate) fn gtk_style_context_get_color(
        context: *mut c_void,
        state: i32,
        color: *mut GdkRgba,
    );
    pub(crate) fn gtk_adjustment_get_value(adjustment: *mut c_void) -> f64;
    pub(crate) fn gtk_adjustment_get_page_size(adjustment: *mut c_void) -> f64;
    pub(crate) fn gtk_adjustment_get_step_increment(adjustment: *mut c_void) -> f64;
    pub(crate) fn gtk_adjustment_get_page_increment(adjustment: *mut c_void) -> f64;
    pub(crate) fn gtk_adjustment_set_value(adjustment: *mut c_void, value: f64);
    pub(crate) fn gtk_adjustment_configure(
        adjustment: *mut c_void,
        value: f64,
        lower: f64,
        upper: f64,
        step_increment: f64,
        page_increment: f64,
        page_size: f64,
    );
    pub(crate) fn gtk_gesture_zoom_new(widget: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_gesture_get_bounding_box_center(
        gesture: *mut c_void,
        x: *mut f64,
        y: *mut f64,
    ) -> i32;

    pub(crate) fn gdk_event_get_state(event: *const c_void, state: *mut u32) -> i32;
    pub(crate) fn gdk_event_get_coords(event: *const c_void, x: *mut f64, y: *mut f64) -> i32;
    pub(crate) fn gdk_event_get_root_coords(event: *const c_void, x: *mut f64, y: *mut f64) -> i32;
    pub(crate) fn gdk_event_get_button(event: *const c_void, button: *mut u32) -> i32;
    pub(crate) fn gdk_event_get_scroll_direction(event: *const c_void, direction: *mut i32) -> i32;
    pub(crate) fn gdk_event_get_scroll_deltas(
        event: *const c_void,
        x: *mut f64,
        y: *mut f64,
    ) -> i32;
    pub(crate) fn gdk_cursor_new_from_name(
        display: *mut c_void,
        name: *const c_char,
    ) -> *mut c_void;
    pub(crate) fn gdk_window_set_cursor(window: *mut c_void, cursor: *mut c_void);
}
