/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[cfg(feature = "window")]
use bwindow::Window;

#[cfg(feature = "window")]
use crate::platforms::PlatformCanvas;
use crate::platforms::{PlatformCanvasContext, PlatformOffscreenCanvas};

/// A portable RGBA color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red component.
    pub red: u8,
    /// Green component.
    pub green: u8,
    /// Blue component.
    pub blue: u8,
    /// Alpha component.
    pub alpha: u8,
}

impl Color {
    /// Create an opaque color from a packed `0xRRGGBB` value.
    pub const fn from_rgb(rgb: u32) -> Self {
        Self::rgb(
            ((rgb >> 16) & 0xff) as u8,
            ((rgb >> 8) & 0xff) as u8,
            (rgb & 0xff) as u8,
        )
    }

    /// Create an opaque RGB color.
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha: 255,
        }
    }

    /// Create an RGBA color.
    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub(crate) fn components(self, global_alpha: f32) -> (f64, f64, f64, f64) {
        (
            f64::from(self.red) / 255.0,
            f64::from(self.green) / 255.0,
            f64::from(self.blue) / 255.0,
            f64::from(self.alpha) / 255.0 * f64::from(global_alpha.clamp(0.0, 1.0)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Color;

    #[test]
    fn saved_state_shares_font_storage_and_preserves_its_value() {
        let mut state = super::CanvasState::default();
        let saved = state.clone();
        assert!(super::Rc::ptr_eq(&state.font_family, &saved.font_family));
        state.font_family = "monospace".into();
        assert_eq!(saved.font_family.as_ref(), "sans-serif");
        assert_eq!(state.font_family.as_ref(), "monospace");
    }

    #[test]
    fn color_from_packed_rgb_uses_rrggbb_order() {
        assert_eq!(Color::from_rgb(0x12_34_56), Color::rgb(0x12, 0x34, 0x56));
        assert_eq!(Color::from_rgb(0), Color::rgba(0, 0, 0, 255));
    }

    #[test]
    fn offscreen_canvas_scales_drawing_and_preserves_scale_across_frames() {
        let mut canvas = super::OffscreenCanvas::with_scale_factor(4, 2, 2.625);
        assert_eq!((canvas.width(), canvas.height()), (11, 6));
        for _ in 0..2 {
            canvas.draw(|context| {
                assert_eq!((context.width(), context.height()), (4.0, 2.0));
                assert_eq!(context.scale_factor(), 2.625);
                context.clear_rect(0.0, 0.0, 4.0, 2.0);
                context.scale(3.0, 3.0);
                context.reset_transform();
                context.set_transform(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
                context.set_fill_style(Color::rgb(255, 0, 0));
                context.fill_rect(0.0, 0.0, 2.0, 2.0);
            });
            let pixels = canvas.pixels();
            // The interior spans five native pixels, not two logical pixels.
            assert_eq!(&pixels[4 * 4..5 * 4], &[255, 0, 0, 255]);
            assert_eq!(&pixels[7 * 4..8 * 4], &[0, 0, 0, 0]);
        }
    }

    #[test]
    fn offscreen_canvas_rejects_invalid_scale_factors() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(
                std::panic::catch_unwind(|| {
                    super::OffscreenCanvas::with_scale_factor(4, 2, scale);
                })
                .is_err()
            );
        }
    }

    #[test]
    fn offscreen_canvas_returns_straight_rgba_pixels() {
        let mut canvas = super::OffscreenCanvas::new(2, 1);
        canvas.draw(|context| {
            context.set_fill_style(Color::rgba(120, 80, 40, 128));
            context.fill_rect(0.0, 0.0, 1.0, 1.0);
        });
        let pixels = canvas.pixels();
        assert!((i16::from(pixels[0]) - 120).abs() <= 2);
        assert!((i16::from(pixels[1]) - 80).abs() <= 2);
        assert!((i16::from(pixels[2]) - 40).abs() <= 2);
        assert_eq!(pixels[3], 128);
        assert_eq!(&pixels[4..8], &[0, 0, 0, 0]);
        let previous = pixels.to_vec();
        assert_eq!(canvas.pixels(), previous);
        canvas.draw(|context| {
            context.clear_rect(0.0, 0.0, 2.0, 1.0);
            context.set_fill_style(Color::rgb(1, 2, 3));
            context.fill_rect(1.0, 0.0, 1.0, 1.0);
        });
        assert_eq!(canvas.pixels(), &[0, 0, 0, 0, 1, 2, 3, 255]);
        let bitmap = canvas.to_bitmap().expect("valid bitmap");
        assert_eq!((bitmap.width(), bitmap.height()), (2, 1));
        assert_eq!(bitmap.data(), &[0, 0, 0, 0, 1, 2, 3, 255]);
    }

    #[test]
    fn rounded_rectangles_preserve_winding_for_negative_dimensions() {
        for (width, height) in [(24.0, 24.0), (-24.0, 24.0), (24.0, -24.0), (-24.0, -24.0)] {
            let mut canvas = super::OffscreenCanvas::new(48, 48);
            canvas.draw(|ctx| {
                ctx.rect(2.0, 2.0, 44.0, 44.0);
                ctx.round_rect(
                    if width < 0.0 { 36.0 } else { 12.0 },
                    if height < 0.0 { 36.0 } else { 12.0 },
                    width,
                    height,
                    6.0,
                );
                ctx.fill();
            });
            let pixels = canvas.pixels();
            let alpha = |x: usize, y: usize| pixels[(y * 48 + x) * 4 + 3];
            assert_eq!(alpha(24, 24), if width * height < 0.0 { 0 } else { 255 });
            assert_eq!(alpha(4, 4), 255);
            assert_eq!(alpha(0, 0), 0);
        }
    }

    #[test]
    fn rounded_rectangle_clamps_radius_and_preserves_transform() {
        let mut canvas = super::OffscreenCanvas::new(48, 32);
        canvas.draw(|ctx| {
            ctx.translate(8.0, 4.0);
            ctx.round_rect(0.0, 0.0, 24.0, 24.0, 100.0);
            ctx.fill();
            ctx.fill_rect(32.0, 0.0, 4.0, 4.0);
        });
        let pixels = canvas.pixels();
        let alpha = |x: usize, y: usize| pixels[(y * 48 + x) * 4 + 3];
        assert_eq!(alpha(8, 4), 0);
        assert_eq!(alpha(20, 16), 255);
        assert_eq!(alpha(42, 6), 255);
        assert_eq!(alpha(34, 2), 0);
    }

    #[test]
    fn rounded_rectangle_leaves_current_point_at_its_origin() {
        let mut canvas = super::OffscreenCanvas::new(40, 40);
        canvas.draw(|ctx| {
            ctx.round_rect(20.0, 20.0, 12.0, 12.0, 6.0);
            ctx.line_to(20.0, 4.0);
            ctx.line_to(4.0, 4.0);
            ctx.close_path();
            ctx.fill();
        });
        assert_eq!(canvas.pixels()[(16 * 40 + 18) * 4 + 3], 255);
    }

    #[test]
    fn nonfinite_arguments_leave_path_and_transform_usable() {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut canvas = super::OffscreenCanvas::new(8, 4);
            canvas.draw(|ctx| {
                ctx.rect(0.0, 0.0, 2.0, 2.0);
                ctx.move_to(invalid, 0.0);
                ctx.line_to(0.0, invalid);
                ctx.rect(0.0, 0.0, invalid, 4.0);
                ctx.round_rect(0.0, 0.0, 4.0, 4.0, invalid);
                ctx.round_rect(invalid, 0.0, 4.0, 4.0, -1.0);
                ctx.arc(0.0, 0.0, invalid, 0.0, 1.0, false);
                ctx.arc(invalid, 0.0, -1.0, 0.0, 1.0, false);
                ctx.ellipse(0.0, 0.0, invalid, 1.0);
                ctx.ellipse(invalid, 0.0, -1.0, 1.0);
                ctx.quadratic_curve_to(0.0, invalid, 1.0, 1.0);
                ctx.bezier_curve_to(0.0, 0.0, 1.0, invalid, 2.0, 2.0);
                ctx.translate(invalid, 0.0);
                ctx.rotate(invalid);
                ctx.scale(1.0, invalid);
                ctx.set_transform(1.0, 0.0, 0.0, 1.0, invalid, 0.0);
                ctx.fill();
                ctx.fill_rect(0.0, 0.0, invalid, 4.0);
                ctx.stroke_rect(invalid, 0.0, 4.0, 4.0);
                ctx.clear_rect(0.0, 0.0, 8.0, invalid);
                ctx.fill_text("ignored", invalid, 0.0);
                ctx.fill_rect(6.0, 2.0, 2.0, 2.0);
            });
            let pixels = canvas.pixels();
            for y in 0..4 {
                for x in 0..8 {
                    assert_eq!(
                        pixels[(y * 8 + x) * 4 + 3],
                        if (x < 2 && y < 2) || (x >= 6 && y >= 2) {
                            255
                        } else {
                            0
                        }
                    );
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "rounded rectangle radius must be nonnegative")]
    fn rounded_rectangle_rejects_negative_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.round_rect(0.0, 0.0, 4.0, 4.0, -1.0));
    }

    #[test]
    #[should_panic(expected = "arc radius must be nonnegative")]
    fn arc_rejects_negative_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.arc(0.0, 0.0, -1.0, 0.0, 1.0, false));
    }

    #[test]
    #[should_panic(expected = "ellipse radii must be nonnegative")]
    fn ellipse_rejects_negative_horizontal_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.ellipse(0.0, 0.0, -1.0, 1.0));
    }

    #[test]
    #[should_panic(expected = "ellipse radii must be nonnegative")]
    fn ellipse_rejects_negative_vertical_radius() {
        super::OffscreenCanvas::new(4, 4).draw(|ctx| ctx.ellipse(0.0, 0.0, 1.0, -1.0));
    }
}

/// Canvas line cap style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineCap {
    /// Flat line ends.
    #[default]
    Butt,
    /// Round line ends.
    Round,
    /// Square line ends.
    Square,
}

/// Canvas line join style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum LineJoin {
    /// Mitered joins.
    #[default]
    Miter,
    /// Rounded joins.
    Round,
    /// Beveled joins.
    Bevel,
}

/// Horizontal text alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// Align at the text start.
    #[default]
    Start,
    /// Align at the text end.
    End,
    /// Align left.
    Left,
    /// Align right.
    Right,
    /// Center text.
    Center,
}

/// Vertical text alignment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextBaseline {
    /// Top edge.
    Top,
    /// Hanging baseline.
    Hanging,
    /// Middle.
    Middle,
    /// Alphabetic baseline.
    #[default]
    Alphabetic,
    /// Ideographic baseline.
    Ideographic,
    /// Bottom edge.
    Bottom,
}

/// Canvas font weight.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum FontWeight {
    /// Normal font weight.
    #[default]
    Normal,
    /// Bold font weight.
    Bold,
}

#[derive(Clone)]
pub(crate) struct CanvasState {
    pub fill: Color,
    pub stroke: Color,
    pub alpha: f32,
    pub line_width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub font_family: Rc<str>,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub text_align: TextAlign,
    pub text_baseline: TextBaseline,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            fill: Color::rgb(0, 0, 0),
            stroke: Color::rgb(0, 0, 0),
            alpha: 1.0,
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            font_family: "sans-serif".into(),
            font_size: 10.0,
            font_weight: FontWeight::Normal,
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
        }
    }
}

/// Builder for one native 2D canvas attached to a window.
#[cfg(feature = "window")]
pub struct CanvasBuilder<'a> {
    window: &'a Window,
}

#[cfg(feature = "window")]
impl<'a> CanvasBuilder<'a> {
    /// Create a canvas builder.
    pub const fn new(window: &'a Window) -> Self {
        Self { window }
    }

    /// Build the canvas, panicking if the window is closed or already has content.
    pub fn build(self) -> Canvas {
        self.try_build().expect("cannot attach canvas")
    }

    /// Build the canvas with a recoverable attachment error.
    pub fn try_build(self) -> Result<Canvas, bwindow::AttachError> {
        Ok(Canvas {
            platform: PlatformCanvas::new(self.window.attach_content()?),
        })
    }
}

/// A native 2D canvas. All access is confined to the main thread.
#[cfg(feature = "window")]
pub struct Canvas {
    platform: PlatformCanvas,
}

#[cfg(feature = "window")]
impl Canvas {
    /// Schedule a repaint. Repeated requests coalesce.
    pub fn request_redraw(&self) {
        self.platform.request_redraw();
    }

    /// Schedule one repaint at the next native display frame.
    pub fn request_animation_frame(&self) {
        self.platform.request_animation_frame();
    }

    /// Paint once during this canvas window's RedrawRequested callback.
    ///
    /// Returns false outside a native paint callback, after close, or if this frame
    /// was already drawn. The closure is not called in those cases. Native frame
    /// scheduling and recovery are handled by the library; callers must not assume
    /// that every redraw produced a completed draw. The context cannot escape the closure.
    ///
    /// Ignoring whether a frame was drawn is diagnosed by Rust:
    ///
    /// ```compile_fail
    /// #![deny(unused_must_use)]
    /// fn paint(canvas: &mut bcanvas::Canvas) {
    ///     canvas.draw(|_| {});
    /// }
    /// ```
    #[must_use = "only update frame-dependent state when the draw closure ran"]
    pub fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        self.platform.draw(draw)
    }
}

/// A thread-bound native 2D canvas backed by tightly packed, straight-alpha RGBA8 pixels.
pub struct OffscreenCanvas {
    platform: PlatformOffscreenCanvas,
    width: u32,
    height: u32,
    scale_factor: f32,
    logical_size: (u32, u32),
}

impl OffscreenCanvas {
    /// Create a transparent offscreen canvas with one logical pixel per output pixel.
    pub fn new(width: u32, height: u32) -> Self {
        Self::with_scale_factor(width, height, 1.0)
    }

    /// Create a canvas with logical dimensions and a native pixel scale.
    /// Bitmap dimensions are rounded up; drawing coordinates remain logical pixels.
    pub fn with_scale_factor(width: u32, height: u32, scale_factor: f32) -> Self {
        assert!(
            scale_factor.is_finite() && scale_factor > 0.0,
            "invalid canvas scale factor"
        );
        let logical_size = (width, height);
        let width = (f64::from(width) * f64::from(scale_factor)).ceil();
        let height = (f64::from(height) * f64::from(scale_factor)).ceil();
        assert!(
            width <= f64::from(i32::MAX) && height <= f64::from(i32::MAX),
            "canvas dimensions are too large"
        );
        let (width, height) = (width as u32, height as u32);
        assert!(
            width > 0 && height > 0 && width <= i32::MAX as u32 && height <= i32::MAX as u32,
            "canvas dimensions must be nonzero and fit in i32"
        );
        let length = width
            .checked_mul(height)
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .expect("canvas dimensions are too large");
        Self {
            platform: PlatformOffscreenCanvas::new(width, height, length),
            width,
            height,
            scale_factor,
            logical_size,
        }
    }

    /// Native pixels per logical pixel.
    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    /// Width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Number of bytes between adjacent rows.
    pub const fn bytes_per_row(&self) -> u32 {
        self.width * 4
    }

    /// Draw into the bitmap with the same API as a window canvas.
    pub fn draw(&mut self, draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>)) {
        let platform = self.platform.begin_draw(self.scale_factor);
        let mut context = CanvasRenderingContext2d::new(
            platform,
            self.logical_size.0 as f32,
            self.logical_size.1 as f32,
            self.scale_factor,
        );
        draw(&mut context);
        drop(context);
        self.platform.end_draw();
    }

    /// Return tightly packed pixels in RGBA byte order with straight alpha.
    pub fn pixels(&mut self) -> &[u8] {
        self.platform.pixels()
    }

    /// Copy the current pixels into an image bitmap for encoding or further processing.
    pub fn to_bitmap(&mut self) -> Result<image::Bitmap, image::EncodeError> {
        image::Bitmap::new(self.width, self.height, self.pixels().to_vec())
    }
}

#[cfg(not(any(target_vendor = "apple", target_os = "android")))]
pub(crate) fn copy_bgra_premultiplied_to_rgba(source: &[u8], output: &mut [u8]) {
    let (source, source_remainder) = source.as_chunks::<4>();
    let (output, output_remainder) = output.as_chunks_mut::<4>();
    debug_assert!(source_remainder.is_empty() && output_remainder.is_empty());
    for (source, output) in source.iter().zip(output) {
        let alpha = source[3];
        output[0] = unpremultiply(source[2], alpha);
        output[1] = unpremultiply(source[1], alpha);
        output[2] = unpremultiply(source[0], alpha);
        output[3] = alpha;
    }
}

#[cfg(any(target_vendor = "apple", target_os = "android"))]
pub(crate) fn copy_rgba_premultiplied_to_straight(source: &[u8], output: &mut [u8]) {
    let (source, source_remainder) = source.as_chunks::<4>();
    let (output, output_remainder) = output.as_chunks_mut::<4>();
    debug_assert!(source_remainder.is_empty() && output_remainder.is_empty());
    for (source, output) in source.iter().zip(output) {
        let alpha = source[3];
        output[0] = unpremultiply(source[0], alpha);
        output[1] = unpremultiply(source[1], alpha);
        output[2] = unpremultiply(source[2], alpha);
        output[3] = alpha;
    }
}

fn unpremultiply(component: u8, alpha: u8) -> u8 {
    if alpha == 0 {
        0
    } else {
        ((u16::from(component) * 255 + u16::from(alpha) / 2) / u16::from(alpha)).min(255) as u8
    }
}

/// A frame-scoped HTML5 Canvas 2D-like drawing context.
///
/// A borrowed context cannot be retained beyond its draw closure:
///
/// ```compile_fail
/// let mut canvas = bcanvas::OffscreenCanvas::new(1, 1);
/// let mut saved = None;
/// canvas.draw(|context| saved = Some(context));
/// ```
///
/// Drawing contexts cannot be sent to another thread, even within the frame:
///
/// ```compile_fail
/// let mut canvas = bcanvas::OffscreenCanvas::new(1, 1);
/// canvas.draw(|context| {
///     std::thread::scope(|scope| {
///         scope.spawn(move || context.fill_rect(0.0, 0.0, 1.0, 1.0));
///     });
/// });
/// ```
pub struct CanvasRenderingContext2d<'a> {
    pub(crate) platform: PlatformCanvasContext,
    pub(crate) state: CanvasState,
    pub(crate) stack: Vec<CanvasState>,
    width: f32,
    height: f32,
    scale_factor: f32,
    timestamp: Duration,
    _lifetime: PhantomData<(&'a mut (), Rc<()>)>,
}

impl<'a> CanvasRenderingContext2d<'a> {
    pub(crate) fn new(
        platform: PlatformCanvasContext,
        width: f32,
        height: f32,
        scale_factor: f32,
    ) -> Self {
        static START: OnceLock<Instant> = OnceLock::new();
        Self {
            platform,
            state: CanvasState::default(),
            stack: Vec::new(),
            width,
            height,
            scale_factor,
            timestamp: START.get_or_init(Instant::now).elapsed(),
            _lifetime: PhantomData,
        }
    }
    /// Width of this frame in logical pixels.
    pub const fn width(&self) -> f32 {
        self.width
    }
    /// Height of this frame in logical pixels.
    pub const fn height(&self) -> f32 {
        self.height
    }
    /// Native pixels per logical pixel for this frame.
    pub const fn scale_factor(&self) -> f32 {
        self.scale_factor
    }
    /// Monotonic time since the first canvas frame in this process.
    pub const fn timestamp(&self) -> Duration {
        self.timestamp
    }
    /// Save styles, the transform, and the clipping region, but not the current path.
    pub fn save(&mut self) {
        self.platform.save();
        self.stack.push(self.state.clone());
    }
    /// Restore the most recently saved drawing state. An empty stack is a no-op.
    pub fn restore(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.platform.restore();
            self.state = state;
        }
    }
    /// Current fill color.
    pub const fn fill_style(&self) -> Color {
        self.state.fill
    }
    /// Set the fill color.
    pub const fn set_fill_style(&mut self, color: Color) {
        self.state.fill = color;
    }
    /// Current stroke color.
    pub const fn stroke_style(&self) -> Color {
        self.state.stroke
    }
    /// Set the stroke color.
    pub const fn set_stroke_style(&mut self, color: Color) {
        self.state.stroke = color;
    }
    /// Current global alpha, from 0.0 to 1.0.
    pub const fn global_alpha(&self) -> f32 {
        self.state.alpha
    }
    /// Set global alpha. Non-finite values and values outside 0.0..=1.0 are ignored.
    pub const fn set_global_alpha(&mut self, alpha: f32) {
        if alpha.is_finite() && alpha >= 0.0 && alpha <= 1.0 {
            self.state.alpha = alpha;
        }
    }
    /// Current line width in logical pixels.
    pub const fn line_width(&self) -> f32 {
        self.state.line_width
    }
    /// Set line width. Non-finite, zero, and negative values are ignored.
    pub const fn set_line_width(&mut self, width: f32) {
        if width.is_finite() && width > 0.0 {
            self.state.line_width = width;
        }
    }
    /// Current line cap.
    pub const fn line_cap(&self) -> LineCap {
        self.state.line_cap
    }
    /// Set line cap.
    pub const fn set_line_cap(&mut self, cap: LineCap) {
        self.state.line_cap = cap;
    }
    /// Current line join.
    pub const fn line_join(&self) -> LineJoin {
        self.state.line_join
    }
    /// Set line join.
    pub const fn set_line_join(&mut self, join: LineJoin) {
        self.state.line_join = join;
    }
    /// Current font family.
    pub fn font_family(&self) -> &str {
        &self.state.font_family
    }
    /// Current font size in logical pixels.
    pub const fn font_size(&self) -> f32 {
        self.state.font_size
    }
    /// Set text font family and size in logical pixels, rather than a CSS font string.
    /// Non-finite, zero, and negative sizes leave both properties unchanged.
    pub fn set_font(&mut self, family: impl Into<String>, size: f32) {
        if !size.is_finite() || size <= 0.0 {
            return;
        }
        let family = family.into();
        if self.state.font_family.as_ref() != family {
            self.state.font_family = family.into();
        }
        self.state.font_size = size;
    }
    /// Current font weight.
    pub const fn font_weight(&self) -> FontWeight {
        self.state.font_weight
    }
    /// Set text font weight.
    pub const fn set_font_weight(&mut self, weight: FontWeight) {
        self.state.font_weight = weight;
    }
    /// Current horizontal text alignment.
    pub const fn text_align(&self) -> TextAlign {
        self.state.text_align
    }
    /// Set horizontal text alignment.
    pub const fn set_text_align(&mut self, align: TextAlign) {
        self.state.text_align = align;
    }
    /// Current text baseline.
    pub const fn text_baseline(&self) -> TextBaseline {
        self.state.text_baseline
    }
    /// Set vertical text baseline.
    pub const fn set_text_baseline(&mut self, baseline: TextBaseline) {
        self.state.text_baseline = baseline;
    }
    /// Clear a rectangle.
    pub fn clear_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.clear_rect(x, y, width, height);
    }
    /// Fill a rectangle.
    pub fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.fill_rect(&self.state, x, y, width, height);
    }
    /// Stroke a rectangle.
    pub fn stroke_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.stroke_rect(&self.state, x, y, width, height);
    }
    /// Begin a new path.
    pub fn begin_path(&mut self) {
        self.platform.begin_path();
    }
    /// Close the current path.
    pub fn close_path(&mut self) {
        self.platform.close_path();
    }
    /// Move the current point.
    pub fn move_to(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.move_to(x, y);
    }
    /// Add a line segment.
    pub fn line_to(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.line_to(x, y);
    }
    /// Add a rectangle to the path.
    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.rect(x, y, width, height);
    }
    /// Add a rounded rectangle to the current path.
    /// Negative dimensions reverse the corresponding axis. Non-finite arguments are ignored.
    ///
    /// # Panics
    /// Panics if the radius is negative and all arguments are finite.
    pub fn round_rect(&mut self, x: f32, y: f32, width: f32, height: f32, radius: f32) {
        if ![x, y, width, height, radius].iter().all(|v| v.is_finite()) {
            return;
        }
        assert!(
            radius >= 0.0,
            "rounded rectangle radius must be nonnegative"
        );
        let radius = radius.min(width.abs() / 2.0).min(height.abs() / 2.0);
        let rx = radius.copysign(width);
        let ry = radius.copysign(height);
        // Cubic approximation of a quarter circle, shared by every native backend.
        let k = 4.0 / 3.0 * (std::f32::consts::FRAC_PI_2 / 4.0).tan();
        let (right, bottom) = (x + width, y + height);
        self.move_to(x + rx, y);
        self.line_to(right - rx, y);
        self.bezier_curve_to(
            right - rx + k * rx,
            y,
            right,
            y + ry - k * ry,
            right,
            y + ry,
        );
        self.line_to(right, bottom - ry);
        self.bezier_curve_to(
            right,
            bottom - ry + k * ry,
            right - rx + k * rx,
            bottom,
            right - rx,
            bottom,
        );
        self.line_to(x + rx, bottom);
        self.bezier_curve_to(
            x + rx - k * rx,
            bottom,
            x,
            bottom - ry + k * ry,
            x,
            bottom - ry,
        );
        self.line_to(x, y + ry);
        self.bezier_curve_to(x, y + ry - k * ry, x + rx - k * rx, y, x + rx, y);
        self.close_path();
        self.move_to(x, y);
    }
    /// Add a complete, axis-aligned ellipse to the current path.
    /// Unlike browser Canvas 2D, this does not accept rotation or arc angles.
    /// Non-finite arguments are ignored.
    ///
    /// # Panics
    /// Panics if either radius is negative and all arguments are finite.
    pub fn ellipse(&mut self, x: f32, y: f32, radius_x: f32, radius_y: f32) {
        if ![x, y, radius_x, radius_y].iter().all(|v| v.is_finite()) {
            return;
        }
        assert!(
            radius_x >= 0.0 && radius_y >= 0.0,
            "ellipse radii must be nonnegative"
        );
        self.platform.ellipse(x, y, radius_x, radius_y);
    }
    /// Add a circular arc, with angles in radians.
    /// Non-finite arguments are ignored.
    ///
    /// # Panics
    /// Panics if the radius is negative and all arguments are finite.
    pub fn arc(
        &mut self,
        x: f32,
        y: f32,
        radius: f32,
        start: f32,
        end: f32,
        counterclockwise: bool,
    ) {
        if ![x, y, radius, start, end].iter().all(|v| v.is_finite()) {
            return;
        }
        assert!(radius >= 0.0, "arc radius must be nonnegative");
        self.platform
            .arc(x, y, radius, start, end, counterclockwise);
    }
    /// Add a quadratic Bezier segment.
    pub fn quadratic_curve_to(&mut self, cpx: f32, cpy: f32, x: f32, y: f32) {
        if ![cpx, cpy, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.quadratic_curve_to(cpx, cpy, x, y);
    }
    /// Add a cubic Bezier segment.
    pub fn bezier_curve_to(&mut self, cp1x: f32, cp1y: f32, cp2x: f32, cp2y: f32, x: f32, y: f32) {
        if ![cp1x, cp1y, cp2x, cp2y, x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y);
    }
    /// Fill the current path without consuming it.
    pub fn fill(&mut self) {
        self.platform.fill(&self.state);
    }
    /// Stroke the current path without consuming it.
    pub fn stroke(&mut self) {
        self.platform.stroke(&self.state);
    }
    /// Clip future drawing to the current path without consuming it.
    pub fn clip(&mut self) {
        self.platform.clip();
    }
    /// Translate the current transform.
    pub fn translate(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.translate(x, y);
    }
    /// Rotate the current transform by radians.
    pub fn rotate(&mut self, angle: f32) {
        if !angle.is_finite() {
            return;
        }
        self.platform.rotate(angle);
    }
    /// Scale the current transform.
    pub fn scale(&mut self, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.scale(x, y);
    }
    /// Replace the current affine transform.
    pub fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        if ![a, b, c, d, e, f].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.set_transform(a, b, c, d, e, f);
    }
    /// Reset the current transform.
    pub fn reset_transform(&mut self) {
        self.platform.reset_transform();
    }
    /// Draw filled text.
    pub fn fill_text(&mut self, text: impl AsRef<str>, x: f32, y: f32) {
        if ![x, y].iter().all(|v| v.is_finite()) {
            return;
        }
        self.platform.fill_text(&self.state, text.as_ref(), x, y);
    }
    /// Measure text width in logical pixels.
    pub fn measure_text(&mut self, text: impl AsRef<str>) -> f32 {
        self.platform.measure_text(&self.state, text.as_ref())
    }
}

impl Drop for CanvasRenderingContext2d<'_> {
    fn drop(&mut self) {
        while self.stack.pop().is_some() {
            self.platform.restore();
        }
    }
}
