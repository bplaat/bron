/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{c_char, c_void};

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

pub(super) const PANGO_SCALE: i32 = 1024;
pub(super) const PANGO_WEIGHT_NORMAL: i32 = 400;
pub(super) const PANGO_WEIGHT_BOLD: i32 = 700;

#[repr(C)]
#[derive(Default)]
pub(super) struct PangoRectangle {
    pub(super) x: i32,
    pub(super) y: i32,
    pub(super) width: i32,
    pub(super) height: i32,
}

unsafe extern "C" {
    pub(super) fn cairo_save(cr: *mut c_void);
    pub(super) fn cairo_restore(cr: *mut c_void);
    pub(super) fn cairo_set_source_rgba(cr: *mut c_void, r: f64, g: f64, b: f64, a: f64);
    pub(super) fn cairo_set_operator(cr: *mut c_void, op: i32);
    pub(super) fn cairo_rectangle(cr: *mut c_void, x: f64, y: f64, w: f64, h: f64);
    pub(super) fn cairo_fill(cr: *mut c_void);
    pub(super) fn cairo_fill_preserve(cr: *mut c_void);
    pub(super) fn cairo_stroke_preserve(cr: *mut c_void);
    pub(super) fn cairo_new_path(cr: *mut c_void);
    pub(super) fn cairo_close_path(cr: *mut c_void);
    pub(super) fn cairo_move_to(cr: *mut c_void, x: f64, y: f64);
    pub(super) fn cairo_line_to(cr: *mut c_void, x: f64, y: f64);
    pub(super) fn cairo_curve_to(cr: *mut c_void, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64);
    pub(super) fn cairo_clip(cr: *mut c_void);
    pub(super) fn cairo_clip_preserve(cr: *mut c_void);
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
    pub(super) fn cairo_font_options_create() -> *mut c_void;
    pub(super) fn cairo_font_options_destroy(options: *mut c_void);
    pub(super) fn cairo_font_options_set_antialias(options: *mut c_void, antialias: i32);
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
    pub(super) fn cairo_surface_status(surface: *mut c_void) -> i32;
    pub(super) fn cairo_surface_flush(surface: *mut c_void);
    pub(super) fn cairo_surface_destroy(surface: *mut c_void);
    pub(super) fn cairo_surface_create_for_rectangle(
        target: *mut c_void,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> *mut c_void;
    pub(super) fn cairo_create(surface: *mut c_void) -> *mut c_void;
    pub(super) fn cairo_destroy(cr: *mut c_void);
    pub(super) fn cairo_status(cr: *mut c_void) -> i32;
    pub(super) fn cairo_set_fill_rule(cr: *mut c_void, rule: i32);
    pub(super) fn cairo_set_dash(cr: *mut c_void, dashes: *const f64, count: i32, offset: f64);
    pub(super) fn cairo_set_miter_limit(cr: *mut c_void, limit: f64);
    pub(super) fn cairo_paint_with_alpha(cr: *mut c_void, alpha: f64);
    pub(super) fn cairo_mask(cr: *mut c_void, pattern: *mut c_void);
    pub(super) fn cairo_push_group(cr: *mut c_void);
    pub(super) fn cairo_pop_group(cr: *mut c_void) -> *mut c_void;
    pub(super) fn cairo_pop_group_to_source(cr: *mut c_void);
    pub(super) fn cairo_set_source(cr: *mut c_void, pattern: *mut c_void);
    pub(super) fn cairo_set_source_surface(cr: *mut c_void, surface: *mut c_void, x: f64, y: f64);
    pub(super) fn cairo_get_source(cr: *mut c_void) -> *mut c_void;
    pub(super) fn cairo_get_group_target(cr: *mut c_void) -> *mut c_void;
    pub(super) fn cairo_pattern_reference(pattern: *mut c_void) -> *mut c_void;
    pub(super) fn cairo_identity_matrix(cr: *mut c_void);
    pub(super) fn cairo_paint(cr: *mut c_void);
    pub(super) fn cairo_pattern_create_linear(x0: f64, y0: f64, x1: f64, y1: f64) -> *mut c_void;
    pub(super) fn cairo_pattern_create_radial(
        cx0: f64,
        cy0: f64,
        radius0: f64,
        cx1: f64,
        cy1: f64,
        radius1: f64,
    ) -> *mut c_void;
    pub(super) fn cairo_pattern_add_color_stop_rgba(
        pattern: *mut c_void,
        offset: f64,
        r: f64,
        g: f64,
        b: f64,
        a: f64,
    );
    pub(super) fn cairo_pattern_set_extend(pattern: *mut c_void, extend: i32);
    pub(super) fn cairo_pattern_set_filter(pattern: *mut c_void, filter: i32);
    pub(super) fn cairo_pattern_set_matrix(pattern: *mut c_void, matrix: *const CairoMatrix);
    pub(super) fn cairo_pattern_destroy(pattern: *mut c_void);
}

pub(super) const CAIRO_FORMAT_ARGB32: i32 = 0;
pub(super) const CAIRO_ANTIALIAS_GRAY: i32 = 2;
pub(super) const CAIRO_OPERATOR_SOURCE: i32 = 1;
pub(super) const CAIRO_OPERATOR_OVER: i32 = 2;
pub(super) const CAIRO_EXTEND_PAD: i32 = 3;
pub(super) const CAIRO_FILTER_GOOD: i32 = 1;
pub(super) const CAIRO_FILTER_NEAREST: i32 = 3;

// Pango lays out text with font fallback for emoji and every script, drawn with Cairo.
unsafe extern "C" {
    pub(super) fn pango_cairo_create_layout(cr: *mut c_void) -> *mut c_void;
    pub(super) fn pango_cairo_update_layout(cr: *mut c_void, layout: *mut c_void);
    pub(super) fn pango_cairo_show_layout(cr: *mut c_void, layout: *mut c_void);
    pub(super) fn pango_cairo_layout_path(cr: *mut c_void, layout: *mut c_void);
    pub(super) fn pango_cairo_context_set_font_options(
        context: *mut c_void,
        options: *const c_void,
    );
    pub(super) fn pango_layout_get_context(layout: *mut c_void) -> *mut c_void;
    pub(super) fn pango_layout_context_changed(layout: *mut c_void);
    pub(super) fn pango_layout_set_text(layout: *mut c_void, text: *const c_char, length: i32);
    pub(super) fn pango_layout_set_font_description(layout: *mut c_void, font: *const c_void);
    pub(super) fn pango_layout_get_extents(
        layout: *mut c_void,
        ink: *mut PangoRectangle,
        logical: *mut PangoRectangle,
    );
    pub(super) fn pango_layout_get_baseline(layout: *mut c_void) -> i32;
    pub(super) fn pango_font_description_new() -> *mut c_void;
    pub(super) fn pango_font_description_set_family(font: *mut c_void, family: *const c_char);
    pub(super) fn pango_font_description_set_weight(font: *mut c_void, weight: i32);
    pub(super) fn pango_font_description_set_absolute_size(font: *mut c_void, size: f64);
    pub(super) fn pango_font_description_free(font: *mut c_void);
    pub(super) fn g_object_unref(object: *mut c_void);
}
