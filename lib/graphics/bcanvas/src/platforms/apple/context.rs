/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Arc;

use super::headers::*;
use crate::cache::{GradientKey, LruCache};
use crate::path::{Command, Path};
use crate::{
    CanvasState, CanvasStyle, Color, CompositeOperation, FillRule, FontWeight, Gradient,
    GradientShape, GradientSpread, LineCap, LineJoin, TextExtents, single_line,
};

const MAX_TEXT_STYLES: usize = 64;

pub(crate) struct PlatformCanvasContext {
    context: CGContextRef,
    initial_transform: CGAffineTransform,
    resources: Resources,
    // Whether each open layer is masked.
    layers: Vec<bool>,
    // Core Graphics cannot leave a singular CTM, so it is not applied and drawing is skipped.
    singular: bool,
    // The singular flag of each saved graphics state.
    saved: Vec<bool>,
}

const GRADIENTS: usize = 32;
const PATHS: usize = 64;

/// Text attributes, gradients and paths shared by all frames of one canvas.
#[derive(Clone)]
pub(super) struct Resources(Rc<ResourcesInner>);

// A gradient with the periods of its spread stops, or [0, 0] for its own stops.
type GradientPeriods = (GradientKey, [i32; 2]);

pub(super) struct ResourcesInner {
    text_styles: RefCell<HashMap<TextStyleKey, Rc<TextStyle>>>,
    gradients: RefCell<LruCache<GradientPeriods, Rc<NativeGradient>>>,
    paths: RefCell<LruCache<u64, NativePath>>,
}

impl Default for Resources {
    fn default() -> Self {
        Self(Rc::new(ResourcesInner {
            text_styles: RefCell::default(),
            gradients: RefCell::new(LruCache::new(GRADIENTS)),
            paths: RefCell::new(LruCache::new(PATHS)),
        }))
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct TextStyleKey {
    family: Arc<str>,
    size: u32,
    weight: FontWeight,
}

/// Core Text attributes, the text color comes from the context fill color.
struct TextStyle {
    attributes: CFObject,
    ascent: f32,
    descent: f32,
}

impl Resources {
    fn text_style(&self, s: &CanvasState) -> Rc<TextStyle> {
        let key = TextStyleKey {
            family: s.font_family.clone(),
            size: s.font_size.to_bits(),
            weight: s.font_weight,
        };
        let mut styles = self.0.text_styles.borrow_mut();
        if !styles.contains_key(&key) && styles.len() >= MAX_TEXT_STYLES {
            styles.clear();
        }
        styles
            .entry(key)
            .or_insert_with(|| Rc::new(create_text_style(s)))
            .clone()
    }

    // The native gradient of the spread stops of periods, or [0, 0] for its own stops.
    fn gradient(&self, gradient: &Arc<Gradient>, periods: [i32; 2]) -> Rc<NativeGradient> {
        let key = (GradientKey(gradient.clone()), periods);
        self.0
            .gradients
            .borrow_mut()
            .get_or_create(key, || {
                let stops = if periods == [0, 0] {
                    gradient.stops.clone()
                } else {
                    gradient.spread_stops(periods[0], periods[1])
                };
                Some(Rc::new(NativeGradient::new(&stops)))
            })
            .cloned()
            .expect("created gradient")
    }

    /// Replace the current path of a context, with the cached path of a reused one.
    unsafe fn add_path(&self, context: CGContextRef, path: &Path) {
        unsafe {
            CGContextBeginPath(context);
            if path.id == 0 {
                build_path(path, |command| match command {
                    PathCommand::Move(x, y) => CGContextMoveToPoint(context, x, y),
                    PathCommand::Line(x, y) => CGContextAddLineToPoint(context, x, y),
                    PathCommand::Quad(cx, cy, x, y) => {
                        CGContextAddQuadCurveToPoint(context, cx, cy, x, y)
                    }
                    PathCommand::Cubic(ax, ay, bx, by, x, y) => {
                        CGContextAddCurveToPoint(context, ax, ay, bx, by, x, y)
                    }
                    PathCommand::Close => CGContextClosePath(context),
                });
                return;
            }
            let mut paths = self.0.paths.borrow_mut();
            let native = paths.get_or_create(path.id, || {
                let native = CGPathCreateMutable();
                let t = null();
                build_path(path, |command| match command {
                    PathCommand::Move(x, y) => CGPathMoveToPoint(native, t, x, y),
                    PathCommand::Line(x, y) => CGPathAddLineToPoint(native, t, x, y),
                    PathCommand::Quad(cx, cy, x, y) => {
                        CGPathAddQuadCurveToPoint(native, t, cx, cy, x, y)
                    }
                    PathCommand::Cubic(ax, ay, bx, by, x, y) => {
                        CGPathAddCurveToPoint(native, t, ax, ay, bx, by, x, y)
                    }
                    PathCommand::Close => CGPathCloseSubpath(native),
                });
                Some(NativePath(native))
            });
            if let Some(native) = native {
                CGContextAddPath(context, native.0);
            }
        }
    }
}

/// An owned Core Graphics path.
struct NativePath(*mut c_void);

impl Drop for NativePath {
    fn drop(&mut self) {
        unsafe { CGPathRelease(self.0) };
    }
}

// A path command with the Core Graphics coordinates of its points.
enum PathCommand {
    Move(f64, f64),
    Line(f64, f64),
    Quad(f64, f64, f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
    Close,
}

fn build_path(path: &Path, mut add: impl FnMut(PathCommand)) {
    for command in &path.commands {
        add(match *command {
            Command::Move([x, y]) => PathCommand::Move(x.into(), y.into()),
            Command::Line([x, y]) => PathCommand::Line(x.into(), y.into()),
            Command::Quadratic([cx, cy], [x, y]) => {
                PathCommand::Quad(cx.into(), cy.into(), x.into(), y.into())
            }
            Command::Cubic([ax, ay], [bx, by], [x, y]) => PathCommand::Cubic(
                ax.into(),
                ay.into(),
                bx.into(),
                by.into(),
                x.into(),
                y.into(),
            ),
            Command::Close => PathCommand::Close,
        });
    }
}

impl Drop for PlatformCanvasContext {
    fn drop(&mut self) {
        unsafe {
            CGContextBeginPath(self.context);
            CGContextRestoreGState(self.context);
        }
    }
}

impl PlatformCanvasContext {
    pub(super) unsafe fn new(context: CGContextRef, resources: Resources) -> Self {
        unsafe {
            CGContextSaveGState(context);
            CGContextBeginPath(context);
        }
        Self {
            context,
            initial_transform: unsafe { CGContextGetCTM(context) },
            resources,
            layers: Vec::new(),
            singular: false,
            saved: Vec::new(),
        }
    }

    pub(crate) unsafe fn from_native(context: crate::NativeContext) -> Self {
        let crate::NativeContext::CoreGraphics(context) = context;
        unsafe { Self::new(context, Resources::default()) }
    }

    pub(crate) fn save(&mut self) {
        self.saved.push(self.singular);
        unsafe { CGContextSaveGState(self.context) }
    }
    pub(crate) fn restore(&mut self) {
        self.singular = self.saved.pop().unwrap_or(false);
        unsafe { CGContextRestoreGState(self.context) }
    }
    pub(crate) fn begin_layer(&mut self, s: &CanvasState) {
        self.begin_transparency_layer(s, false);
    }
    pub(crate) fn begin_mask_layer(&mut self, s: &CanvasState) {
        self.begin_transparency_layer(s, true);
    }
    /// The content of a masked layer is a nested layer that composites into the mask.
    pub(crate) fn end_mask(&mut self) {
        self.saved.push(self.singular);
        unsafe {
            CGContextSaveGState(self.context);
            CGContextSetAlpha(self.context, 1.0);
            CGContextSetBlendMode(self.context, blend_mode(CompositeOperation::SourceIn));
            CGContextBeginTransparencyLayer(self.context, null());
        }
    }
    pub(crate) fn end_layer(&mut self) {
        let masked = self.layers.pop().expect("open layer");
        if masked {
            self.saved.pop();
        }
        self.singular = self.saved.pop().unwrap_or(false);
        unsafe {
            if masked {
                CGContextEndTransparencyLayer(self.context);
                CGContextRestoreGState(self.context);
            }
            CGContextEndTransparencyLayer(self.context);
            CGContextRestoreGState(self.context);
        }
    }
    fn begin_transparency_layer(&mut self, s: &CanvasState, masked: bool) {
        self.layers.push(masked);
        self.saved.push(self.singular);
        unsafe {
            begin_paint(self.context, s);
            CGContextBeginTransparencyLayer(self.context, null());
        }
    }
    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        if self.singular {
            return;
        }
        unsafe { CGContextClearRect(self.context, rect(x, y, w, h)) };
    }
    pub(crate) fn fill_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        if self.singular {
            return;
        }
        // A color fills the rect directly, without building a path.
        let CanvasStyle::Color(color) = s.fill else {
            self.paint_rect(s, PaintMode::Fill(FillRule::NonZero), x, y, w, h);
            return;
        };
        unsafe {
            begin_paint(self.context, s);
            set_color(self.context, color, false);
            CGContextFillRect(self.context, rect(x, y, w, h));
            CGContextRestoreGState(self.context);
        }
    }
    pub(crate) fn stroke_rect(&mut self, s: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        self.paint_rect(s, PaintMode::Stroke, x, y, w, h);
    }
    fn paint_rect(&mut self, s: &CanvasState, mode: PaintMode, x: f32, y: f32, w: f32, h: f32) {
        if self.singular {
            return;
        }
        unsafe {
            CGContextBeginPath(self.context);
            CGContextAddRect(self.context, rect(x, y, w, h));
            paint_path(self.context, &self.resources, s, mode);
        }
    }
    pub(crate) fn fill(&mut self, s: &CanvasState, path: &Path, rule: FillRule) {
        if self.singular {
            return;
        }
        unsafe {
            self.resources.add_path(self.context, path);
            paint_path(self.context, &self.resources, s, PaintMode::Fill(rule));
        }
    }
    pub(crate) fn stroke(&mut self, s: &CanvasState, path: &Path) {
        if self.singular {
            return;
        }
        unsafe {
            self.resources.add_path(self.context, path);
            paint_path(self.context, &self.resources, s, PaintMode::Stroke);
        }
    }
    pub(crate) fn clip(&mut self, path: &Path, rule: FillRule) {
        unsafe {
            // Core Graphics ignores a clip to an empty path, which hides everything.
            if path.commands.is_empty() {
                CGContextClipToRect(self.context, rect(0.0, 0.0, 0.0, 0.0));
            } else {
                self.resources.add_path(self.context, path);
                clip_path(self.context, rule);
            }
        }
    }
    pub(crate) fn draw_image(
        &mut self,
        s: &CanvasState,
        image: &PlatformImage,
        source: [f32; 4],
        [x, y, w, h]: [f32; 4],
    ) {
        if self.singular {
            return;
        }
        unsafe {
            begin_paint(self.context, s);
            CGContextSetInterpolationQuality(
                self.context,
                if s.image_smoothing {
                    K_CG_INTERPOLATION_HIGH
                } else {
                    K_CG_INTERPOLATION_NONE
                },
            );
            // The context is flipped, so flip the image back upright.
            CGContextTranslateCTM(self.context, x.into(), f64::from(y) + f64::from(h));
            CGContextScaleCTM(self.context, 1.0, -1.0);
            // A source smaller than the image is a sub-image sharing its pixels.
            let [sx, sy, sw, sh] = source;
            let full = source == [0.0, 0.0, image.width as f32, image.height as f32];
            let sub_image = if full {
                null_mut()
            } else {
                CGImageCreateWithImageInRect(image.image, rect(sx, sy, sw, sh))
            };
            CGContextDrawImage(
                self.context,
                rect(0.0, 0.0, w, h),
                if sub_image.is_null() {
                    image.image
                } else {
                    sub_image
                },
            );
            if !sub_image.is_null() {
                CGImageRelease(sub_image);
            }
            CGContextRestoreGState(self.context);
        }
    }
    pub(crate) fn translate(&mut self, x: f32, y: f32) {
        self.concat(affine([1.0, 0.0, 0.0, 1.0, x, y]));
    }
    pub(crate) fn rotate(&mut self, a: f32) {
        let (sin, cos) = a.sin_cos();
        self.concat(affine([cos, sin, -sin, cos, 0.0, 0.0]));
    }
    pub(crate) fn scale(&mut self, x: f32, y: f32) {
        self.concat(affine([x, 0.0, 0.0, y, 0.0, 0.0]));
    }
    pub(crate) fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.concat(affine([a, b, c, d, e, f]));
    }
    // A singular transform stays singular until it is replaced.
    fn concat(&mut self, transform: CGAffineTransform) {
        if self.singular {
            return;
        }
        let ctm = unsafe { CGAffineTransformConcat(transform, CGContextGetCTM(self.context)) };
        self.singular = !invertible(ctm);
        if !self.singular {
            unsafe { CGContextConcatCTM(self.context, transform) };
        }
    }
    pub(crate) fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        let transform = affine([a, b, c, d, e, f]);
        self.set_ctm(unsafe { CGAffineTransformConcat(transform, self.initial_transform) });
    }
    pub(crate) fn reset_transform(&mut self) {
        self.set_ctm(self.initial_transform);
    }
    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        if self.singular {
            return;
        }
        let style = self.resources.text_style(s);
        let line = text_line(text, &style);
        // Core Text draws from the alphabetic baseline.
        let width = line_width(&line);
        let [dx, dy] = TextExtents {
            width,
            ascent: style.ascent,
            descent: style.descent,
            ..TextExtents::default()
        }
        .origin(s);
        let (x, y) = (x + dx, y + dy);
        unsafe {
            begin_paint(self.context, s);
            // The context is flipped, so flip the glyphs back upright.
            CGContextSetTextMatrix(self.context, affine([1.0, 0.0, 0.0, -1.0, 0.0, 0.0]));
            CGContextSetTextPosition(self.context, x.into(), y.into());
            match &s.fill {
                CanvasStyle::Color(color) => {
                    set_color(self.context, *color, false);
                    CTLineDraw(line.as_ptr(), self.context);
                }
                // Gradients fill the glyphs in a layer bounded by the line, as Core Text
                // ignores the clip drawing mode.
                CanvasStyle::Gradient(gradient) if !gradient.0.stops.is_empty() => {
                    let margin = s.font_size;
                    CGContextClipToRect(
                        self.context,
                        rect(
                            x - margin,
                            y - style.ascent - margin,
                            width + margin * 2.0,
                            style.ascent + style.descent + margin * 2.0,
                        ),
                    );
                    CGContextBeginTransparencyLayer(self.context, null());
                    CGContextSetAlpha(self.context, 1.0);
                    CGContextSetBlendMode(self.context, blend_mode(CompositeOperation::SourceOver));
                    set_color(self.context, Color::rgb(0, 0, 0), false);
                    CTLineDraw(line.as_ptr(), self.context);
                    CGContextSetBlendMode(self.context, blend_mode(CompositeOperation::SourceIn));
                    draw_gradient(self.context, &self.resources, &gradient.0);
                    CGContextEndTransparencyLayer(self.context);
                }
                // A gradient without stops paints nothing.
                CanvasStyle::Gradient(_) => {}
            }
            CGContextRestoreGState(self.context);
        }
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> TextExtents {
        const K_CT_LINE_BOUNDS_USE_GLYPH_PATH_BOUNDS: usize = 1 << 3;
        let style = self.resources.text_style(s);
        let line = text_line(text, &style);
        // The glyph bounds are above the baseline with y up.
        let ink = unsafe {
            CTLineGetBoundsWithOptions(line.as_ptr(), K_CT_LINE_BOUNDS_USE_GLYPH_PATH_BOUNDS)
        };
        let (left, bottom) = (ink.origin.x as f32, -ink.origin.y as f32);
        TextExtents {
            width: line_width(&line),
            ink: [
                left,
                bottom - ink.size.height as f32,
                left + ink.size.width as f32,
                bottom,
            ],
            ascent: style.ascent,
            descent: style.descent,
        }
    }
    /// Replace the CTM with public API only, CGContextSetCTM is private, which needs the
    /// current CTM to be invertible.
    fn set_ctm(&mut self, transform: CGAffineTransform) {
        self.singular = !invertible(transform);
        if self.singular {
            return;
        }
        unsafe {
            let current = CGAffineTransformInvert(CGContextGetCTM(self.context));
            CGContextConcatCTM(self.context, CGAffineTransformConcat(transform, current));
        }
    }
}

/// A Core Graphics image sharing the pixels of a canvas image.
pub(crate) struct PlatformImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    image: *mut c_void,
}

impl PlatformImage {
    pub(crate) fn new(width: u32, height: u32, pixels: Arc<[u8]>) -> Self {
        unsafe extern "C" fn release(info: *mut c_void, _: *const c_void, _: usize) {
            drop(unsafe { Box::from_raw(info.cast::<Arc<[u8]>>()) });
        }
        let (data, length) = (pixels.as_ptr(), pixels.len());
        unsafe {
            let provider = CGDataProviderCreateWithData(
                Box::into_raw(Box::new(pixels)).cast(),
                data.cast(),
                length,
                release,
            );
            assert!(!provider.is_null(), "Could not create image data provider");
            let space = srgb_color_space();
            let image = CGImageCreate(
                width as usize,
                height as usize,
                8,
                32,
                width as usize * 4,
                space,
                K_CG_IMAGE_PREMULTIPLIED_BGRA,
                provider,
                null(),
                true,
                0,
            );
            CGDataProviderRelease(provider);
            assert!(!image.is_null(), "Could not create image");
            Self {
                width,
                height,
                image,
            }
        }
    }
}

impl Drop for PlatformImage {
    fn drop(&mut self) {
        unsafe { CGImageRelease(self.image) };
    }
}

#[derive(Clone, Copy)]
enum PaintMode {
    Fill(FillRule),
    Stroke,
}

/// Set the fill or stroke color as sRGB.
unsafe fn set_color(context: CGContextRef, color: Color, stroke: bool) {
    let (r, g, b, a) = color.components(1.0);
    let components = [r, g, b, a];
    unsafe {
        if stroke {
            CGContextSetStrokeColorSpace(context, srgb_color_space());
            CGContextSetStrokeColor(context, components.as_ptr());
        } else {
            CGContextSetFillColorSpace(context, srgb_color_space());
            CGContextSetFillColor(context, components.as_ptr());
        }
    }
}

/// Save the state and apply the global alpha and composite operation.
unsafe fn begin_paint(context: CGContextRef, s: &CanvasState) {
    unsafe {
        CGContextSaveGState(context);
        CGContextSetAlpha(context, s.alpha.into());
        CGContextSetBlendMode(context, blend_mode(s.composite));
    }
}

/// Paint the current path with the fill or stroke style, consuming it.
unsafe fn paint_path(
    context: CGContextRef,
    resources: &Resources,
    s: &CanvasState,
    mode: PaintMode,
) {
    unsafe {
        begin_paint(context, s);
        let style = match mode {
            PaintMode::Fill(_) => &s.fill,
            PaintMode::Stroke => {
                CGContextSetLineWidth(context, s.line_width.into());
                CGContextSetLineCap(
                    context,
                    match s.line_cap {
                        LineCap::Butt => 0,
                        LineCap::Round => 1,
                        LineCap::Square => 2,
                    },
                );
                CGContextSetLineJoin(
                    context,
                    match s.line_join {
                        LineJoin::Miter => 0,
                        LineJoin::Round => 1,
                        LineJoin::Bevel => 2,
                    },
                );
                CGContextSetMiterLimit(context, s.miter_limit.into());
                let dash: Vec<f64> = s.line_dash.iter().copied().map(f64::from).collect();
                CGContextSetLineDash(
                    context,
                    s.line_dash_offset.into(),
                    dash.as_ptr(),
                    dash.len(),
                );
                &s.stroke
            }
        };
        match style {
            CanvasStyle::Color(color) => {
                set_color(context, *color, matches!(mode, PaintMode::Stroke));
                match mode {
                    PaintMode::Fill(FillRule::NonZero) => CGContextFillPath(context),
                    PaintMode::Fill(FillRule::EvenOdd) => CGContextEOFillPath(context),
                    PaintMode::Stroke => CGContextStrokePath(context),
                }
            }
            CanvasStyle::Gradient(gradient) => {
                match mode {
                    PaintMode::Fill(rule) => clip_path(context, rule),
                    PaintMode::Stroke => {
                        CGContextReplacePathWithStrokedPath(context);
                        CGContextClip(context);
                    }
                }
                draw_gradient(context, resources, &gradient.0);
            }
        }
        CGContextRestoreGState(context);
    }
}

unsafe fn clip_path(context: CGContextRef, rule: FillRule) {
    unsafe {
        match rule {
            FillRule::NonZero => CGContextClip(context),
            FillRule::EvenOdd => CGContextEOClip(context),
        }
    }
}

// The most periods drawn of a reflected or repeated gradient.
const MAX_SPREAD_PERIODS: f64 = 4096.0;

/// Fill the clip with a gradient. Core Graphics only pads gradients, so a reflected or
/// repeated one is drawn as one padded gradient over the periods the clip covers.
unsafe fn draw_gradient(context: CGContextRef, resources: &Resources, gradient: &Arc<Gradient>) {
    if gradient.stops.is_empty() {
        return;
    }
    let point = |(x, y): (f32, f32)| CGPoint {
        x: x.into(),
        y: y.into(),
    };
    let pad = gradient.spread == GradientSpread::Pad;
    unsafe {
        CGContextConcatCTM(context, affine(gradient.transform));
        let clip = CGContextGetClipBoundingBox(context);
        // An entirely clipped path has a null bounding box with an infinite origin.
        // It has no paintable pixels, and cannot be projected into gradient periods.
        if clip.size.width <= 0.0
            || clip.size.height <= 0.0
            || !clip.origin.x.is_finite()
            || !clip.origin.y.is_finite()
            || !clip.size.width.is_finite()
            || !clip.size.height.is_finite()
        {
            return;
        }
        let corners = [
            (clip.origin.x, clip.origin.y),
            (clip.origin.x + clip.size.width, clip.origin.y),
            (clip.origin.x, clip.origin.y + clip.size.height),
            (
                clip.origin.x + clip.size.width,
                clip.origin.y + clip.size.height,
            ),
        ];
        let extend = K_CG_GRADIENT_DRAWS_BEFORE_START | K_CG_GRADIENT_DRAWS_AFTER_END;
        match gradient.shape {
            GradientShape::Linear { start, end } => {
                let (start, end) = (point(start), point(end));
                let (dx, dy) = (end.x - start.x, end.y - start.y);
                let length_squared = dx * dx + dy * dy;
                if pad || length_squared == 0.0 {
                    let native = resources.gradient(gradient, [0, 0]);
                    CGContextDrawLinearGradient(context, native.0, start, end, extend);
                    return;
                }
                let project =
                    |(x, y): (f64, f64)| ((x - start.x) * dx + (y - start.y) * dy) / length_squared;
                let first = corners
                    .into_iter()
                    .map(project)
                    .fold(f64::INFINITY, f64::min);
                let last = corners
                    .into_iter()
                    .map(project)
                    .fold(f64::NEG_INFINITY, f64::max);
                let first = first.floor().max(-MAX_SPREAD_PERIODS / 2.0);
                let last = last.ceil().min(first + MAX_SPREAD_PERIODS);
                let native = resources.gradient(gradient, [first as i32, last as i32]);
                let along = |t: f64| CGPoint {
                    x: start.x + dx * t,
                    y: start.y + dy * t,
                };
                CGContextDrawLinearGradient(context, native.0, along(first), along(last), extend);
            }
            GradientShape::Radial {
                focal,
                center,
                radius,
            } => {
                let (focal, center, radius) = (point(focal), point(center), f64::from(radius));
                if pad || radius == 0.0 {
                    let native = resources.gradient(gradient, [0, 0]);
                    CGContextDrawRadialGradient(
                        context, native.0, focal, 0.0, center, radius, extend,
                    );
                    return;
                }
                let farthest = corners
                    .into_iter()
                    .map(|(x, y)| (x - center.x).hypot(y - center.y))
                    .fold(0.0, f64::max);
                let periods = (farthest / radius).ceil().clamp(1.0, MAX_SPREAD_PERIODS);
                let native = resources.gradient(gradient, [0, periods as i32]);
                CGContextDrawRadialGradient(
                    context,
                    native.0,
                    center,
                    0.0,
                    center,
                    radius * periods,
                    extend,
                );
            }
        }
    }
}

/// An owned Core Graphics gradient.
struct NativeGradient(*mut c_void);

impl NativeGradient {
    fn new(stops: &[(f32, Color)]) -> Self {
        // Core Graphics needs two stops, a single one is a uniform color.
        let stops = if stops.len() == 1 {
            vec![stops[0], (1.0, stops[0].1)]
        } else {
            stops.to_vec()
        };
        let components: Vec<f64> = stops
            .iter()
            .flat_map(|(_, color)| {
                let (r, g, b, a) = color.components(1.0);
                [r, g, b, a]
            })
            .collect();
        let locations: Vec<f64> = stops.iter().map(|(offset, _)| f64::from(*offset)).collect();
        unsafe {
            let space = srgb_color_space();
            let gradient = CGGradientCreateWithColorComponents(
                space,
                components.as_ptr(),
                locations.as_ptr(),
                locations.len(),
            );
            assert!(!gradient.is_null(), "Could not create gradient");
            Self(gradient)
        }
    }
}

impl Drop for NativeGradient {
    fn drop(&mut self) {
        unsafe { CGGradientRelease(self.0) };
    }
}

const fn blend_mode(operation: CompositeOperation) -> i32 {
    use CompositeOperation::*;
    match operation {
        SourceOver => 0,
        Multiply => 1,
        Screen => 2,
        Overlay => 3,
        Darken => 4,
        Lighten => 5,
        ColorDodge => 6,
        ColorBurn => 7,
        SoftLight => 8,
        HardLight => 9,
        Difference => 10,
        Exclusion => 11,
        Hue => 12,
        Saturation => 13,
        Color => 14,
        Luminosity => 15,
        Copy => 17,
        SourceIn => 18,
        SourceOut => 19,
        SourceAtop => 20,
        DestinationOver => 21,
        DestinationIn => 22,
        DestinationOut => 23,
        DestinationAtop => 24,
        Xor => 25,
        Lighter => 27,
    }
}

fn invertible(t: CGAffineTransform) -> bool {
    let det = t.a * t.d - t.b * t.c;
    det != 0.0 && det.is_finite() && [t.tx, t.ty].iter().all(|v| v.is_finite())
}

const fn affine([a, b, c, d, tx, ty]: [f32; 6]) -> CGAffineTransform {
    CGAffineTransform {
        a: a as f64,
        b: b as f64,
        c: c as f64,
        d: d as f64,
        tx: tx as f64,
        ty: ty as f64,
    }
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> CGRect {
    CGRect {
        origin: CGPoint {
            x: x.into(),
            y: y.into(),
        },
        size: CGSize {
            width: w.into(),
            height: h.into(),
        },
    }
}

fn text_line(text: &str, style: &TextStyle) -> CFObject {
    let string = CFObject::string(&single_line(text));
    unsafe {
        let attributed = CFObject::from_create(CFAttributedStringCreate(
            null(),
            string.as_ptr(),
            style.attributes.as_ptr(),
        ))
        .expect("Could not create attributed string");
        CFObject::from_create(CTLineCreateWithAttributedString(attributed.as_ptr()))
            .expect("Could not create text line")
    }
}

fn line_width(line: &CFObject) -> f32 {
    unsafe { CTLineGetTypographicBounds(line.as_ptr(), null_mut(), null_mut(), null_mut()) as f32 }
}

fn create_text_style(s: &CanvasState) -> TextStyle {
    let size = f64::from(s.font_size);
    // Generic families map to fonts on every Apple platform, sans-serif is the system font.
    let family = match s.font_family.as_ref() {
        "serif" => "Times New Roman",
        "monospace" => "Menlo",
        family => family,
    };
    let mut font = named_font(family, size).unwrap_or_else(|| unsafe {
        CFObject::from_create(CTFontCreateUIFontForLanguage(
            K_CT_FONT_UI_FONT_SYSTEM,
            size,
            null(),
        ))
        .expect("Could not create system font")
    });
    if s.font_weight == FontWeight::Bold {
        // Fonts without a bold face keep their regular face.
        if let Some(bold) = unsafe {
            CFObject::from_create(CTFontCreateCopyWithSymbolicTraits(
                font.as_ptr(),
                0.0,
                null(),
                K_CT_FONT_BOLD_TRAIT,
                K_CT_FONT_BOLD_TRAIT,
            ))
        } {
            font = bold;
        }
    }
    unsafe {
        let keys = [
            kCTFontAttributeName,
            kCTForegroundColorFromContextAttributeName,
        ];
        let values = [font.as_ptr(), kCFBooleanTrue];
        let attributes = CFObject::from_create(CFDictionaryCreate(
            null(),
            keys.as_ptr(),
            values.as_ptr(),
            keys.len() as isize,
            &raw const kCFTypeDictionaryKeyCallBacks,
            &raw const kCFTypeDictionaryValueCallBacks,
        ))
        .expect("Could not create text attributes");
        TextStyle {
            attributes,
            ascent: CTFontGetAscent(font.as_ptr()) as f32,
            descent: CTFontGetDescent(font.as_ptr()) as f32,
        }
    }
}

/// Core Text substitutes a fallback font for unknown names, so only accept a font that
/// matches the requested family, full or PostScript name.
fn named_font(family: &str, size: f64) -> Option<CFObject> {
    let name = CFObject::string(family);
    unsafe {
        let font = CFObject::from_create(CTFontCreateWithName(name.as_ptr(), size, null()))?;
        [
            CTFontCopyFamilyName,
            CTFontCopyFullName,
            CTFontCopyPostScriptName,
        ]
        .into_iter()
        .filter_map(|copy| CFObject::from_create(copy(font.as_ptr())))
        .any(|candidate| {
            CFStringCompare(
                name.as_ptr(),
                candidate.as_ptr(),
                K_CF_COMPARE_CASE_INSENSITIVE,
            ) == 0
        })
        .then_some(font)
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bold_text_preserves_the_requested_family() {
        let state = CanvasState {
            font_family: Arc::from("Menlo"),
            font_size: 18.0,
            ..CanvasState::default()
        };
        let normal = create_text_style(&state);
        let bold = create_text_style(&CanvasState {
            font_weight: FontWeight::Bold,
            ..state
        });
        unsafe {
            let normal_font =
                CFDictionaryGetValue(normal.attributes.as_ptr(), kCTFontAttributeName);
            let bold_font = CFDictionaryGetValue(bold.attributes.as_ptr(), kCTFontAttributeName);
            let normal_family =
                CFObject::from_create(CTFontCopyFamilyName(normal_font)).expect("family name");
            let bold_family =
                CFObject::from_create(CTFontCopyFamilyName(bold_font)).expect("family name");
            assert_eq!(
                CFStringCompare(normal_family.as_ptr(), bold_family.as_ptr(), 0),
                0
            );
            assert_ne!(CTFontGetSymbolicTraits(bold_font) & K_CT_FONT_BOLD_TRAIT, 0);
            assert_eq!(CTFontGetSize(bold_font), 18.0);
        }
    }

    #[test]
    fn unknown_font_family_falls_back_to_the_system_font() {
        assert!(named_font("sans-serif", 12.0).is_none());
        assert!(named_font("menlo", 12.0).is_some());
    }

    #[test]
    fn text_is_drawn_upright_above_the_alphabetic_baseline() {
        let mut canvas = crate::OffscreenCanvas::new(40, 40);
        canvas.draw(|context| {
            context.set_font("Menlo", 20.0);
            context.set_fill_style(Color::rgb(255, 0, 0));
            context.fill_text("T", 10.0, 30.0);
        });
        let pixels = canvas.pixels();
        let ink = |y: usize| (0..40).any(|x| pixels[(y * 40 + x) * 4 + 3] > 128);
        // The crossbar of an upright T sits near the top of its glyph box.
        let top = (0..40).find(|&y| ink(y)).expect("text was drawn");
        assert!((10..20).contains(&top));
        assert!(!(31..40).any(ink));
        let red = (0..40 * 40)
            .find(|i| pixels[i * 4 + 3] == 255)
            .expect("opaque ink");
        assert_eq!(&pixels[red * 4..][..4], [255, 0, 0, 255]);
    }

    #[test]
    fn cached_fill_preserves_restored_colors_and_alpha() {
        let mut pixels = [0_u8; 16];
        unsafe {
            let space = srgb_color_space();
            // Big-endian RGBA, premultiplied alpha in the last component.
            let context = CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                4,
                1,
                8,
                16,
                space,
                (4 << 12) | 1,
            );
            assert!(!context.is_null());
            {
                let mut canvas = PlatformCanvasContext::new(context, Resources::default());
                let mut state = CanvasState {
                    fill: Color::rgb(255, 0, 0).into(),
                    ..CanvasState::default()
                };
                canvas.fill_rect(&state, 0.0, 0.0, 1.0, 1.0);
                canvas.save();
                state.fill = Color::rgb(0, 0, 255).into();
                canvas.fill_rect(&state, 1.0, 0.0, 1.0, 1.0);
                canvas.restore();
                canvas.fill_rect(&state, 2.0, 0.0, 1.0, 1.0);
                state.alpha = 0.5;
                canvas.fill_rect(&state, 3.0, 0.0, 1.0, 1.0);
            }
            CGContextRelease(context);
        }
        assert_eq!(
            &pixels[..12],
            &[255, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 255]
        );
        assert_eq!(&pixels[12..], &[0, 0, 128, 128]);
    }

    #[test]
    fn rect_painting_preserves_the_current_path() {
        let mut canvas = crate::OffscreenCanvas::new(80, 80);
        canvas.draw(|context| {
            context.begin_path();
            context.rect(10.0, 10.0, 20.0, 20.0);
            context.fill_rect(60.0, 60.0, 10.0, 10.0);
            context.stroke_rect(40.0, 40.0, 10.0, 10.0);
            context.clear_rect(0.0, 0.0, 5.0, 5.0);
            context.fill();
        });
        let pixels = canvas.pixels();
        let pixel = |x: usize, y: usize| &pixels[(y * 80 + x) * 4..][..4];
        assert_eq!(pixel(20, 20), [0, 0, 0, 255]);
        assert_eq!(pixel(65, 65), [0, 0, 0, 255]);
        assert_eq!(pixel(35, 35), [0, 0, 0, 0]);
    }

    #[test]
    fn set_transform_is_relative_to_the_initial_transform() {
        let mut canvas = crate::OffscreenCanvas::new(4, 2);
        canvas.draw(|context| {
            context.translate(3.0, 0.0);
            context.scale(2.0, 2.0);
            context.set_transform(1.0, 0.0, 0.0, 1.0, 1.0, 1.0);
            context.fill_rect(0.0, 0.0, 1.0, 1.0);
            context.reset_transform();
            context.fill_rect(0.0, 0.0, 1.0, 1.0);
        });
        let alpha: Vec<u8> = canvas.pixels().iter().skip(3).step_by(4).copied().collect();
        assert_eq!(alpha, [255, 0, 0, 0, 0, 255, 0, 0]);
    }
}
