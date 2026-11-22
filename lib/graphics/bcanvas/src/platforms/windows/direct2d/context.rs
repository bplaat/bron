/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::ffi::c_void;
use std::ops::Deref;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::headers::*;
use crate::cache::{GradientKey, LruCache};
use crate::path::{Command, Path, Point, Transform};
use crate::platforms::windows::com::ComPtr;
use crate::platforms::windows::headers::{w, wide};
use crate::platforms::windows::image::PlatformImage;
use crate::{
    CanvasState, CanvasStyle, Color, CompositeOperation, FillRule, FontWeight, Gradient,
    GradientShape, GradientSpread, LineCap, LineJoin, TextExtents,
};

const TEXT_FORMATS: usize = 16;
const TEXT_LAYOUTS: usize = 128;
const GRADIENTS: usize = 32;
const PATHS: usize = 64;
// The layout box of text, wide enough to never wrap.
const LAYOUT_WIDTH: f32 = 100_000.0;
const LAYOUT_HEIGHT: f32 = 10_000.0;

type TextFormatKey = (Arc<str>, u32, FontWeight);
type TextLayoutKey = (Box<str>, TextFormatKey);

// A laid out line of text with its extents and line height.
#[derive(Clone)]
struct TextLayout {
    layout: ComPtr<IDWriteTextLayout>,
    extents: TextExtents,
    height: f32,
}

/// Device-independent objects shared by every frame of one canvas.
pub(super) struct Resources {
    pub(super) factory: ComPtr<ID2D1Factory>,
    write: ComPtr<IDWriteFactory>,
    // Indexed by cap * 3 + join, in the order of stroke_style.
    strokes: [ComPtr<ID2D1StrokeStyle>; 9],
    formats: RefCell<Vec<(TextFormatKey, ComPtr<IDWriteTextFormat>)>>,
    // Least recently used first, so text drawn every frame is laid out once.
    layouts: RefCell<Vec<(TextLayoutKey, TextLayout)>>,
    text: RefCell<Vec<u16>>,
    gradient_stops: RefCell<LruCache<(GradientKey, u64), ComPtr<ID2D1GradientStopCollection>>>,
    // The user-space geometries of reused paths with their fill rule.
    geometries: RefCell<LruCache<(u64, FillRule), ComPtr<ID2D1PathGeometry>>>,
    // Whether DirectWrite draws color fonts like emoji, since Windows 8.1.
    color_fonts: bool,
}

impl Resources {
    pub(super) fn new() -> Rc<Self> {
        const CAPS: [u32; 3] = [
            D2D1_CAP_STYLE_FLAT,
            D2D1_CAP_STYLE_SQUARE,
            D2D1_CAP_STYLE_ROUND,
        ];
        const JOINS: [u32; 3] = [
            D2D1_LINE_JOIN_MITER_OR_BEVEL,
            D2D1_LINE_JOIN_BEVEL,
            D2D1_LINE_JOIN_ROUND,
        ];
        let factory = unsafe { ID2D1Factory::new() }.expect("Direct2D factory");
        let strokes = std::array::from_fn(|index| {
            let cap = CAPS[index / 3];
            unsafe {
                factory.CreateStrokeStyle(
                    &D2D1_STROKE_STYLE_PROPERTIES {
                        startCap: cap,
                        endCap: cap,
                        dashCap: cap,
                        lineJoin: JOINS[index % 3],
                        miterLimit: 10.0,
                        ..Default::default()
                    },
                    null(),
                    0,
                )
            }
            .expect("Direct2D stroke style")
        });
        let write = unsafe { IDWriteFactory::new() }.expect("DirectWrite factory");
        let color_fonts = write.supports(&IID_IDWRITE_FACTORY2);
        Rc::new(Self {
            factory,
            write,
            strokes,
            formats: RefCell::new(Vec::new()),
            layouts: RefCell::new(Vec::new()),
            text: RefCell::new(Vec::new()),
            gradient_stops: RefCell::new(LruCache::new(GRADIENTS)),
            geometries: RefCell::new(LruCache::new(PATHS)),
            color_fonts,
        })
    }
}

/// A render target with the one brush that every draw recolors, and a unique id that images
/// key their bitmaps of its resource domain on.
pub(super) struct DrawTarget<T> {
    pub(super) native: ComPtr<T>,
    brush: ComPtr<ID2D1SolidColorBrush>,
    id: u64,
}

impl<T: Deref<Target = ID2D1RenderTarget>> DrawTarget<T> {
    pub(super) fn new(native: ComPtr<T>) -> Result<Self, i32> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let brush = unsafe { native.CreateSolidColorBrush(&D2D1_COLOR_F::default(), null()) }?;
        Ok(Self {
            native,
            brush,
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        })
    }
}

impl<T> Clone for DrawTarget<T> {
    fn clone(&self) -> Self {
        Self {
            native: self.native.clone(),
            brush: self.brush.clone(),
            id: self.id,
        }
    }
}

enum Clip {
    Rect(D2D_RECT_F),
    Layer,
    // A canvas layer, which restores the transform of its start when it ends. Its opacity
    // brush stays alive while the layer is pushed.
    Group {
        transform: Transform,
        _mask: Option<ComPtr<ID2D1Brush>>,
    },
}

// A bitmap that drawing goes into before it becomes a mask or is composited.
struct Offscreen {
    outer: ComPtr<ID2D1RenderTarget>,
    outer_context: ComPtr<ID2D1DeviceContext>,
    outer_brush: ComPtr<ID2D1SolidColorBrush>,
    outer_clips: Vec<Clip>,
    native: ComPtr<ID2D1BitmapRenderTarget>,
    alpha: f32,
    transform: Transform,
    operation: CompositeOperation,
}

// An open canvas layer: a pushed Direct2D layer, which can be inside a composited offscreen,
// or an offscreen alone.
#[derive(Clone, Copy)]
enum Layer {
    Group { composited: bool },
    Composited,
}

pub(crate) struct Context {
    target: ComPtr<ID2D1RenderTarget>,
    // The Direct2D 1.1 interface of the target, which the backend requires.
    context: ComPtr<ID2D1DeviceContext>,
    // The id of the draw target, whose compatible offscreen targets share its resources.
    target_id: u64,
    brush: ComPtr<ID2D1SolidColorBrush>,
    resources: Rc<Resources>,
    transform: Transform,
    stack: Vec<(Transform, usize)>,
    clips: Vec<Clip>,
    offscreens: Vec<Offscreen>,
    layers: Vec<Layer>,
}

impl Context {
    pub(super) fn new<T: Deref<Target = ID2D1RenderTarget>>(
        target: &DrawTarget<T>,
        resources: Rc<Resources>,
    ) -> Self {
        let native = target.native.upcast();
        unsafe { native.SetTransform(&D2D_MATRIX_3X2_F::identity()) };
        Self {
            context: device_context(&native),
            target: native,
            target_id: target.id,
            brush: target.brush.clone(),
            resources,
            transform: Transform::IDENTITY,
            stack: Vec::new(),
            clips: Vec::new(),
            offscreens: Vec::new(),
            layers: Vec::new(),
        }
    }

    pub(crate) fn save(&mut self) {
        self.stack.push((self.transform, self.clips.len()));
    }

    pub(crate) fn restore(&mut self) {
        if let Some((transform, clips)) = self.stack.pop() {
            self.pop_clips(clips);
            self.transform = transform;
            self.apply_transform();
        }
    }

    fn pop_clips(&mut self, depth: usize) {
        while self.clips.len() > depth {
            self.pop_clip();
        }
    }

    fn pop_clip(&mut self) -> Option<Clip> {
        let clip = self.clips.pop()?;
        if matches!(clip, Clip::Rect(_)) {
            unsafe { self.target.PopAxisAlignedClip() };
        } else {
            unsafe { self.target.PopLayer() };
        }
        Some(clip)
    }

    pub(crate) fn begin_layer(&mut self, s: &CanvasState) {
        self.begin_offscreen(s.alpha, s.composite);
        self.layers.push(Layer::Composited);
    }

    // Pushes a Direct2D layer, which composites with source over.
    fn push_group(&mut self, alpha: f32, mask: Option<ComPtr<ID2D1Brush>>, transform: Transform) {
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: rect(-1.0e10, -1.0e10, 2.0e10, 2.0e10),
            maskTransform: D2D_MATRIX_3X2_F::identity(),
            opacity: alpha,
            opacityBrush: mask.as_ref().map_or(null_mut(), ComPtr::as_ptr),
            ..Default::default()
        };
        unsafe {
            // The opacity brush maps the mask bitmap from the device origin.
            self.target.SetTransform(&D2D_MATRIX_3X2_F::identity());
            self.target.PushLayer(&params, null_mut());
        }
        self.clips.push(Clip::Group {
            transform,
            _mask: mask,
        });
        self.transform = transform;
        self.apply_transform();
    }

    pub(crate) fn begin_mask_layer(&mut self, s: &CanvasState) {
        // Other operations composite the masked layer as a whole.
        let composited = s.composite != CompositeOperation::SourceOver;
        let alpha = if composited {
            self.begin_offscreen(s.alpha, s.composite);
            1.0
        } else {
            s.alpha
        };
        self.begin_offscreen(alpha, CompositeOperation::SourceOver);
        self.layers.push(Layer::Group { composited });
    }

    pub(crate) fn end_mask(&mut self) {
        let (offscreen, bitmap) = self.finish_offscreen();
        let brush = bitmap
            .and_then(|bitmap| unsafe { self.target.CreateBitmapBrush(bitmap.as_ptr()) }.ok());
        self.push_group(offscreen.alpha, brush, offscreen.transform);
    }

    pub(crate) fn end_layer(&mut self) {
        let Some(layer) = self.layers.pop() else {
            return;
        };
        if let Layer::Group { composited } = layer {
            while let Some(clip) = self.pop_clip() {
                if let Clip::Group { transform, .. } = clip {
                    self.transform = transform;
                    self.apply_transform();
                    break;
                }
            }
            if !composited {
                return;
            }
        }
        self.end_composited();
    }

    // Redirects drawing into a bitmap like the target, which starts without clips.
    fn begin_offscreen(&mut self, alpha: f32, operation: CompositeOperation) {
        let native = unsafe { self.target.CreateCompatibleRenderTarget() }
            .expect("Direct2D offscreen target");
        let brush = unsafe { native.CreateSolidColorBrush(&D2D1_COLOR_F::default(), null()) }
            .expect("Direct2D offscreen brush");
        unsafe {
            native.BeginDraw();
            native.Clear(&D2D1_COLOR_F::default());
        }
        let outer = std::mem::replace(&mut self.target, native.upcast());
        let outer_context = std::mem::replace(&mut self.context, device_context(&self.target));
        let outer_brush = std::mem::replace(&mut self.brush, brush);
        let outer_clips = std::mem::take(&mut self.clips);
        self.offscreens.push(Offscreen {
            outer,
            outer_context,
            outer_brush,
            outer_clips,
            native,
            alpha,
            transform: self.transform,
            operation,
        });
        self.apply_transform();
    }

    // Returns drawing to the target of before the offscreen, with the offscreen bitmap.
    fn finish_offscreen(&mut self) -> (Offscreen, Option<ComPtr<ID2D1Bitmap>>) {
        self.pop_clips(0);
        let offscreen = self.offscreens.pop().expect("open offscreen");
        let bitmap = unsafe {
            let _ = offscreen.native.EndDraw(null_mut(), null_mut());
            offscreen.native.GetBitmap()
        };
        self.target = offscreen.outer.clone();
        self.context = offscreen.outer_context.clone();
        self.brush = offscreen.outer_brush.clone();
        let mut offscreen = offscreen;
        self.clips = std::mem::take(&mut offscreen.outer_clips);
        (offscreen, bitmap.ok())
    }

    fn end_composited(&mut self) {
        let (offscreen, bitmap) = self.finish_offscreen();
        if let Some(bitmap) = bitmap {
            self.composite(&bitmap, offscreen.alpha, offscreen.operation);
        }
        self.transform = offscreen.transform;
        self.apply_transform();
    }

    // Composites a bitmap of the whole target onto it, through Direct2D 1.1 effects.
    fn composite(&self, bitmap: &ComPtr<ID2D1Bitmap>, alpha: f32, operation: CompositeOperation) {
        unsafe { self.target.SetTransform(&D2D_MATRIX_3X2_F::identity()) };
        let context = &self.context;
        let mut image: *mut c_void = bitmap.as_ptr().cast();
        let opacity = (alpha < 1.0)
            .then(|| unsafe { context.CreateEffect(&CLSID_D2D1_OPACITY) }.ok())
            .flatten()
            .map(|effect| unsafe {
                effect.SetInput(0, image);
                let _ = effect.SetValue(0, alpha);
                effect.GetOutput()
            });
        if let Some(output) = &opacity {
            image = output.as_ptr().cast();
        }
        match (composite_mode(operation), blend_mode(operation)) {
            (Some(mode), _) => unsafe { context.DrawImage(image, mode) },
            (None, Some(mode)) => {
                // The blend effect mixes the image with a snapshot of the target. Taking it
                // restores the user transform, while the image covers the whole target.
                let snapshot = unsafe { self.snapshot() };
                unsafe { self.target.SetTransform(&D2D_MATRIX_3X2_F::identity()) };
                let blended = snapshot.and_then(|snapshot| unsafe {
                    let effect = context.CreateEffect(&CLSID_D2D1_BLEND).ok()?;
                    effect.SetInput(0, snapshot.as_ptr().cast());
                    effect.SetInput(1, image);
                    effect.SetValue(0, mode).ok()?;
                    Some(effect.GetOutput())
                });
                match blended {
                    Some(output) => unsafe {
                        context.DrawImage(output.as_ptr().cast(), D2D1_COMPOSITE_MODE_SOURCE_COPY)
                    },
                    None => unsafe { context.DrawImage(image, 0) },
                }
            }
            (None, None) => unreachable!("every operation is a composite or blend mode"),
        }
        self.apply_transform();
    }

    // A copy of the target, which the blend effect reads as its destination.
    unsafe fn snapshot(&self) -> Option<ComPtr<ID2D1Bitmap>> {
        unsafe {
            let (dpi_x, dpi_y) = self.target.GetDpi();
            let bitmap = self
                .target
                .CreateBitmap(
                    self.target.GetPixelSize(),
                    null(),
                    0,
                    &D2D1_BITMAP_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: dpi_x,
                        dpiY: dpi_y,
                    },
                )
                .ok()?;
            // CopyFromRenderTarget rejects active clips. Rectangular clips do not
            // buffer drawing, so suspend them while copying the backdrop.
            let rectangles = self
                .clips
                .iter()
                .rev()
                .take_while(|clip| matches!(clip, Clip::Rect(_)))
                .count();
            for _ in 0..rectangles {
                self.target.PopAxisAlignedClip();
            }
            let result = bitmap.CopyFromRenderTarget(self.target.as_ptr());
            self.target.SetTransform(&D2D_MATRIX_3X2_F::identity());
            for clip in &self.clips[self.clips.len() - rectangles..] {
                if let Clip::Rect(rect) = clip {
                    self.target
                        .PushAxisAlignedClip(rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                }
            }
            self.apply_transform();
            result.ok()?;
            Some(bitmap)
        }
    }

    // Draws with the composite operation. Source over, copy and lighter are primitive blends,
    // other operations composite a layer of the drawing limited to the device-space shape of
    // `bounds`, so they only change the drawn area like on the other backends.
    fn draw_with(
        &mut self,
        state: &CanvasState,
        bounds: impl FnOnce(&Self) -> Option<ComPtr<ID2D1PathGeometry>>,
        draw: impl FnOnce(&Self, &CanvasState),
    ) {
        let blend = match state.composite {
            CompositeOperation::SourceOver => Some(D2D1_PRIMITIVE_BLEND_SOURCE_OVER),
            CompositeOperation::Copy => Some(D2D1_PRIMITIVE_BLEND_COPY),
            CompositeOperation::Lighter => Some(D2D1_PRIMITIVE_BLEND_ADD),
            _ => None,
        };
        if let Some(blend) = blend {
            let blended = blend != D2D1_PRIMITIVE_BLEND_SOURCE_OVER;
            if blended {
                unsafe { self.context.SetPrimitiveBlend(blend) };
            }
            draw(self, state);
            if blended {
                unsafe {
                    self.context
                        .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER)
                };
            }
            return;
        }
        let Some(bounds) = bounds(self) else {
            return;
        };
        // Blend modes preserve the destination outside the source. Porter-Duff
        // operations instead need a complete result copied through the draw's bounds.
        let bounded = blend_mode(state.composite).is_none();
        if bounded {
            let Some(backdrop) = (unsafe { self.snapshot() }) else {
                return;
            };
            self.begin_offscreen(1.0, CompositeOperation::SourceOver);
            unsafe {
                self.target.SetTransform(&D2D_MATRIX_3X2_F::identity());
                self.target.DrawBitmap(
                    backdrop.as_ptr(),
                    null(),
                    1.0,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                );
            }
            self.apply_transform();
        }
        self.begin_offscreen(1.0, state.composite);
        draw(
            self,
            &CanvasState {
                composite: CompositeOperation::SourceOver,
                ..state.clone()
            },
        );
        self.end_composited();
        if bounded {
            let (offscreen, bitmap) = self.finish_offscreen();
            let bitmap = bitmap.expect("Direct2D composite bitmap");
            let brush = unsafe { self.target.CreateBitmapBrush(bitmap.as_ptr()) }
                .expect("Direct2D composite brush");
            unsafe {
                self.target.SetTransform(&D2D_MATRIX_3X2_F::identity());
                self.context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
                self.target
                    .FillGeometry(bounds.as_ptr(), brush.as_ptr(), null_mut());
                self.context
                    .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            }
            self.transform = offscreen.transform;
            self.apply_transform();
        }
    }

    // The device-space outline of a stroke of a user-space geometry.
    fn widened(
        &self,
        geometry: &ComPtr<ID2D1PathGeometry>,
        state: &CanvasState,
    ) -> Option<ComPtr<ID2D1PathGeometry>> {
        let outline = unsafe { self.resources.factory.CreatePathGeometry() }.ok()?;
        let sink = unsafe { outline.Open() }.ok()?;
        unsafe {
            geometry
                .Widen(
                    state.line_width,
                    self.stroke_style(state).as_ptr(),
                    &matrix(self.transform),
                    sink.as_ptr(),
                )
                .ok()?;
            sink.Close().ok()?;
        }
        Some(outline)
    }

    // The device-space geometry of a user-space rectangle.
    fn rect_geometry(&self, x: f32, y: f32, w: f32, h: f32) -> ComPtr<ID2D1PathGeometry> {
        let mut path = Path::default();
        path.rect(x, y, w, h);
        self.path_geometry(&path, self.transform, FillRule::NonZero)
    }

    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let rect = rect(x, y, w, h);
        // An axis-aligned clip is the transformed bounding box, so rotated or skewed
        // rects instead copy a transparent fill through the Direct2D 1.1 interface.
        let [a, b, c, d, _, _] = self.transform.0;
        if !((b == 0.0 && c == 0.0) || (a == 0.0 && d == 0.0)) {
            let brush = self.color_brush(Color::rgba(0, 0, 0, 0), 1.0);
            unsafe {
                self.context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
                self.target.FillRectangle(&rect, brush.as_ptr());
                self.context
                    .SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
            }
            return;
        }
        unsafe {
            self.target
                .PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_ALIASED);
            self.target.Clear(&D2D1_COLOR_F::default());
            self.target.PopAxisAlignedClip();
        }
    }

    pub(crate) fn fill_rect(&mut self, state: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        self.draw_with(
            state,
            |this| Some(this.rect_geometry(x, y, w, h)),
            |this, state| {
                let brush = this.brush(&state.fill, state.alpha, Transform::IDENTITY);
                unsafe { this.target.FillRectangle(&rect(x, y, w, h), brush.as_ptr()) };
            },
        );
    }

    pub(crate) fn stroke_rect(&mut self, state: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        self.draw_with(
            state,
            |this| {
                let mut path = Path::default();
                path.rect(x, y, w, h);
                let geometry = this.path_geometry(&path, Transform::IDENTITY, FillRule::NonZero);
                this.widened(&geometry, state)
            },
            |this, state| {
                let brush = this.brush(&state.stroke, state.alpha, Transform::IDENTITY);
                let style = this.stroke_style(state);
                unsafe {
                    this.target.DrawRectangle(
                        &rect(x, y, w, h),
                        brush.as_ptr(),
                        state.line_width,
                        style.as_ptr(),
                    )
                };
            },
        );
    }

    // The user-space geometry of a path, cached for a reused path.
    fn user_geometry(&self, path: &Path, rule: FillRule) -> ComPtr<ID2D1PathGeometry> {
        let build = || self.path_geometry(path, Transform::IDENTITY, rule);
        if path.id == 0 {
            return build();
        }
        self.resources
            .geometries
            .borrow_mut()
            .get_or_create((path.id, rule), || Some(build()))
            .cloned()
            .expect("created geometry")
    }

    fn path_geometry(
        &self,
        path: &Path,
        transform: Transform,
        rule: FillRule,
    ) -> ComPtr<ID2D1PathGeometry> {
        let geometry =
            unsafe { self.resources.factory.CreatePathGeometry() }.expect("Direct2D path");
        let sink = unsafe { geometry.Open() }.expect("Direct2D geometry sink");
        // Every subpath of a path starts with a move.
        let mut open = false;
        unsafe {
            sink.SetFillMode(match rule {
                FillRule::NonZero => D2D1_FILL_MODE_WINDING,
                FillRule::EvenOdd => D2D1_FILL_MODE_ALTERNATE,
            });
            for command in &path.commands {
                match *command {
                    Command::Move(point) => {
                        if open {
                            sink.EndFigure(D2D1_FIGURE_END_OPEN);
                        }
                        sink.BeginFigure(vector(transform.point(point)), D2D1_FIGURE_BEGIN_FILLED);
                        open = true;
                    }
                    Command::Line(point) => sink.AddLine(vector(transform.point(point))),
                    Command::Cubic(a, b, end) => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                        point1: vector(transform.point(a)),
                        point2: vector(transform.point(b)),
                        point3: vector(transform.point(end)),
                    }),
                    Command::Quadratic(control, end) => {
                        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
                            point1: vector(transform.point(control)),
                            point2: vector(transform.point(end)),
                        })
                    }
                    Command::Close => {
                        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                        open = false;
                    }
                }
            }
            if open {
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
            }
            sink.Close().expect("finish Direct2D geometry");
        }
        geometry
    }

    pub(crate) fn fill(&mut self, state: &CanvasState, path: &Path, rule: FillRule) {
        // A lone ellipse in a solid color fills without building a path, like particles.
        if let Some(([x, y], [rx, ry])) = path.lone_ellipse()
            && state.composite == CompositeOperation::SourceOver
            && let CanvasStyle::Color(color) = &state.fill
        {
            let brush = self.color_brush(*color, state.alpha);
            let ellipse = D2D1_ELLIPSE {
                point: D2D_POINT_2F { x, y },
                radiusX: rx,
                radiusY: ry,
            };
            unsafe { self.target.FillEllipse(&ellipse, brush.as_ptr()) };
            return;
        }
        let geometry = self.user_geometry(path, rule);
        self.draw_with(
            state,
            |this| Some(this.path_geometry(path, this.transform, rule)),
            |this, state| {
                let brush = this.brush(&state.fill, state.alpha, Transform::IDENTITY);
                unsafe {
                    this.target
                        .FillGeometry(geometry.as_ptr(), brush.as_ptr(), null_mut())
                };
            },
        );
    }

    pub(crate) fn stroke(&mut self, state: &CanvasState, path: &Path) {
        // Strokes are drawn in user space, which the transform maps with their width.
        let geometry = self.user_geometry(path, FillRule::NonZero);
        self.draw_with(
            state,
            |this| this.widened(&geometry, state),
            |this, state| {
                let brush = this.brush(&state.stroke, state.alpha, Transform::IDENTITY);
                let style = this.stroke_style(state);
                unsafe {
                    this.target.DrawGeometry(
                        geometry.as_ptr(),
                        brush.as_ptr(),
                        state.line_width,
                        style.as_ptr(),
                    )
                };
            },
        );
    }

    pub(crate) fn draw_image(
        &mut self,
        state: &CanvasState,
        image: &PlatformImage,
        source: [f32; 4],
        [x, y, w, h]: [f32; 4],
    ) {
        let rect = rect(x, y, w, h);
        let [sx, sy, sw, sh] = source;
        let source = self::rect(sx, sy, sw, sh);
        self.draw_with(
            state,
            |this| Some(this.rect_geometry(x, y, w, h)),
            |this, state| {
                let Some(bitmap) = image.direct2d.bitmap(image, &this.target, this.target_id)
                else {
                    return;
                };
                unsafe {
                    this.context.DrawBitmap(
                        bitmap.as_ptr(),
                        &rect,
                        state.alpha,
                        if state.image_smoothing {
                            D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC
                        } else {
                            D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR
                        },
                        &source,
                    );
                }
            },
        );
    }

    pub(crate) fn clip(&mut self, path: &Path, rule: FillRule) {
        unsafe { self.target.SetTransform(&D2D_MATRIX_3X2_F::identity()) };
        if let Some(rect) = device_rect(path, self.transform) {
            unsafe {
                self.target
                    .PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            self.clips.push(Clip::Rect(rect));
        } else {
            let geometry = self.user_geometry(path, rule);
            let params = D2D1_LAYER_PARAMETERS {
                contentBounds: rect(-1.0e10, -1.0e10, 2.0e10, 2.0e10),
                geometricMask: geometry.as_ptr(),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: matrix(self.transform),
                opacity: 1.0,
                ..Default::default()
            };
            unsafe { self.target.PushLayer(&params, null_mut()) };
            self.clips.push(Clip::Layer);
        }
        self.apply_transform();
    }

    fn stroke_style(&self, state: &CanvasState) -> ComPtr<ID2D1StrokeStyle> {
        let cap = match state.line_cap {
            LineCap::Butt => 0,
            LineCap::Square => 1,
            LineCap::Round => 2,
        };
        let join = match state.line_join {
            LineJoin::Miter => 0,
            LineJoin::Bevel => 1,
            LineJoin::Round => 2,
        };
        if state.line_dash.is_empty() && state.miter_limit == 10.0 {
            return self.resources.strokes[cap * 3 + join].clone();
        }
        const CAPS: [u32; 3] = [
            D2D1_CAP_STYLE_FLAT,
            D2D1_CAP_STYLE_SQUARE,
            D2D1_CAP_STYLE_ROUND,
        ];
        const JOINS: [u32; 3] = [
            D2D1_LINE_JOIN_MITER_OR_BEVEL,
            D2D1_LINE_JOIN_BEVEL,
            D2D1_LINE_JOIN_ROUND,
        ];
        // Direct2D dashes are in units of the line width.
        let dashes: Vec<f32> = state
            .line_dash
            .iter()
            .map(|length| length / state.line_width)
            .collect();
        let dashed = !dashes.is_empty();
        unsafe {
            self.resources.factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: CAPS[cap],
                    endCap: CAPS[cap],
                    dashCap: CAPS[cap],
                    lineJoin: JOINS[join],
                    miterLimit: state.miter_limit,
                    dashStyle: if dashed { D2D1_DASH_STYLE_CUSTOM } else { 0 },
                    dashOffset: state.line_dash_offset / state.line_width,
                },
                if dashed { dashes.as_ptr() } else { null() },
                dashes.len() as u32,
            )
        }
        .unwrap_or_else(|_| self.resources.strokes[cap * 3 + join].clone())
    }

    pub(crate) fn translate(&mut self, x: f32, y: f32) {
        self.concat(Transform([1.0, 0.0, 0.0, 1.0, x, y]));
    }
    pub(crate) fn rotate(&mut self, angle: f32) {
        let (s, c) = angle.sin_cos();
        self.concat(Transform([c, s, -s, c, 0.0, 0.0]));
    }
    pub(crate) fn scale(&mut self, x: f32, y: f32) {
        self.concat(Transform([x, 0.0, 0.0, y, 0.0, 0.0]));
    }
    pub(crate) fn transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.concat(Transform([a, b, c, d, e, f]));
    }
    pub(crate) fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.transform = Transform([a, b, c, d, e, f]);
        self.apply_transform();
    }
    pub(crate) fn reset_transform(&mut self) {
        self.transform = Transform::IDENTITY;
        self.apply_transform();
    }
    fn concat(&mut self, transform: Transform) {
        self.transform = self.transform.concat(transform);
        self.apply_transform();
    }
    fn apply_transform(&self) {
        unsafe { self.target.SetTransform(&matrix(self.transform)) };
    }

    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        let Some(TextLayout {
            layout,
            extents,
            height,
        }) = self.prepare_text(s, text)
        else {
            return;
        };
        // The layout box starts at the top of the line.
        let [dx, dy] = extents.origin(s);
        let origin = D2D_POINT_2F {
            x: x + dx,
            y: y + dy - extents.ascent,
        };
        let options = if self.resources.color_fonts {
            D2D1_DRAW_TEXT_OPTIONS_ENABLE_COLOR_FONT
        } else {
            D2D1_DRAW_TEXT_OPTIONS_NONE
        };
        self.draw_with(
            s,
            |this| Some(this.rect_geometry(origin.x, origin.y, extents.width, height)),
            |this, s| {
                let brush = this.brush(&s.fill, s.alpha, Transform::IDENTITY);
                unsafe {
                    this.target
                        .DrawTextLayout(origin, layout.as_ptr(), brush.as_ptr(), options)
                };
            },
        );
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> TextExtents {
        self.prepare_text(s, text)
            .map(|text| text.extents)
            .unwrap_or_default()
    }

    fn prepare_text(&self, s: &CanvasState, text: &str) -> Option<TextLayout> {
        let size = s.font_size.to_bits();
        let mut layouts = self.resources.layouts.borrow_mut();
        if let Some(index) = layouts
            .iter()
            .position(|((cached, (family, key_size, weight)), _)| {
                **cached == *text
                    && *family == s.font_family
                    && *key_size == size
                    && *weight == s.font_weight
            })
        {
            // Move the hit to the back, the most recently used end.
            layouts[index..].rotate_left(1);
            return layouts.last().map(|(_, layout)| layout.clone());
        }
        let format = self.format(s);
        let mut wide = self.resources.text.borrow_mut();
        wide.clear();
        // DirectWrite takes a length-delimited buffer without a terminator.
        // Reuse the allocation; canvas line breaks render as spaces.
        wide.extend(text.encode_utf16().map(|unit| match unit {
            0x09..=0x0d | 0x85 | 0x2028 | 0x2029 => 0x20,
            unit => unit,
        }));
        let layout = unsafe {
            let layout = self
                .resources
                .write
                .CreateTextLayout(
                    wide.as_ptr(),
                    wide.len() as u32,
                    format.as_ptr(),
                    LAYOUT_WIDTH,
                    LAYOUT_HEIGHT,
                )
                .ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            let mut line = DWRITE_LINE_METRICS::default();
            let mut count = 0;
            layout.GetLineMetrics(&mut line, 1, &mut count).ok()?;
            // Overhangs are the distances of the ink outside the layout box.
            let mut overhang = DWRITE_OVERHANG_METRICS::default();
            layout.GetOverhangMetrics(&mut overhang).ok()?;
            let baseline = line.baseline;
            TextLayout {
                layout,
                extents: TextExtents {
                    width: metrics.widthIncludingTrailingWhitespace,
                    ink: [
                        -overhang.left,
                        -overhang.top - baseline,
                        LAYOUT_WIDTH + overhang.right,
                        LAYOUT_HEIGHT + overhang.bottom - baseline,
                    ],
                    ascent: baseline,
                    descent: line.height - baseline,
                },
                height: line.height,
            }
        };
        if layouts.len() == TEXT_LAYOUTS {
            layouts.remove(0);
        }
        let key = (s.font_family.clone(), size, s.font_weight);
        layouts.push(((text.into(), key), layout.clone()));
        Some(layout)
    }
    fn color_brush(&self, color: Color, alpha: f32) -> ComPtr<ID2D1Brush> {
        unsafe { self.brush.SetColor(&color_f(color, alpha)) };
        self.brush.upcast()
    }
    // A brush for a style, whose gradient space maps through `to_world` into the world
    // transform of the draw.
    fn brush(&self, style: &CanvasStyle, alpha: f32, to_world: Transform) -> ComPtr<ID2D1Brush> {
        match style {
            CanvasStyle::Color(color) => self.color_brush(*color, alpha),
            CanvasStyle::Gradient(gradient) => self
                .gradient_brush(&gradient.0, alpha, to_world)
                .unwrap_or_else(|| self.color_brush(Color::rgba(0, 0, 0, 0), 1.0)),
        }
    }
    fn gradient_brush(
        &self,
        gradient: &Arc<Gradient>,
        alpha: f32,
        to_world: Transform,
    ) -> Option<ComPtr<ID2D1Brush>> {
        if gradient.stops.is_empty() {
            return None;
        }
        // Stop collections are resources of the render target.
        let key = (GradientKey(gradient.clone()), self.target_id);
        let collection = self
            .resources
            .gradient_stops
            .borrow_mut()
            .get_or_create(key, || {
                let stops: Vec<D2D1_GRADIENT_STOP> = gradient
                    .stops
                    .iter()
                    .map(|&(position, color)| D2D1_GRADIENT_STOP {
                        position,
                        color: color_f(color, 1.0),
                    })
                    .collect();
                let extend = match gradient.spread {
                    GradientSpread::Pad => D2D1_EXTEND_MODE_CLAMP,
                    GradientSpread::Repeat => D2D1_EXTEND_MODE_WRAP,
                    GradientSpread::Reflect => D2D1_EXTEND_MODE_MIRROR,
                };
                unsafe { self.target.CreateGradientStopCollection(&stops, extend) }.ok()
            })
            .cloned()?;
        let [a, b, c, d, e, f] = to_world.concat(Transform(gradient.transform)).0;
        let properties = D2D1_BRUSH_PROPERTIES {
            opacity: alpha,
            transform: D2D_MATRIX_3X2_F {
                m11: a,
                m12: b,
                m21: c,
                m22: d,
                dx: e,
                dy: f,
            },
        };
        let point = |(x, y): (f32, f32)| D2D_POINT_2F { x, y };
        unsafe {
            match gradient.shape {
                GradientShape::Linear { start, end } => self.target.CreateLinearGradientBrush(
                    &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: point(start),
                        endPoint: point(end),
                    },
                    &properties,
                    collection.as_ptr(),
                ),
                GradientShape::Radial {
                    focal,
                    center,
                    radius,
                } => self.target.CreateRadialGradientBrush(
                    &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                        center: point(center),
                        gradientOriginOffset: point((focal.0 - center.0, focal.1 - center.1)),
                        radiusX: radius,
                        radiusY: radius,
                    },
                    &properties,
                    collection.as_ptr(),
                ),
            }
            .ok()
        }
    }
    fn format(&self, s: &CanvasState) -> ComPtr<IDWriteTextFormat> {
        let size = s.font_size.to_bits();
        let mut formats = self.resources.formats.borrow_mut();
        if let Some((_, format)) = formats.iter().find(|((family, key_size, weight), _)| {
            *family == s.font_family && *key_size == size && *weight == s.font_weight
        }) {
            return format.clone();
        }
        let custom_family;
        let family = match s.font_family.as_ref() {
            "sans-serif" => w!("Segoe UI"),
            "serif" => w!("Times New Roman"),
            "monospace" => w!("Consolas"),
            name => {
                custom_family = wide(name);
                &custom_family
            }
        };
        let locale = w!("en-us");
        let format = unsafe {
            self.resources
                .write
                .CreateTextFormat(
                    family.as_ptr(),
                    null_mut(),
                    match s.font_weight {
                        FontWeight::Normal => DWRITE_FONT_WEIGHT_NORMAL,
                        FontWeight::Bold => DWRITE_FONT_WEIGHT_BOLD,
                    },
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    s.font_size,
                    locale.as_ptr(),
                )
                .expect("DirectWrite text format")
        };
        unsafe {
            let _ = format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR);
        }
        if formats.len() == TEXT_FORMATS {
            formats.remove(0);
        }
        formats.push(((s.font_family.clone(), size, s.font_weight), format.clone()));
        format
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        self.pop_clips(0);
    }
}
fn color_f(color: Color, alpha: f32) -> D2D1_COLOR_F {
    let (r, g, b, a) = color.components(alpha);
    D2D1_COLOR_F {
        r: r as f32,
        g: g as f32,
        b: b as f32,
        a: a as f32,
    }
}

/// A Direct2D bitmap sharing the pixels of a canvas image, created for the device of the
/// render target that last drew it.
pub(crate) struct ImageCache {
    // The bitmap and the id of the draw target it was made for, which keeps no reference to
    // the target itself, so a cached image never keeps a large render target alive.
    bitmap: RefCell<Option<(u64, ComPtr<ID2D1Bitmap>)>>,
}

impl ImageCache {
    pub(crate) const fn new() -> Self {
        Self {
            bitmap: RefCell::new(None),
        }
    }
}

impl ImageCache {
    fn bitmap(
        &self,
        image: &PlatformImage,
        target: &ComPtr<ID2D1RenderTarget>,
        target_id: u64,
    ) -> Option<ComPtr<ID2D1Bitmap>> {
        let mut cache = self.bitmap.borrow_mut();
        if let Some((cached, bitmap)) = &*cache
            && *cached == target_id
        {
            return Some(bitmap.clone());
        }
        let bitmap = unsafe {
            target.CreateBitmap(
                D2D_SIZE_U {
                    width: image.width,
                    height: image.height,
                },
                image.pixels.as_ptr().cast(),
                image.width * 4,
                &D2D1_BITMAP_PROPERTIES {
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                },
            )
        }
        .ok()?;
        *cache = Some((target_id, bitmap.clone()));
        Some(bitmap)
    }
}

// The Direct2D 1.1 interface of a render target, which the backend requires.
fn device_context(target: &ComPtr<ID2D1RenderTarget>) -> ComPtr<ID2D1DeviceContext> {
    target
        .query::<ID2D1DeviceContext>()
        .expect("Direct2D 1.1 device context")
}

const fn matrix(Transform([a, b, c, d, e, f]): Transform) -> D2D_MATRIX_3X2_F {
    D2D_MATRIX_3X2_F {
        m11: a,
        m12: b,
        m21: c,
        m22: d,
        dx: e,
        dy: f,
    }
}

// The Direct2D 1.1 composite mode of a Porter-Duff operation.
const fn composite_mode(operation: CompositeOperation) -> Option<u32> {
    use CompositeOperation::*;
    Some(match operation {
        SourceOver => 0,
        DestinationOver => 1,
        SourceIn => 2,
        DestinationIn => 3,
        SourceOut => 4,
        DestinationOut => 5,
        SourceAtop => 6,
        DestinationAtop => 7,
        Xor => 8,
        Lighter => 9,
        Copy => 10,
        _ => return None,
    })
}

// The blend effect mode of a blend operation.
const fn blend_mode(operation: CompositeOperation) -> Option<u32> {
    use CompositeOperation::*;
    Some(match operation {
        Multiply => 0,
        Screen => 1,
        Darken => 2,
        Lighten => 3,
        ColorBurn => 5,
        ColorDodge => 9,
        Overlay => 11,
        SoftLight => 12,
        HardLight => 13,
        Difference => 18,
        Exclusion => 19,
        Hue => 20,
        Saturation => 21,
        Color => 22,
        Luminosity => 23,
        _ => return None,
    })
}

const fn vector([x, y]: Point) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}
/// The device bounds of a user-space path that is exactly one axis-aligned rectangle.
fn device_rect(path: &Path, transform: Transform) -> Option<D2D_RECT_F> {
    let [
        Command::Move(a),
        Command::Line(b),
        Command::Line(c),
        Command::Line(d),
        Command::Close,
    ] = *path.commands
    else {
        return None;
    };
    let [a, b, c, d] = [a, b, c, d].map(|point| transform.point(point));
    let aligned = (a[1] == b[1] && b[0] == c[0] && c[1] == d[1] && d[0] == a[0])
        || (a[0] == b[0] && b[1] == c[1] && c[0] == d[0] && d[1] == a[1]);
    aligned.then(|| rect(a[0], a[1], c[0] - a[0], c[1] - a[1]))
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F {
        left: x.min(x + w),
        top: y.min(y + h),
        right: x.max(x + w),
        bottom: y.max(y + h),
    }
}
