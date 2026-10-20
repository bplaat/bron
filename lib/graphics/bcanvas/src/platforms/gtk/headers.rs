/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{c_char, c_void};

use bwindow::ffi::{GObject, GtkWidget};

unsafe extern "C" {
    pub(super) fn gdk_window_get_display(window: *mut c_void) -> *mut c_void;
    pub(super) fn gdk_cursor_new_from_name(
        display: *mut c_void,
        name: *const c_char,
    ) -> *mut GObject;
    pub(super) fn gdk_window_set_cursor(window: *mut c_void, cursor: *mut GObject);
    pub(super) fn gtk_drawing_area_new() -> *mut GtkWidget;
    pub(super) fn gtk_widget_set_can_focus(widget: *mut GtkWidget, can_focus: i32);
    pub(super) fn gtk_widget_grab_focus(widget: *mut GtkWidget);
    pub(super) fn gtk_widget_add_events(widget: *mut GtkWidget, events: i32);
    pub(super) fn gtk_widget_queue_draw(widget: *mut GtkWidget);
    pub(super) fn gtk_widget_add_tick_callback(
        widget: *mut GtkWidget,
        callback: extern "C" fn(*mut GtkWidget, *mut c_void, *mut c_void) -> i32,
        data: *mut c_void,
        notify: extern "C" fn(*mut c_void),
    ) -> u32;
    pub(super) fn gtk_widget_remove_tick_callback(widget: *mut GtkWidget, id: u32);
    pub(super) fn g_object_ref_sink(object: *mut GObject) -> *mut GObject;
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct CairoMatrix {
    pub(super) xx: f64,
    pub(super) yx: f64,
    pub(super) xy: f64,
    pub(super) yy: f64,
    pub(super) x0: f64,
    pub(super) y0: f64,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct CairoTextExtents {
    pub(super) x_bearing: f64,
    pub(super) y_bearing: f64,
    pub(super) width: f64,
    pub(super) height: f64,
    pub(super) x_advance: f64,
    pub(super) y_advance: f64,
}

#[repr(C)]
pub(super) struct CairoPath([u8; 0]);

unsafe extern "C" {
    pub(super) fn cairo_save(cr: *mut c_void);
    pub(super) fn cairo_restore(cr: *mut c_void);
    pub(super) fn cairo_set_source_rgba(cr: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    pub(super) fn cairo_set_operator(cr: *mut c_void, op: i32);
    pub(super) fn cairo_rectangle(cr: *mut c_void, x: f64, y: f64, w: f64, h: f64);
    pub(super) fn cairo_fill(cr: *mut c_void);
    pub(super) fn cairo_fill_preserve(cr: *mut c_void);
    pub(super) fn cairo_stroke(cr: *mut c_void);
    pub(super) fn cairo_stroke_preserve(cr: *mut c_void);
    pub(super) fn cairo_new_path(cr: *mut c_void);
    pub(super) fn cairo_new_sub_path(cr: *mut c_void);
    pub(super) fn cairo_copy_path(cr: *mut c_void) -> *mut CairoPath;
    pub(super) fn cairo_append_path(cr: *mut c_void, path: *const CairoPath);
    pub(super) fn cairo_path_destroy(path: *mut CairoPath);
    pub(super) fn cairo_close_path(cr: *mut c_void);
    pub(super) fn cairo_move_to(cr: *mut c_void, x: f64, y: f64);
    pub(super) fn cairo_line_to(cr: *mut c_void, x: f64, y: f64);
    pub(super) fn cairo_arc(cr: *mut c_void, x: f64, y: f64, r: f64, a: f64, b: f64);
    pub(super) fn cairo_arc_negative(cr: *mut c_void, x: f64, y: f64, r: f64, a: f64, b: f64);
    pub(super) fn cairo_curve_to(cr: *mut c_void, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64);
    pub(super) fn cairo_get_current_point(cr: *mut c_void, x: *mut f64, y: *mut f64);
    pub(super) fn cairo_has_current_point(cr: *mut c_void) -> i32;
    pub(super) fn cairo_clip(cr: *mut c_void);
    pub(super) fn cairo_translate(cr: *mut c_void, x: f64, y: f64);
    pub(super) fn cairo_scale(cr: *mut c_void, x: f64, y: f64);
    pub(super) fn cairo_get_matrix(cr: *mut c_void, m: *mut CairoMatrix);
    pub(super) fn cairo_set_matrix(cr: *mut c_void, m: *const CairoMatrix);
    pub(super) fn cairo_matrix_multiply(
        result: *mut CairoMatrix,
        a: *const CairoMatrix,
        b: *const CairoMatrix,
    );
    pub(super) fn cairo_matrix_invert(matrix: *mut CairoMatrix) -> i32;
    pub(super) fn cairo_set_line_width(cr: *mut c_void, w: f64);
    pub(super) fn cairo_set_line_cap(cr: *mut c_void, c: i32);
    pub(super) fn cairo_set_line_join(cr: *mut c_void, j: i32);
    pub(super) fn cairo_select_font_face(
        cr: *mut c_void,
        family: *const c_char,
        slant: i32,
        weight: i32,
    );
    pub(super) fn cairo_set_font_size(cr: *mut c_void, size: f64);
    pub(super) fn cairo_text_extents(
        cr: *mut c_void,
        text: *const c_char,
        extents: *mut CairoTextExtents,
    );
    pub(super) fn cairo_show_text(cr: *mut c_void, text: *const c_char);
    #[cfg(test)]
    pub(super) fn cairo_image_surface_create(format: i32, width: i32, height: i32) -> *mut c_void;
    pub(super) fn cairo_image_surface_create_for_data(
        data: *mut u8,
        format: i32,
        width: i32,
        height: i32,
        stride: i32,
    ) -> *mut c_void;
    #[cfg(test)]
    pub(super) fn cairo_image_surface_get_data(surface: *mut c_void) -> *mut u8;
    pub(super) fn cairo_surface_flush(surface: *mut c_void);
    pub(super) fn cairo_surface_destroy(surface: *mut c_void);
    pub(super) fn cairo_create(surface: *mut c_void) -> *mut c_void;
    pub(super) fn cairo_destroy(cr: *mut c_void);
    pub(super) fn cairo_status(cr: *mut c_void) -> i32;
}
