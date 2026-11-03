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
pub(crate) const GDK_KEY_HOME: u32 = 0xff50;
pub(crate) const GDK_KEY_UP: u32 = 0xff52;
pub(crate) const GDK_KEY_DOWN: u32 = 0xff54;
pub(crate) const GDK_KEY_PAGE_UP: u32 = 0xff55;
pub(crate) const GDK_KEY_PAGE_DOWN: u32 = 0xff56;
pub(crate) const GDK_KEY_END: u32 = 0xff57;
pub(crate) const GTK_ALIGN_CENTER: i32 = 3;
pub(crate) const GTK_ORIENTATION_HORIZONTAL: i32 = 0;
pub(crate) const GTK_ORIENTATION_VERTICAL: i32 = 1;
pub(crate) const GTK_ICON_SIZE_BUTTON: i32 = 4;
pub(crate) const G_BINDING_SYNC_CREATE: u32 = 1 << 1;
pub(crate) const G_SIGNAL_MATCH_DATA: i32 = 1 << 4;

unsafe extern "C" {
    pub(crate) fn gtk_header_bar_new() -> *mut c_void;
    pub(crate) fn gtk_header_bar_set_subtitle(bar: *mut c_void, subtitle: *const c_char);
    pub(crate) fn gtk_header_bar_set_show_close_button(bar: *mut c_void, show: i32);
    pub(crate) fn gtk_header_bar_pack_start(bar: *mut c_void, child: *mut c_void);
    pub(crate) fn gtk_header_bar_pack_end(bar: *mut c_void, child: *mut c_void);
    pub(crate) fn gtk_window_set_default_size(window: *mut c_void, width: i32, height: i32);
    pub(crate) fn gtk_window_resize(window: *mut c_void, width: i32, height: i32);
    pub(crate) fn gtk_widget_get_preferred_width(
        widget: *mut c_void,
        minimum: *mut i32,
        natural: *mut i32,
    );
    pub(crate) fn gtk_widget_get_preferred_height(
        widget: *mut c_void,
        minimum: *mut i32,
        natural: *mut i32,
    );
    pub(crate) fn gtk_widget_set_margin_top(widget: *mut c_void, margin: i32);
    pub(crate) fn gtk_widget_set_margin_bottom(widget: *mut c_void, margin: i32);
    pub(crate) fn gtk_window_set_titlebar(window: *mut c_void, bar: *mut c_void);
    pub(crate) fn gtk_widget_hide(widget: *mut c_void);
    pub(crate) fn gtk_widget_unrealize(widget: *mut c_void);
    pub(crate) fn gtk_widget_show_all(widget: *mut c_void);
    pub(crate) fn gtk_box_new(orientation: i32, spacing: i32) -> *mut c_void;
    pub(crate) fn gtk_box_pack_start(
        container: *mut c_void,
        child: *mut c_void,
        expand: i32,
        fill: i32,
        padding: u32,
    );
    pub(crate) fn gtk_box_pack_end(
        container: *mut c_void,
        child: *mut c_void,
        expand: i32,
        fill: i32,
        padding: u32,
    );
    pub(crate) fn gtk_button_new_from_icon_name(name: *const c_char, size: i32) -> *mut c_void;
    pub(crate) fn gtk_widget_set_tooltip_text(widget: *mut c_void, text: *const c_char);
    pub(crate) fn gtk_button_set_focus_on_click(widget: *mut c_void, focus: i32);
    pub(crate) fn gtk_widget_get_style_context(widget: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_style_context_add_class(context: *mut c_void, class: *const c_char);
    pub(crate) fn g_object_bind_property(
        source: *mut c_void,
        source_property: *const c_char,
        target: *mut c_void,
        target_property: *const c_char,
        flags: u32,
    ) -> *mut c_void;

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
    pub(crate) fn gtk_overlay_new() -> *mut c_void;
    pub(crate) fn gtk_overlay_add_overlay(overlay: *mut c_void, widget: *mut c_void);
    pub(crate) fn gtk_overlay_set_overlay_pass_through(
        overlay: *mut c_void,
        widget: *mut c_void,
        pass_through: i32,
    );
    pub(crate) fn gtk_spinner_new() -> *mut c_void;
    pub(crate) fn gtk_spinner_start(spinner: *mut c_void);
    pub(crate) fn gtk_widget_set_halign(widget: *mut c_void, align: i32);
    pub(crate) fn gtk_widget_set_valign(widget: *mut c_void, align: i32);
    pub(crate) fn gtk_widget_destroy(widget: *mut c_void);
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
    pub(crate) fn gtk_adjustment_get_value(adjustment: *mut c_void) -> f64;
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
    pub(crate) fn gdk_event_get_keyval(event: *const c_void, keyval: *mut u32) -> i32;
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
