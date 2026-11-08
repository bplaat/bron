/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! GTK, GDK, GLib and cairo declarations, bwindow and bcanvas link their libraries.

use std::ffi::{c_char, c_ulong, c_void};

pub(crate) const GDK_CONTROL_MASK: u32 = 1 << 2;
pub(crate) const GDK_BUTTON1_MOTION_MASK: i32 = 1 << 5;
pub(crate) const GDK_BUTTON_PRESS_MASK: i32 = 1 << 8;
pub(crate) const GDK_BUTTON_RELEASE_MASK: i32 = 1 << 9;
pub(crate) const GDK_SCROLL_MASK: i32 = 1 << 21;
pub(crate) const GDK_SMOOTH_SCROLL_MASK: i32 = 1 << 23;
pub(crate) const GDK_TOUCHPAD_GESTURE_MASK: i32 = 1 << 24;
pub(crate) const GDK_BUTTON_PRESS: i32 = 4;
pub(crate) const GDK_SCROLL_UP: i32 = 0;
pub(crate) const GDK_SCROLL_DOWN: i32 = 1;
pub(crate) const GDK_SHIFT_MASK: u32 = 1 << 0;
pub(crate) const GDK_KEY_RETURN: u32 = 0xff0d;
pub(crate) const GDK_KEY_ESCAPE: u32 = 0xff1b;
pub(crate) const GDK_KEY_KP_ENTER: u32 = 0xff8d;
pub(crate) const GDK_KEY_HOME: u32 = 0xff50;
pub(crate) const GDK_KEY_UP: u32 = 0xff52;
pub(crate) const GDK_KEY_DOWN: u32 = 0xff54;
pub(crate) const GDK_KEY_PAGE_UP: u32 = 0xff55;
pub(crate) const GDK_KEY_PAGE_DOWN: u32 = 0xff56;
pub(crate) const GDK_KEY_END: u32 = 0xff57;
pub(crate) const GTK_ALIGN_START: i32 = 1;
pub(crate) const GTK_ALIGN_CENTER: i32 = 3;
pub(crate) const GTK_REVEALER_TRANSITION_TYPE_SLIDE_RIGHT: i32 = 2;
pub(crate) const GTK_REVEALER_TRANSITION_TYPE_SLIDE_LEFT: i32 = 3;
pub(crate) const GTK_WRAP_WORD_CHAR: i32 = 2;
pub(crate) const GTK_POLICY_NEVER: i32 = 2;
pub(crate) const GTK_POLICY_AUTOMATIC: i32 = 1;
pub(crate) const GTK_POS_BOTTOM: i32 = 3;
pub(crate) const GTK_ORIENTATION_HORIZONTAL: i32 = 0;
pub(crate) const GTK_ORIENTATION_VERTICAL: i32 = 1;
pub(crate) const GTK_ICON_SIZE_BUTTON: i32 = 4;
pub(crate) const G_BINDING_SYNC_CREATE: u32 = 1 << 1;
pub(crate) const G_SIGNAL_MATCH_DATA: i32 = 1 << 4;
pub(crate) const GDK_WINDOW_STATE_WITHDRAWN: u32 = 1 << 0;
pub(crate) const GDK_WINDOW_STATE_ICONIFIED: u32 = 1 << 1;
pub(crate) const CAIRO_CONTENT_COLOR: i32 = 0x1000;
pub(crate) const CAIRO_CONTENT_COLOR_ALPHA: i32 = 0x3000;

/// A clipboard format, as GTK reads it.
#[repr(C)]
pub(crate) struct GtkTargetEntry {
    pub(crate) target: *const c_char,
    pub(crate) flags: u32,
    pub(crate) info: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct GdkRectangle {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

unsafe extern "C" {
    pub(crate) fn gdk_cairo_get_clip_rectangle(cr: *mut c_void, rect: *mut GdkRectangle) -> i32;
    pub(crate) fn gdk_rectangle_union(
        a: *const GdkRectangle,
        b: *const GdkRectangle,
        dest: *mut GdkRectangle,
    );
    pub(crate) fn gdk_window_create_similar_surface(
        window: *mut c_void,
        content: i32,
        width: i32,
        height: i32,
    ) -> *mut c_void;
    pub(crate) fn gdk_window_get_state(window: *mut c_void) -> u32;
    pub(crate) fn cairo_create(surface: *mut c_void) -> *mut c_void;
    pub(crate) fn cairo_destroy(cr: *mut c_void);
    pub(crate) fn cairo_translate(cr: *mut c_void, x: f64, y: f64);
    pub(crate) fn cairo_set_source_surface(cr: *mut c_void, surface: *mut c_void, x: f64, y: f64);
    pub(crate) fn cairo_paint(cr: *mut c_void);
    pub(crate) fn cairo_surface_status(surface: *mut c_void) -> i32;
    pub(crate) fn cairo_surface_destroy(surface: *mut c_void);
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
    ) -> c_ulong;
    pub(crate) fn g_signal_handlers_disconnect_matched(
        instance: *mut c_void,
        mask: i32,
        signal: u32,
        detail: u32,
        closure: *mut c_void,
        handler: *mut c_void,
        data: *const c_void,
    ) -> u32;

    pub(crate) fn gtk_scrolled_window_new(h: *mut c_void, v: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_scrolled_window_get_hadjustment(window: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_scrolled_window_get_vadjustment(window: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_bin_get_child(bin: *mut c_void) -> *mut c_void;
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
    pub(crate) fn gtk_widget_queue_draw_area(
        widget: *mut c_void,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    );
    pub(crate) fn gtk_widget_add_tick_callback(
        widget: *mut c_void,
        callback: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        data: *const c_void,
        notify: Option<extern "C" fn(*mut c_void)>,
    ) -> u32;
    pub(crate) fn gtk_widget_remove_tick_callback(widget: *mut c_void, id: u32);
    pub(crate) fn gtk_widget_get_mapped(widget: *mut c_void) -> i32;
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

    pub(crate) fn gdk_event_get_event_type(event: *const c_void) -> i32;
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
    pub(crate) fn gdk_atom_intern(name: *const c_char, only_if_exists: i32) -> *mut c_void;

    pub(crate) fn gtk_widget_get_toplevel(widget: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_widget_has_focus(widget: *mut c_void) -> i32;
    pub(crate) fn gtk_widget_show(widget: *mut c_void);
    pub(crate) fn gtk_widget_set_no_show_all(widget: *mut c_void, no_show_all: i32);
    pub(crate) fn gtk_widget_set_visible(widget: *mut c_void, visible: i32);
    pub(crate) fn gtk_widget_set_sensitive(widget: *mut c_void, sensitive: i32);
    pub(crate) fn gtk_widget_set_margin_start(widget: *mut c_void, margin: i32);
    pub(crate) fn gtk_widget_set_margin_end(widget: *mut c_void, margin: i32);
    pub(crate) fn gtk_widget_get_preferred_size(
        widget: *mut c_void,
        minimum: *mut [i32; 2],
        natural: *mut [i32; 2],
    );
    pub(crate) fn gtk_button_new_with_label(label: *const c_char) -> *mut c_void;
    pub(crate) fn gtk_button_set_image(button: *mut c_void, image: *mut c_void);
    pub(crate) fn gtk_image_new_from_icon_name(name: *const c_char, size: i32) -> *mut c_void;
    pub(crate) fn gtk_toggle_button_get_active(button: *mut c_void) -> i32;
    pub(crate) fn gtk_toggle_button_set_active(button: *mut c_void, active: i32);
    pub(crate) fn gtk_toggle_button_set_mode(button: *mut c_void, draw_indicator: i32);
    pub(crate) fn gtk_radio_button_new_from_widget(group: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_menu_button_new() -> *mut c_void;
    pub(crate) fn gtk_menu_button_set_popup(button: *mut c_void, menu: *mut c_void);
    pub(crate) fn gtk_menu_new() -> *mut c_void;
    pub(crate) fn gtk_menu_item_new_with_label(label: *const c_char) -> *mut c_void;
    pub(crate) fn gtk_separator_menu_item_new() -> *mut c_void;
    pub(crate) fn gtk_menu_shell_append(menu: *mut c_void, item: *mut c_void);
    pub(crate) fn gtk_popover_new(relative_to: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_popover_set_modal(popover: *mut c_void, modal: i32);
    pub(crate) fn gtk_popover_set_position(popover: *mut c_void, position: i32);
    pub(crate) fn gtk_grid_new() -> *mut c_void;
    pub(crate) fn gtk_grid_set_row_spacing(grid: *mut c_void, spacing: u32);
    pub(crate) fn gtk_grid_set_column_spacing(grid: *mut c_void, spacing: u32);
    pub(crate) fn gtk_grid_attach(
        grid: *mut c_void,
        child: *mut c_void,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
    );
    pub(crate) fn gtk_label_new(text: *const c_char) -> *mut c_void;
    pub(crate) fn gtk_label_set_text(label: *mut c_void, text: *const c_char);
    pub(crate) fn gtk_label_set_xalign(label: *mut c_void, align: f32);
    pub(crate) fn gtk_label_set_width_chars(label: *mut c_void, chars: i32);
    pub(crate) fn gtk_label_set_markup(label: *mut c_void, markup: *const c_char);
    pub(crate) fn gtk_scale_new_with_range(
        orientation: i32,
        minimum: f64,
        maximum: f64,
        step: f64,
    ) -> *mut c_void;
    pub(crate) fn gtk_scale_set_draw_value(scale: *mut c_void, draw: i32);
    pub(crate) fn gtk_range_get_value(range: *mut c_void) -> f64;
    pub(crate) fn gtk_range_set_value(range: *mut c_void, value: f64);
    pub(crate) fn gtk_spin_button_new_with_range(
        minimum: f64,
        maximum: f64,
        step: f64,
    ) -> *mut c_void;
    pub(crate) fn gtk_spin_button_set_numeric(button: *mut c_void, numeric: i32);
    pub(crate) fn gtk_spin_button_set_value(button: *mut c_void, value: f64);
    pub(crate) fn gtk_entry_get_text(entry: *mut c_void) -> *const c_char;
    pub(crate) fn gtk_combo_box_text_new() -> *mut c_void;
    pub(crate) fn gtk_combo_box_text_append_text(combo: *mut c_void, text: *const c_char);
    pub(crate) fn gtk_combo_box_get_active(combo: *mut c_void) -> i32;
    pub(crate) fn gtk_combo_box_set_active(combo: *mut c_void, index: i32);
    pub(crate) fn gtk_check_button_new_with_label(label: *const c_char) -> *mut c_void;
    pub(crate) fn gtk_scrolled_window_set_policy(window: *mut c_void, h: i32, v: i32);
    pub(crate) fn gtk_clipboard_get(selection: *mut c_void) -> *mut c_void;
    pub(crate) fn gtk_clipboard_set_with_data(
        clipboard: *mut c_void,
        targets: *const GtkTargetEntry,
        count: u32,
        get: extern "C" fn(*mut c_void, *mut c_void, u32, *mut c_void),
        clear: extern "C" fn(*mut c_void, *mut c_void),
        data: *mut c_void,
    ) -> i32;
    pub(crate) fn gtk_clipboard_set_can_store(
        clipboard: *mut c_void,
        targets: *const GtkTargetEntry,
        count: i32,
    );
    pub(crate) fn gtk_selection_data_set(
        data: *mut c_void,
        kind: *mut c_void,
        format: i32,
        bytes: *const u8,
        length: i32,
    );
    pub(crate) fn gtk_file_chooser_set_extra_widget(chooser: *mut c_void, widget: *mut c_void);
    pub(crate) fn gtk_file_chooser_set_do_overwrite_confirmation(
        chooser: *mut c_void,
        confirm: i32,
    );
    pub(crate) fn gtk_file_chooser_set_current_name(chooser: *mut c_void, name: *const c_char);
    pub(crate) fn gtk_file_chooser_get_current_name(chooser: *mut c_void) -> *mut c_char;
    pub(crate) fn g_free(memory: *mut c_void);
    pub(crate) fn gtk_revealer_new() -> *mut c_void;
    pub(crate) fn gtk_revealer_set_transition_type(revealer: *mut c_void, transition: i32);
    pub(crate) fn gtk_revealer_set_reveal_child(revealer: *mut c_void, reveal: i32);
    pub(crate) fn gtk_separator_new(orientation: i32) -> *mut c_void;
    pub(crate) fn gtk_list_box_new() -> *mut c_void;
    pub(crate) fn gtk_list_box_select_row(list: *mut c_void, row: *mut c_void);
    pub(crate) fn gtk_list_box_get_row_at_index(list: *mut c_void, index: i32) -> *mut c_void;
    pub(crate) fn gtk_list_box_row_get_index(row: *mut c_void) -> i32;
    pub(crate) fn gtk_toggle_button_new() -> *mut c_void;
    pub(crate) fn gtk_label_set_line_wrap(label: *mut c_void, wrap: i32);
    pub(crate) fn gtk_label_set_line_wrap_mode(label: *mut c_void, mode: i32);
    pub(crate) fn gtk_label_set_selectable(label: *mut c_void, selectable: i32);
}
