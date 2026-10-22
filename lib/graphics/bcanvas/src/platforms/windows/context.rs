/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::ops::Deref;
use std::ptr::{null, null_mut};
use std::rc::Rc;

use super::com::ComPtr;
use super::headers::*;
use crate::path::{Command, Path, Point, Transform};
use crate::{CanvasState, Color, FontWeight, LineCap, LineJoin, TextAlign, TextBaseline};

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
}

pub(crate) struct PlatformCanvasContext {
    target: ComPtr<ID2D1RenderTarget>,
    brush: ComPtr<ID2D1SolidColorBrush>,
    resources: Rc<Resources>,
    path: Path,
    transform: Transform,
    stack: Vec<(Transform, usize)>,
    clips: Vec<Clip>,
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
            if matches!(self.clips.pop(), Some(Clip::Rect)) {
                unsafe { self.target.PopAxisAlignedClip() };
            } else {
                unsafe { self.target.PopLayer() };
            }
        }
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
            let brush = self.brush(Color::rgba(0, 0, 0, 0), 1.0);
            unsafe {
                context.SetPrimitiveBlend(D2D1_PRIMITIVE_BLEND_COPY);
                self.target.FillRectangle(&rect, brush);
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
        let brush = self.brush(state.fill, state.alpha);
        unsafe { self.target.FillRectangle(&rect(x, y, w, h), brush) };
    }

    pub(crate) fn stroke_rect(&mut self, state: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let brush = self.brush(state.stroke, state.alpha);
        unsafe {
            self.target.DrawRectangle(
                &rect(x, y, w, h),
                brush,
                state.line_width,
                self.stroke_style(state),
            )
        };
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

    fn geometry(&self, transform: Transform) -> ComPtr<ID2D1PathGeometry> {
        let geometry =
            unsafe { self.resources.factory.CreatePathGeometry() }.expect("Direct2D path");
        let sink = unsafe { geometry.Open() }.expect("Direct2D geometry sink");
        let mut open = false;
        let mut start = [0.0; 2];
        unsafe {
            sink.SetFillMode(D2D1_FILL_MODE_WINDING);
            for command in &self.path.commands {
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

    pub(crate) fn fill(&mut self, state: &CanvasState) {
        let geometry = self.geometry(Transform::IDENTITY);
        let brush = self.brush(state.fill, state.alpha);
        unsafe {
            self.target.SetTransform(&D2D_MATRIX_3X2_F::identity());
            self.target
                .FillGeometry(geometry.as_ptr(), brush, null_mut());
        }
        self.apply_transform();
    }

    pub(crate) fn stroke(&mut self, state: &CanvasState) {
        // Paths capture their construction transform, while strokes use the current
        // transform for width/caps. Undo it on the geometry before native drawing.
        let Some(inverse) = self.transform.inverse() else {
            return;
        };
        let geometry = self.geometry(inverse);
        let brush = self.brush(state.stroke, state.alpha);
        unsafe {
            self.target.DrawGeometry(
                geometry.as_ptr(),
                brush,
                state.line_width,
                self.stroke_style(state),
            )
        };
    }

    pub(crate) fn clip(&mut self) {
        unsafe { self.target.SetTransform(&D2D_MATRIX_3X2_F::identity()) };
        if let Some(rect) = device_rect(&self.path.commands) {
            unsafe {
                self.target
                    .PushAxisAlignedClip(&rect, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE)
            };
            self.clips.push(Clip::Rect);
        } else {
            let geometry = self.geometry(Transform::IDENTITY);
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

    fn stroke_style(&self, state: &CanvasState) -> *mut ID2D1StrokeStyle {
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
        self.resources.strokes[cap * 3 + join].as_ptr()
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
        let [a, b, c, d, e, f] = self.transform.0;
        unsafe {
            self.target.SetTransform(&D2D_MATRIX_3X2_F {
                m11: a,
                m12: b,
                m21: c,
                m22: d,
                dx: e,
                dy: f,
            })
        };
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
        let brush = self.brush(s.fill, s.alpha);
        unsafe {
            self.target
                .DrawTextLayout(origin, layout.as_ptr(), brush, D2D1_DRAW_TEXT_OPTIONS_NONE)
        }
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
    fn brush(&self, color: Color, alpha: f32) -> *mut ID2D1SolidColorBrush {
        let (r, g, b, a) = color.components(alpha);
        unsafe {
            self.brush.SetColor(&D2D1_COLOR_F {
                r: r as f32,
                g: g as f32,
                b: b as f32,
                a: a as f32,
            })
        };
        self.brush.as_ptr()
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
