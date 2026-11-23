/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CString, c_void};
use std::sync::Arc;

use super::headers::*;
use crate::path::{Command, Path};
use crate::{
    CanvasState, CanvasStyle, CompositeOperation, FillRule, FontWeight, Gradient, GradientShape,
    GradientSpread, LineCap, LineJoin, TextExtents, single_line,
};

pub(crate) struct PlatformCanvasContext {
    cr: *mut c_void,
    initial: CairoMatrix,
    // Cairo cannot hold a singular matrix, so drawing is skipped instead.
    degenerate: bool,
    saved: Vec<bool>,
    // The text layout, created when text is first drawn or measured, and its font.
    layout: Option<*mut c_void>,
    font: Option<(Arc<str>, FontWeight, f32)>,
    layers: Vec<Layer>,
}

// A layer drawn into a group, composited with its alpha and operator when it ends.
struct Layer {
    alpha: f64,
    operator: i32,
    // The mask of a masked layer, once its drawing is finished.
    mask: Option<*mut c_void>,
    masked: bool,
}
impl PlatformCanvasContext {
    pub(super) unsafe fn new(cr: *mut c_void) -> Self {
        let mut initial = CairoMatrix {
            xx: 1.0,
            yx: 0.0,
            xy: 0.0,
            yy: 1.0,
            x0: 0.0,
            y0: 0.0,
        };
        unsafe {
            cairo_get_matrix(cr, &mut initial);
            cairo_save(cr);
            cairo_new_path(cr);
        }
        Self {
            cr,
            initial,
            degenerate: false,
            saved: Vec::new(),
            layout: None,
            font: None,
            layers: Vec::new(),
        }
    }
    pub(crate) unsafe fn from_native(context: crate::NativeContext) -> Self {
        let crate::NativeContext::Cairo(cr) = context;
        unsafe { Self::new(cr) }
    }
    pub(crate) fn save(&mut self) {
        self.saved.push(self.degenerate);
        unsafe { cairo_save(self.cr) }
    }
    pub(crate) fn restore(&mut self) {
        self.degenerate = self.saved.pop().unwrap_or(false);
        unsafe { cairo_restore(self.cr) }
    }
    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if self.degenerate {
            return;
        }
        unsafe { cairo_new_path(self.cr) };
        unsafe {
            cairo_save(self.cr);
            cairo_rectangle(self.cr, x.into(), y.into(), w.into(), h.into());
            cairo_set_operator(self.cr, 0);
            cairo_fill(self.cr);
            cairo_restore(self.cr)
        }
    }
    pub(crate) fn fill_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        if self.degenerate {
            return;
        }
        unsafe { cairo_new_path(self.cr) };
        unsafe { cairo_rectangle(self.cr, x.into(), y.into(), w.into(), h.into()) };
        self.paint(s, false, FillRule::NonZero);
    }
    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        if self.degenerate {
            return;
        }
        unsafe { cairo_new_path(self.cr) };
        unsafe { cairo_rectangle(self.cr, x.into(), y.into(), w.into(), h.into()) };
        self.paint(s, true, FillRule::NonZero);
    }
    pub(crate) fn begin_layer(&mut self, s: &CanvasState) {
        self.push_layer(s, false);
    }
    pub(crate) fn begin_mask_layer(&mut self, s: &CanvasState) {
        self.push_layer(s, true);
        // The mask is drawn in a group of its own inside the layer group.
        unsafe { cairo_push_group(self.cr) };
    }
    fn push_layer(&mut self, s: &CanvasState, masked: bool) {
        self.layers.push(Layer {
            alpha: s.alpha.into(),
            operator: operator(s.composite),
            mask: None,
            masked,
        });
        self.saved.push(self.degenerate);
        unsafe { cairo_push_group(self.cr) };
    }
    pub(crate) fn end_mask(&mut self) {
        let layer = self.layers.last_mut().expect("open layer");
        unsafe {
            layer.mask = Some(cairo_pop_group(self.cr));
            cairo_push_group(self.cr);
        }
    }
    pub(crate) fn end_layer(&mut self) {
        let layer = self.layers.pop().expect("open layer");
        self.degenerate = self.saved.pop().unwrap_or(false);
        unsafe {
            if layer.masked {
                let content = cairo_pop_group(self.cr);
                cairo_set_source(self.cr, content);
                if let Some(mask) = layer.mask {
                    cairo_mask(self.cr, mask);
                    cairo_pattern_destroy(mask);
                }
                cairo_pattern_destroy(content);
            }
            cairo_pop_group_to_source(self.cr);
            cairo_set_operator(self.cr, layer.operator);
            cairo_paint_with_alpha(self.cr, layer.alpha);
            cairo_set_operator(self.cr, CAIRO_OPERATOR_OVER);
        }
    }
    pub(crate) fn draw_image(
        &mut self,
        s: &CanvasState,
        image: &PlatformImage,
        source: [f32; 4],
        [x, y, w, h]: [f32; 4],
    ) {
        if self.degenerate {
            return;
        }
        let [sx, sy, sw, sh] = source.map(f64::from);
        unsafe {
            cairo_new_path(self.cr);
            // A subsurface of the source pads at its own edges.
            let surface = cairo_surface_create_for_rectangle(image.surface, sx, sy, sw, sh);
            cairo_save(self.cr);
            cairo_translate(self.cr, x.into(), y.into());
            cairo_scale(self.cr, f64::from(w) / sw, f64::from(h) / sh);
            cairo_set_source_surface(self.cr, surface, 0.0, 0.0);
            cairo_surface_destroy(surface);
            let pattern = cairo_get_source(self.cr);
            // Padding keeps the edge pixels from blending with transparency when scaled.
            cairo_pattern_set_extend(pattern, CAIRO_EXTEND_PAD);
            cairo_pattern_set_filter(
                pattern,
                if s.image_smoothing {
                    CAIRO_FILTER_GOOD
                } else {
                    CAIRO_FILTER_NEAREST
                },
            );
            cairo_rectangle(self.cr, 0.0, 0.0, sw, sh);
            cairo_clip(self.cr);
            cairo_set_operator(self.cr, operator(s.composite));
            cairo_paint_with_alpha(self.cr, s.alpha.into());
            cairo_restore(self.cr);
        }
    }
    pub(crate) fn fill(&mut self, s: &CanvasState, path: &Path, rule: FillRule) {
        if self.degenerate {
            return;
        }
        self.set_path(path);
        self.paint(s, false, rule);
    }
    pub(crate) fn stroke(&mut self, s: &CanvasState, path: &Path) {
        if self.degenerate {
            return;
        }
        self.set_path(path);
        self.paint(s, true, FillRule::NonZero);
    }
    // Replace the current path, every subpath of which starts with a move.
    fn set_path(&self, path: &Path) {
        let mut current = [0.0; 2];
        unsafe {
            cairo_new_path(self.cr);
            for command in &path.commands {
                match *command {
                    Command::Move([x, y]) => cairo_move_to(self.cr, x.into(), y.into()),
                    Command::Line([x, y]) => cairo_line_to(self.cr, x.into(), y.into()),
                    Command::Quadratic(c, end) => {
                        // The cubic Bezier curve of a quadratic one.
                        let a = [0, 1].map(|i| current[i] + 2.0 / 3.0 * (c[i] - current[i]));
                        let b = [0, 1].map(|i| end[i] + 2.0 / 3.0 * (c[i] - end[i]));
                        curve_to(self.cr, a, b, end);
                    }
                    Command::Cubic(a, b, end) => curve_to(self.cr, a, b, end),
                    Command::Close => cairo_close_path(self.cr),
                }
                current = match *command {
                    Command::Move(point)
                    | Command::Line(point)
                    | Command::Quadratic(_, point)
                    | Command::Cubic(_, _, point) => point,
                    Command::Close => current,
                };
            }
        }
    }
    // Fills or strokes the current path with the style, keeping the path.
    fn paint(&self, s: &CanvasState, stroke: bool, rule: FillRule) {
        let style = if stroke {
            self.stroke_style(s);
            &s.stroke
        } else {
            unsafe { cairo_set_fill_rule(self.cr, fill_rule(rule)) };
            &s.fill
        };
        let pattern = self.set_source(style, s.alpha);
        let operator = operator(s.composite);
        unsafe {
            if !is_unbounded(operator) {
                cairo_set_operator(self.cr, operator);
                if stroke {
                    cairo_stroke_preserve(self.cr);
                } else {
                    cairo_fill_preserve(self.cr);
                }
                cairo_set_operator(self.cr, CAIRO_OPERATOR_OVER);
            } else if stroke {
                self.bounded(operator, || cairo_stroke_preserve(self.cr));
            } else {
                // A fill only changes its area when it clips the painted operator.
                cairo_save(self.cr);
                cairo_clip_preserve(self.cr);
                cairo_set_operator(self.cr, operator);
                cairo_paint(self.cr);
                cairo_restore(self.cr);
            }
            if let Some(pattern) = pattern {
                cairo_pattern_destroy(pattern);
            }
        }
    }
    // Paints an unbounded operator with the source only over the coverage that `draw`
    // paints: the result of the operator over everything is blended in through the coverage,
    // as the source operator interpolates by its mask.
    unsafe fn bounded(&self, operator: i32, draw: impl Fn()) {
        unsafe {
            let source = cairo_pattern_reference(cairo_get_source(self.cr));
            let target = cairo_get_group_target(self.cr);
            cairo_push_group(self.cr);
            cairo_save(self.cr);
            cairo_identity_matrix(self.cr);
            cairo_set_source_surface(self.cr, target, 0.0, 0.0);
            cairo_set_operator(self.cr, CAIRO_OPERATOR_SOURCE);
            cairo_paint(self.cr);
            cairo_restore(self.cr);
            cairo_set_source(self.cr, source);
            cairo_set_operator(self.cr, operator);
            cairo_paint(self.cr);
            let result = cairo_pop_group(self.cr);
            cairo_push_group(self.cr);
            cairo_set_source_rgba(self.cr, 1.0, 1.0, 1.0, 1.0);
            draw();
            let coverage = cairo_pop_group(self.cr);
            cairo_set_source(self.cr, result);
            cairo_set_operator(self.cr, CAIRO_OPERATOR_SOURCE);
            cairo_mask(self.cr, coverage);
            cairo_set_operator(self.cr, CAIRO_OPERATOR_OVER);
            cairo_set_source(self.cr, source);
            for pattern in [result, coverage, source] {
                cairo_pattern_destroy(pattern);
            }
        }
    }
    // Sets a color or gradient source, returning a pattern to destroy after painting.
    fn set_source(&self, style: &CanvasStyle, alpha: f32) -> Option<*mut c_void> {
        match style {
            CanvasStyle::Color(color) => {
                let (r, g, b, a) = color.components(alpha);
                unsafe { cairo_set_source_rgba(self.cr, r, g, b, a) };
                None
            }
            CanvasStyle::Gradient(gradient) => {
                let pattern = gradient_pattern(&gradient.0, alpha);
                unsafe { cairo_set_source(self.cr, pattern) };
                Some(pattern)
            }
        }
    }
    pub(crate) fn clip(&mut self, path: &Path, rule: FillRule) {
        // Clipping to an empty path hides everything until restore.
        self.set_path(path);
        unsafe {
            cairo_set_fill_rule(self.cr, fill_rule(rule));
            cairo_clip(self.cr);
        }
    }
    pub(crate) fn translate(&mut self, x: f32, y: f32) {
        self.apply_matrix([1.0, 0.0, 0.0, 1.0, x.into(), y.into()], false);
    }
    pub(crate) fn rotate(&mut self, a: f32) {
        let (sin, cos) = f64::from(a).sin_cos();
        self.apply_matrix([cos, sin, -sin, cos, 0.0, 0.0], false);
    }
    pub(crate) fn scale(&mut self, x: f32, y: f32) {
        self.apply_matrix([x.into(), 0.0, 0.0, y.into(), 0.0, 0.0], false);
    }
    pub(crate) fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.apply_matrix(
            [a.into(), b.into(), c.into(), d.into(), e.into(), f.into()],
            false,
        );
    }
    pub(crate) fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.apply_matrix(
            [a.into(), b.into(), c.into(), d.into(), e.into(), f.into()],
            true,
        );
    }
    fn apply_matrix(&mut self, [a, b, c, d, e, f]: [f64; 6], replace: bool) {
        // Non-finite arguments are ignored and a singular matrix stays singular.
        if ![a, b, c, d, e, f].iter().all(|v| v.is_finite()) || (!replace && self.degenerate) {
            return;
        }
        let m = CairoMatrix {
            xx: a,
            yx: b,
            xy: c,
            yy: d,
            x0: e,
            y0: f,
        };
        let mut base = self.initial;
        let mut candidate = self.initial;
        unsafe {
            if !replace {
                cairo_get_matrix(self.cr, &mut base);
            }
            // Cairo multiplication applies its first argument before the second.
            cairo_matrix_multiply(&mut candidate, &m, &base);
            let finite = |m: CairoMatrix| {
                [m.xx, m.yx, m.xy, m.yy, m.x0, m.y0]
                    .iter()
                    .all(|v| v.is_finite())
            };
            let mut inverse = candidate;
            // Context errors are permanent, unlike a failed matrix inversion.
            self.degenerate =
                !(finite(candidate) && cairo_matrix_invert(&mut inverse) == 0 && finite(inverse));
            if !self.degenerate {
                cairo_set_matrix(self.cr, &candidate);
            }
        }
    }
    pub(crate) fn reset_transform(&mut self) {
        self.degenerate = false;
        unsafe { cairo_set_matrix(self.cr, &self.initial) }
    }
    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        if self.degenerate {
            return;
        }
        let layout = self.layout(s, text);
        let extents = layout_extents(layout);
        let [dx, dy] = extents.origin(s);
        let operator = operator(s.composite);
        unsafe {
            cairo_new_path(self.cr);
            // Pango draws from the top left of the layout.
            cairo_move_to(
                self.cr,
                f64::from(x + dx),
                f64::from(y + dy - extents.ascent),
            );
        }
        // Colors keep the colors of glyphs like emoji, other paints fill the glyph outlines.
        if matches!(s.fill, CanvasStyle::Color(_)) && !is_unbounded(operator) {
            self.set_source(&s.fill, s.alpha);
            unsafe {
                cairo_set_operator(self.cr, operator);
                pango_cairo_show_layout(self.cr, layout);
                cairo_set_operator(self.cr, CAIRO_OPERATOR_OVER);
            }
        } else {
            unsafe { pango_cairo_layout_path(self.cr, layout) };
            self.paint(s, false, FillRule::NonZero);
        }
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> TextExtents {
        layout_extents(self.layout(s, text))
    }
    // The layout of one line of text in the font of a state, which follows the transform.
    fn layout(&mut self, s: &CanvasState, text: &str) -> *mut c_void {
        let cr = self.cr;
        let layout = *self.layout.get_or_insert_with(|| unsafe {
            let layout = pango_cairo_create_layout(cr);
            // Grayscale like filled outlines, as fontconfig may ask for subpixel glyphs.
            let options = cairo_font_options_create();
            cairo_font_options_set_antialias(options, CAIRO_ANTIALIAS_GRAY);
            pango_cairo_context_set_font_options(pango_layout_get_context(layout), options);
            cairo_font_options_destroy(options);
            pango_layout_context_changed(layout);
            layout
        });
        let font = (s.font_family.clone(), s.font_weight, s.font_size);
        if self.font.as_ref() != Some(&font) {
            let family =
                CString::new(s.font_family.as_ref()).unwrap_or_else(|_| c"sans-serif".to_owned());
            unsafe {
                let description = pango_font_description_new();
                pango_font_description_set_family(description, family.as_ptr());
                pango_font_description_set_weight(
                    description,
                    match s.font_weight {
                        FontWeight::Normal => PANGO_WEIGHT_NORMAL,
                        FontWeight::Bold => PANGO_WEIGHT_BOLD,
                    },
                );
                pango_font_description_set_absolute_size(
                    description,
                    f64::from(s.font_size) * f64::from(PANGO_SCALE),
                );
                pango_layout_set_font_description(layout, description);
                pango_font_description_free(description);
            }
            self.font = Some(font);
        }
        let text = single_line(text);
        unsafe {
            pango_cairo_update_layout(cr, layout);
            pango_layout_set_text(layout, text.as_ptr().cast(), text.len() as i32);
        }
        layout
    }
    fn stroke_style(&self, s: &CanvasState) {
        let dash: Vec<f64> = s.line_dash.iter().copied().map(f64::from).collect();
        unsafe {
            cairo_set_dash(
                self.cr,
                dash.as_ptr(),
                dash.len() as i32,
                s.line_dash_offset.into(),
            );
            cairo_set_miter_limit(self.cr, s.miter_limit.into());
            cairo_set_line_width(self.cr, s.line_width.into());
            cairo_set_line_cap(
                self.cr,
                match s.line_cap {
                    LineCap::Butt => 0,
                    LineCap::Round => 1,
                    LineCap::Square => 2,
                },
            );
            cairo_set_line_join(
                self.cr,
                match s.line_join {
                    LineJoin::Miter => 0,
                    LineJoin::Round => 1,
                    LineJoin::Bevel => 2,
                },
            )
        }
    }
}

impl Drop for PlatformCanvasContext {
    fn drop(&mut self) {
        unsafe {
            if let Some(layout) = self.layout {
                g_object_unref(layout);
            }
            cairo_new_path(self.cr);
            cairo_restore(self.cr);
        }
    }
}

/// A cairo image surface sharing the pixels of a canvas image.
pub(crate) struct PlatformImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    surface: *mut c_void,
    _pixels: Arc<[u8]>,
}

impl PlatformImage {
    pub(crate) fn new(width: u32, height: u32, pixels: Arc<[u8]>) -> Self {
        // ARGB32 pixels are native-endian words, which are BGRA bytes in little-endian order.
        // Cairo only reads the pixels of a surface that is a source.
        let surface = unsafe {
            cairo_image_surface_create_for_data(
                pixels.as_ptr().cast_mut(),
                CAIRO_FORMAT_ARGB32,
                width as i32,
                height as i32,
                (width * 4) as i32,
            )
        };
        assert_eq!(
            unsafe { cairo_surface_status(surface) },
            0,
            "Could not create Cairo image surface"
        );
        Self {
            width,
            height,
            surface,
            _pixels: pixels,
        }
    }
}

impl Drop for PlatformImage {
    fn drop(&mut self) {
        unsafe { cairo_surface_destroy(self.surface) };
    }
}

fn gradient_pattern(gradient: &Gradient, alpha: f32) -> *mut c_void {
    let point = |(x, y): (f32, f32)| (f64::from(x), f64::from(y));
    unsafe {
        let pattern = match gradient.shape {
            GradientShape::Linear { start, end } => {
                let ((x0, y0), (x1, y1)) = (point(start), point(end));
                cairo_pattern_create_linear(x0, y0, x1, y1)
            }
            GradientShape::Radial {
                focal,
                center,
                radius,
            } => {
                let ((fx, fy), (cx, cy)) = (point(focal), point(center));
                cairo_pattern_create_radial(fx, fy, 0.0, cx, cy, radius.into())
            }
        };
        for &(offset, color) in &gradient.stops {
            let (r, g, b, a) = color.components(alpha);
            cairo_pattern_add_color_stop_rgba(pattern, offset.into(), r, g, b, a);
        }
        cairo_pattern_set_extend(
            pattern,
            match gradient.spread {
                GradientSpread::Pad => CAIRO_EXTEND_PAD,
                GradientSpread::Repeat => 1,
                GradientSpread::Reflect => 2,
            },
        );
        // A pattern matrix maps user space to pattern space, the inverse of the gradient one.
        let [a, b, c, d, e, f] = gradient.transform.map(f64::from);
        let mut matrix = CairoMatrix {
            xx: a,
            yx: b,
            xy: c,
            yy: d,
            x0: e,
            y0: f,
        };
        if cairo_matrix_invert(&mut matrix) == 0 {
            cairo_pattern_set_matrix(pattern, &matrix);
        }
        pattern
    }
}

fn layout_extents(layout: *mut c_void) -> TextExtents {
    let (mut ink, mut logical) = (PangoRectangle::default(), PangoRectangle::default());
    let baseline = unsafe {
        pango_layout_get_extents(layout, &mut ink, &mut logical);
        pango_layout_get_baseline(layout)
    };
    let pixels = |value: i32| value as f32 / PANGO_SCALE as f32;
    let ascent = pixels(baseline);
    TextExtents {
        width: pixels(logical.width),
        ink: [
            pixels(ink.x),
            pixels(ink.y) - ascent,
            pixels(ink.x + ink.width),
            pixels(ink.y + ink.height) - ascent,
        ],
        ascent,
        descent: pixels(logical.height) - ascent,
    }
}

unsafe fn curve_to(cr: *mut c_void, [ax, ay]: [f32; 2], [bx, by]: [f32; 2], [x, y]: [f32; 2]) {
    unsafe {
        cairo_curve_to(
            cr,
            ax.into(),
            ay.into(),
            bx.into(),
            by.into(),
            x.into(),
            y.into(),
        )
    }
}

// Whether cairo applies an operator outside of what is drawn too.
const fn is_unbounded(operator: i32) -> bool {
    matches!(operator, 3 | 4 | 8 | 10)
}

const fn fill_rule(rule: FillRule) -> i32 {
    match rule {
        FillRule::NonZero => 0,
        FillRule::EvenOdd => 1,
    }
}

const fn operator(operation: CompositeOperation) -> i32 {
    use CompositeOperation::*;
    match operation {
        Copy => 1,
        SourceOver => 2,
        SourceIn => 3,
        SourceOut => 4,
        SourceAtop => 5,
        DestinationOver => 7,
        DestinationIn => 8,
        DestinationOut => 9,
        DestinationAtop => 10,
        Xor => 11,
        Lighter => 12,
        Multiply => 14,
        Screen => 15,
        Overlay => 16,
        Darken => 17,
        Lighten => 18,
        ColorDodge => 19,
        ColorBurn => 20,
        HardLight => 21,
        SoftLight => 22,
        Difference => 23,
        Exclusion => 24,
        Hue => 25,
        Saturation => 26,
        Color => 27,
        Luminosity => 28,
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::Color;

    struct Surface(*mut c_void, *mut c_void);

    impl Surface {
        fn new() -> Self {
            unsafe {
                let surface = cairo_image_surface_create(0, 4, 1);
                Self(surface, cairo_create(surface))
            }
        }

        fn pixels(&self) -> [u32; 4] {
            unsafe {
                cairo_surface_flush(self.0);
                std::ptr::read_unaligned(cairo_image_surface_get_data(self.0).cast())
            }
        }
    }

    impl Drop for Surface {
        fn drop(&mut self) {
            unsafe {
                cairo_destroy(self.1);
                cairo_surface_destroy(self.0);
            }
        }
    }

    #[test]
    fn singular_transforms_hide_drawing_until_reset_or_restore() {
        let surface = Surface::new();
        let mut canvas = unsafe { PlatformCanvasContext::new(surface.1) };
        let state = CanvasState {
            fill: Color::rgb(255, 0, 0).into(),
            ..CanvasState::default()
        };
        canvas.translate(1.0, 0.0);
        canvas.save();
        canvas.scale(0.0, 1.0);
        canvas.scale(2.0, 1.0);
        canvas.set_transform(1.0, 2.0, 2.0, 4.0, 0.0, 0.0);
        canvas.translate(f32::INFINITY, 0.0);
        canvas.rotate(f32::NAN);
        canvas.set_transform(1.0, 0.0, 0.0, 1.0, f32::NAN, 0.0);
        assert_eq!(unsafe { cairo_status(surface.1) }, 0);
        canvas.fill_rect(&state, 0.0, 0.0, 1.0, 1.0);
        canvas.restore();
        canvas.fill_rect(&state, 1.0, 0.0, 1.0, 1.0);
        canvas.save();
        canvas.scale(0.0, 1.0);
        canvas.reset_transform();
        canvas.fill_rect(&state, 0.0, 0.0, 1.0, 1.0);
        canvas.scale(0.0, 1.0);
        canvas.clip(&Path::default(), FillRule::NonZero);
        canvas.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        canvas.fill_rect(&state, 3.0, 0.0, 1.0, 1.0);
        canvas.restore();
        drop(canvas);
        assert_eq!(unsafe { cairo_status(surface.1) }, 0);
        assert_eq!(surface.pixels(), [0xffff0000, 0, 0xffff0000, 0]);
    }

    #[test]
    fn transforms_preserve_composition_and_initial_device_scale() {
        let surface = Surface::new();
        unsafe { cairo_scale(surface.1, 2.0, 2.0) };
        let mut canvas = unsafe { PlatformCanvasContext::new(surface.1) };
        canvas.translate(1.0, 2.0);
        canvas.scale(3.0, -4.0);
        let mut m = canvas.initial;
        unsafe { cairo_get_matrix(surface.1, &mut m) };
        assert_eq!([m.xx, m.yy, m.x0, m.y0], [6.0, -8.0, 2.0, 4.0]);
        canvas.set_transform(1.0, 0.0, 0.0, 1.0, 3.0, 4.0);
        unsafe { cairo_get_matrix(surface.1, &mut m) };
        assert_eq!([m.xx, m.yy, m.x0, m.y0], [2.0, 2.0, 6.0, 8.0]);
        canvas.scale(0.0001, 0.0001);
        unsafe { cairo_get_matrix(surface.1, &mut m) };
        assert!(m.xx > 0.0 && m.xx < 0.001);
        assert_eq!(unsafe { cairo_status(surface.1) }, 0);
    }
}
