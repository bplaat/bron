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

use super::com::ComPtr;
use super::headers::*;
use crate::path::{Command, Path, Point, Transform};
use crate::{
    CanvasState, CanvasStyle, Color, CompositeOperation, FillRule, FontWeight, Gradient,
    GradientShape, GradientSpread, LineCap, LineJoin, TextAlign, TextBaseline,
};

const TEXT_FORMATS: usize = 16;

type TextFormatKey = (Rc<str>, u32, FontWeight);

/// Device-independent objects shared by every frame of one canvas.
pub(super) struct Resources {
    pub(super) factory: ComPtr<ID2D1Factory>,
    write: ComPtr<IDWriteFactory>,
    // Indexed by cap * 3 + join, in the order of stroke_style.
    strokes: [ComPtr<ID2D1StrokeStyle>; 9],
    formats: RefCell<Vec<(TextFormatKey, ComPtr<IDWriteTextFormat>)>>,
    text: RefCell<Vec<u16>>,
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
        Rc::new(Self {
            factory,
            write: unsafe { IDWriteFactory::new() }.expect("DirectWrite factory"),
            strokes,
            formats: RefCell::new(Vec::new()),
            text: RefCell::new(Vec::new()),
        })
    }
}

/// A render target with the one brush that every draw recolors.
pub(super) struct DrawTarget<T> {
    pub(super) native: ComPtr<T>,
    brush: ComPtr<ID2D1SolidColorBrush>,
}

impl<T: Deref<Target = ID2D1RenderTarget>> DrawTarget<T> {
    pub(super) fn new(native: ComPtr<T>) -> Result<Self, i32> {
        let brush = unsafe { native.CreateSolidColorBrush(&D2D1_COLOR_F::default(), null()) }?;
        Ok(Self { native, brush })
    }
}

impl<T> Clone for DrawTarget<T> {
    fn clone(&self) -> Self {
        Self {
            native: self.native.clone(),
            brush: self.brush.clone(),
        }
    }
}

enum Clip {
    Rect,
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

pub(crate) struct PlatformCanvasContext {
    target: ComPtr<ID2D1RenderTarget>,
    brush: ComPtr<ID2D1SolidColorBrush>,
    resources: Rc<Resources>,
    path: Path,
    transform: Transform,
    stack: Vec<(Transform, usize)>,
    clips: Vec<Clip>,
    offscreens: Vec<Offscreen>,
    layers: Vec<Layer>,
}

impl PlatformCanvasContext {
    pub(super) fn new<T: Deref<Target = ID2D1RenderTarget>>(
        target: &DrawTarget<T>,
        resources: Rc<Resources>,
    ) -> Self {
        let native = target.native.upcast();
        unsafe { native.SetTransform(&D2D_MATRIX_3X2_F::identity()) };
        Self {
            target: native,
            brush: target.brush.clone(),
            resources,
            path: Path::default(),
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
        if matches!(clip, Clip::Rect) {
            unsafe { self.target.PopAxisAlignedClip() };
        } else {
            unsafe { self.target.PopLayer() };
        }
        Some(clip)
    }

    pub(crate) fn begin_layer(&mut self, s: &CanvasState) {
        if s.composite == CompositeOperation::SourceOver {
            self.push_group(s.alpha, None, self.transform);
            self.layers.push(Layer::Group { composited: false });
        } else {
            self.begin_offscreen(s.alpha, s.composite);
            self.layers.push(Layer::Composited);
        }
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
        let outer_brush = std::mem::replace(&mut self.brush, brush);
        let outer_clips = std::mem::take(&mut self.clips);
        self.offscreens.push(Offscreen {
            outer,
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
        let Ok(context) = (unsafe {
            self.target
                .query::<ID2D1DeviceContext>(&IID_ID2D1_DEVICE_CONTEXT)
        }) else {
            unsafe {
                self.target.DrawBitmap(
                    bitmap.as_ptr(),
                    null(),
                    alpha,
                    D2D1_BITMAP_INTERPOLATION_MODE_LINEAR,
                )
            };
            return;
        };
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
                // The blend effect mixes the image with a snapshot of the target.
                let blended = unsafe { self.snapshot() }.and_then(|snapshot| unsafe {
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
            bitmap.CopyFromRenderTarget(self.target.as_ptr()).ok()?;
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
            let context = (blend != D2D1_PRIMITIVE_BLEND_SOURCE_OVER)
                .then(|| unsafe {
                    self.target
                        .query::<ID2D1DeviceContext>(&IID_ID2D1_DEVICE_CONTEXT)
                })
                .and_then(Result::ok);
            if let Some(context) = &context {
                unsafe { context.SetPrimitiveBlend(blend) };
            }
            draw(self, state);
            if let Some(context) = &context {
                unsafe { context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER) };
            }
            return;
        }
        let Some(bounds) = bounds(self) else {
            return;
        };
        self.push_clip_layer(&bounds);
        self.begin_offscreen(1.0, state.composite);
        draw(
            self,
            &CanvasState {
                composite: CompositeOperation::SourceOver,
                ..state.clone()
            },
        );
        self.end_composited();
        self.pop_clip();
    }

    fn push_clip_layer(&mut self, geometry: &ComPtr<ID2D1PathGeometry>) {
        let params = D2D1_LAYER_PARAMETERS {
            contentBounds: rect(-1.0e10, -1.0e10, 2.0e10, 2.0e10),
            geometricMask: geometry.as_ptr(),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: D2D_MATRIX_3X2_F::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        unsafe {
            self.target.SetTransform(&D2D_MATRIX_3X2_F::identity());
            self.target.PushLayer(&params, null_mut());
        }
        self.clips.push(Clip::Layer);
        self.apply_transform();
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
        path.rect(self.transform, x, y, w, h);
        self.path_geometry(&path, Transform::IDENTITY, FillRule::NonZero)
    }

    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let rect = rect(x, y, w, h);
        // An axis-aligned clip is the transformed bounding box, so rotated or skewed
        // rects instead copy a transparent fill through the Direct2D 1.1 interface.
        let [a, b, c, d, _, _] = self.transform.0;
        if !((b == 0.0 && c == 0.0) || (a == 0.0 && d == 0.0))
            && let Ok(context) = unsafe {
                self.target
                    .query::<ID2D1DeviceContext>(&IID_ID2D1_DEVICE_CONTEXT)
            }
        {
            let brush = self.color_brush(Color::rgba(0, 0, 0, 0), 1.0);
            unsafe {
                context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
                self.target.FillRectangle(&rect, brush.as_ptr());
                context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_SOURCE_OVER);
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
                path.rect(Transform::IDENTITY, x, y, w, h);
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

    pub(crate) fn begin_path(&mut self) {
        // Keep the command buffer allocation for the next path.
        let mut commands = std::mem::take(&mut self.path.commands);
        commands.clear();
        self.path = Path::default();
        self.path.commands = commands;
    }
    pub(crate) fn close_path(&mut self) {
        self.path.close();
    }
    pub(crate) fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to(self.transform, [x, y]);
    }
    pub(crate) fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.transform, [x, y]);
    }
    pub(crate) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.path.rect(self.transform, x, y, w, h);
    }
    pub(crate) fn ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32) {
        self.path.ellipse(self.transform, [x, y], [rx, ry]);
    }
    pub(crate) fn arc(&mut self, x: f32, y: f32, r: f32, start: f32, end: f32, ccw: bool) {
        self.path
            .arc(self.transform, [x, y], [r, r], start, end, ccw);
    }
    pub(crate) fn quadratic_curve_to(&mut self, a: f32, b: f32, x: f32, y: f32) {
        self.path.quadratic(self.transform, [a, b], [x, y]);
    }
    pub(crate) fn bezier_curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, x: f32, y: f32) {
        self.path.cubic(self.transform, [a, b], [c, d], [x, y]);
    }

    fn geometry(&self, transform: Transform, rule: FillRule) -> ComPtr<ID2D1PathGeometry> {
        self.path_geometry(&self.path, transform, rule)
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
        let mut open = false;
        let mut start = [0.0; 2];
        unsafe {
            sink.SetFillMode(match rule {
                FillRule::NonZero => D2D1_FILL_MODE_WINDING,
                FillRule::EvenOdd => D2D1_FILL_MODE_ALTERNATE,
            });
            for command in &path.commands {
                if let Command::Move(point) = *command {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    start = point;
                    sink.BeginFigure(vector(transform.point(point)), D2D1_FIGURE_BEGIN_FILLED);
                    open = true;
                    continue;
                }
                if !open {
                    sink.BeginFigure(vector(transform.point(start)), D2D1_FIGURE_BEGIN_FILLED);
                    open = true;
                }
                match *command {
                    Command::Move(_) => unreachable!(),
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

    pub(crate) fn fill(&mut self, state: &CanvasState, rule: FillRule) {
        // Paths are in device space, so the brush maps from the current transform.
        let geometry = self.geometry(Transform::IDENTITY, rule);
        self.draw_with(
            state,
            |_| Some(geometry.clone()),
            |this, state| {
                let brush = this.brush(&state.fill, state.alpha, this.transform);
                unsafe {
                    this.target.SetTransform(&D2D_MATRIX_3X2_F::identity());
                    this.target
                        .FillGeometry(geometry.as_ptr(), brush.as_ptr(), null_mut());
                }
                this.apply_transform();
            },
        );
    }

    pub(crate) fn stroke(&mut self, state: &CanvasState) {
        // Paths capture their construction transform, while strokes use the current
        // transform for width/caps. Undo it on the geometry before native drawing.
        let Some(inverse) = self.transform.inverse() else {
            return;
        };
        let geometry = self.geometry(inverse, FillRule::NonZero);
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
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) {
        let rect = rect(x, y, w, h);
        self.draw_with(
            state,
            |this| Some(this.rect_geometry(x, y, w, h)),
            |this, state| {
                let Some(bitmap) = image.bitmap(&this.target) else {
                    return;
                };
                let context = unsafe {
                    this.target
                        .query::<ID2D1DeviceContext>(&IID_ID2D1_DEVICE_CONTEXT)
                };
                unsafe {
                    match &context {
                        Ok(context) => context.DrawBitmap(
                            bitmap.as_ptr(),
                            &rect,
                            state.alpha,
                            if state.image_smoothing {
                                D2D1_INTERPOLATION_MODE_HIGH_QUALITY_CUBIC
                            } else {
                                D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR
                            },
                        ),
                        Err(_) => this.target.DrawBitmap(
                            bitmap.as_ptr(),
                            &rect,
                            state.alpha,
                            if state.image_smoothing {
                                D2D1_BITMAP_INTERPOLATION_MODE_LINEAR
                            } else {
                                D2D1_BITMAP_INTERPOLATION_MODE_NEAREST_NEIGHBOR
                            },
                        ),
                    }
                }
            },
        );
    }

    pub(crate) fn clip(&mut self, rule: FillRule) {
        unsafe { self.target.SetTransform(&D2D_MATRIX_3X2_F::identity()) };
        if let Some(rect) = device_rect(&self.path.commands) {
            unsafe {
                self.target
                    .PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            self.clips.push(Clip::Rect);
        } else {
            let geometry = self.geometry(Transform::IDENTITY, rule);
            let params = D2D1_LAYER_PARAMETERS {
                contentBounds: rect(-1.0e10, -1.0e10, 2.0e10, 2.0e10),
                geometricMask: geometry.as_ptr(),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: D2D_MATRIX_3X2_F::identity(),
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
        let Some((layout, metrics)) = self.prepare_text(s, text) else {
            return;
        };
        let mut line = DWRITE_LINE_METRICS::default();
        let mut count = 0;
        if unsafe { layout.GetLineMetrics(&mut line, 1, &mut count) }.is_err() {
            return;
        }
        let width = metrics.widthIncludingTrailingWhitespace;
        let origin = D2D_POINT_2F {
            x: x - match s.text_align {
                TextAlign::Center => width / 2.0,
                TextAlign::Right | TextAlign::End => width,
                _ => 0.0,
            },
            y: y - match s.text_baseline {
                TextBaseline::Middle => line.height / 2.0,
                TextBaseline::Bottom | TextBaseline::Ideographic => line.height,
                TextBaseline::Alphabetic => line.baseline,
                _ => 0.0,
            },
        };
        self.draw_with(
            s,
            |this| Some(this.rect_geometry(origin.x, origin.y, width, line.height)),
            |this, s| {
                let brush = this.brush(&s.fill, s.alpha, Transform::IDENTITY);
                unsafe {
                    this.target.DrawTextLayout(
                        origin,
                        layout.as_ptr(),
                        brush.as_ptr(),
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    )
                };
            },
        );
    }
    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> f32 {
        self.prepare_text(s, text)
            .map_or(0.0, |(_, metrics)| metrics.widthIncludingTrailingWhitespace)
    }

    fn prepare_text(
        &self,
        s: &CanvasState,
        text: &str,
    ) -> Option<(ComPtr<IDWriteTextLayout>, DWRITE_TEXT_METRICS)> {
        let format = self.format(s);
        let mut wide = self.resources.text.borrow_mut();
        wide.clear();
        // Canvas text is one line: line breaks render as spaces.
        wide.extend(text.encode_utf16().map(|unit| match unit {
            0x09..=0x0d | 0x85 | 0x2028 | 0x2029 => 0x20,
            unit => unit,
        }));
        unsafe {
            let layout = self
                .resources
                .write
                .CreateTextLayout(
                    wide.as_ptr(),
                    wide.len() as u32,
                    format.as_ptr(),
                    100000.0,
                    10000.0,
                )
                .ok()?;
            let mut metrics = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut metrics).ok()?;
            Some((layout, metrics))
        }
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
        gradient: &Gradient,
        alpha: f32,
        to_world: Transform,
    ) -> Option<ComPtr<ID2D1Brush>> {
        if gradient.stops.is_empty() {
            return None;
        }
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
            let collection = self
                .target
                .CreateGradientStopCollection(&stops, extend)
                .ok()?;
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
        let family = wide_string(match s.font_family.as_ref() {
            "sans-serif" => "Segoe UI",
            "serif" => "Times New Roman",
            "monospace" => "Consolas",
            name => name,
        });
        let locale = wide_string("en-us");
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
impl Drop for PlatformCanvasContext {
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
pub(crate) struct PlatformImage {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pixels: Arc<[u8]>,
    bitmap: RefCell<Option<(*mut c_void, ComPtr<ID2D1Bitmap>)>>,
}

impl PlatformImage {
    pub(crate) const fn new(width: u32, height: u32, pixels: Arc<[u8]>) -> Self {
        Self {
            width,
            height,
            pixels,
            bitmap: RefCell::new(None),
        }
    }

    fn bitmap(&self, target: &ComPtr<ID2D1RenderTarget>) -> Option<ComPtr<ID2D1Bitmap>> {
        let key = target.as_ptr().cast::<c_void>();
        let mut cache = self.bitmap.borrow_mut();
        if let Some((cached, bitmap)) = &*cache
            && *cached == key
        {
            return Some(bitmap.clone());
        }
        let bitmap = unsafe {
            target.CreateBitmap(
                D2D_SIZE_U {
                    width: self.width,
                    height: self.height,
                },
                self.pixels.as_ptr().cast(),
                self.width * 4,
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
        *cache = Some((key, bitmap.clone()));
        Some(bitmap)
    }
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
/// The device bounds of a path that is exactly one axis-aligned rectangle.
fn device_rect(commands: &[Command]) -> Option<D2D_RECT_F> {
    let [
        Command::Move(a),
        Command::Line(b),
        Command::Line(c),
        Command::Line(d),
        Command::Close,
    ] = *commands
    else {
        return None;
    };
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
