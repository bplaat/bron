/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Arc;

use super::composite::{Plane, Rect, Target, composite};
use super::gradient;
use super::headers::*;
use crate::path::{Command, Path, Transform};
use crate::platforms::windows::headers::{
    BITMAPINFOHEADER, DIB_RGB_COLORS, DeleteObject, FALSE, GdiFlush, HANDLE, HDC, SelectObject,
    TRUE, w, wide,
};
use crate::platforms::windows::image::PlatformImage;
use crate::{
    CanvasState, CanvasStyle, Color, CompositeOperation, FillRule, FontWeight, LineCap, LineJoin,
    TextAlign, TextExtents,
};

const FONTS: usize = 16;

type FontKey = (Arc<str>, u32, FontWeight);

/// A GDI+ font with its metrics in units of its size.
struct Font {
    font: Gp<GpFont>,
    style: i32,
    ascent: f32,
    line_height: f32,
}

/// Objects shared by every frame of one canvas.
pub(crate) struct Resources {
    format: Gp<GpStringFormat>,
    fonts: RefCell<Vec<(FontKey, Rc<Font>)>>,
    text: RefCell<Vec<u16>>,
    // The brush of the last GDI fill, most fills repeat a color
    pattern: RefCell<Option<PatternBrush>>,
}

impl Resources {
    pub(crate) fn new() -> Rc<Self> {
        startup();
        // The typographic format measures and draws without the padding of the generic
        // default format, like DirectWrite.
        let format = unsafe {
            let mut typographic = null_mut();
            let status = GdipStringFormatGetGenericTypographic(&mut typographic);
            assert_eq!(status, GP_OK, "GDI+ string format");
            Gp::create(|out| GdipCloneStringFormat(typographic, out)).expect("GDI+ string format")
        };
        unsafe {
            GdipSetStringFormatFlags(
                format.get(),
                StringFormatFlagsMeasureTrailingSpaces
                    | StringFormatFlagsNoWrap
                    | StringFormatFlagsNoClip,
            )
        };
        Rc::new(Self {
            format,
            fonts: RefCell::new(Vec::new()),
            text: RefCell::new(Vec::new()),
            pattern: RefCell::new(None),
        })
    }
}

/// A GDI brush of one premultiplied BGRA pixel. GDI's solid brushes clear the alpha of
/// 32-bit bitmaps, a 32-bit pattern keeps it.
struct PatternBrush {
    pixel: u32,
    brush: HANDLE,
}

impl PatternBrush {
    fn new(pixel: u32) -> Option<Self> {
        // An 8x8 pattern, the size every Windows version takes
        #[repr(C)]
        struct Pattern {
            header: BITMAPINFOHEADER,
            pixels: [u32; 64],
        }
        let pattern = Pattern {
            header: BITMAPINFOHEADER {
                size: size_of::<BITMAPINFOHEADER>() as u32,
                width: 8,
                height: 8,
                planes: 1,
                bit_count: 32,
                ..Default::default()
            },
            pixels: [pixel; 64],
        };
        let brush = unsafe { CreateDIBPatternBrushPt((&raw const pattern).cast(), DIB_RGB_COLORS) };
        (!brush.is_null()).then_some(Self { pixel, brush })
    }
}

impl Drop for PatternBrush {
    fn drop(&mut self) {
        unsafe { DeleteObject(self.brush) };
    }
}

fn intersect([ax, ay, aw, ah]: Rect, [bx, by, bw, bh]: Rect) -> Rect {
    let left = ax.max(bx);
    let top = ay.max(by);
    let right = (ax + aw).min(bx + bw).max(left);
    let bottom = (ay + ah).min(by + bh).max(top);
    [left, top, right - left, bottom - top]
}

/// A GDI+ graphics drawing into premultiplied BGRA pixels, which cover a rectangle of the
/// canvas. Layers own their pixels and only cover the area they can change.
pub(crate) struct Surface {
    // Deleted before the bitmap and pixels it draws into.
    graphics: Gp<GpGraphics>,
    // The world transform, reused instead of a new matrix for every drawing.
    world: Gp<GpMatrix>,
    _bitmap: Gp<GpImage>,
    pixels: *mut u8,
    rect: Rect,
    // The device context of a canvas bitmap, which GDI fills, null for a layer.
    dc: HDC,
    _storage: Option<Box<[u8]>>,
}

impl Surface {
    /// # Safety
    /// The pixels and device context of the bitmap must outlive the surface.
    pub(crate) unsafe fn new(pixels: *mut u8, width: u32, height: u32, dc: HDC) -> Option<Self> {
        unsafe { Self::at(pixels, [0, 0, width as i32, height as i32], dc) }
    }

    unsafe fn at(pixels: *mut u8, rect: Rect, dc: HDC) -> Option<Self> {
        unsafe {
            let bitmap = Gp::create(|out| {
                GdipCreateBitmapFromScan0(
                    rect[2],
                    rect[3],
                    rect[2] * 4,
                    PixelFormat32bppPARGB,
                    pixels,
                    out,
                )
            })?;
            let graphics = Gp::create(|out| GdipGetImageGraphicsContext(bitmap.get(), out))?;
            let g = graphics.get();
            GdipSetPageUnit(g, UnitPixel);
            GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            // Pixel centers at half coordinates, like Direct2D.
            GdipSetPixelOffsetMode(g, PixelOffsetModeHalf);
            GdipSetTextRenderingHint(g, TextRenderingHintAntiAlias);
            GdipSetInterpolationMode(g, InterpolationModeHighQualityBicubic);
            let world = Gp::create(|out| GdipCreateMatrix2(1.0, 0.0, 0.0, 1.0, 0.0, 0.0, out))?;
            let surface = Self {
                graphics,
                world,
                _bitmap: bitmap,
                pixels,
                rect,
                dc,
                _storage: None,
            };
            surface.set_world(Transform::IDENTITY);
            Some(surface)
        }
    }

    /// A transparent surface with its own pixels over a nonempty part of the canvas.
    fn owned(rect: Rect) -> Self {
        let rect = [rect[0], rect[1], rect[2].max(1), rect[3].max(1)];
        let mut storage = vec![0u8; rect[2] as usize * rect[3] as usize * 4].into_boxed_slice();
        let mut surface = unsafe { Self::at(storage.as_mut_ptr(), rect, null_mut()) }
            .expect("GDI+ layer surface");
        surface._storage = Some(storage);
        surface
    }

    const fn g(&self) -> *mut GpGraphics {
        self.graphics.get()
    }

    /// Map canvas device pixels through `transform` into the pixels of the surface.
    fn set_world(&self, transform: Transform) {
        let origin = Transform([
            1.0,
            0.0,
            0.0,
            1.0,
            -self.rect[0] as f32,
            -self.rect[1] as f32,
        ]);
        let Transform([a, b, c, d, e, f]) = origin.concat(transform);
        unsafe {
            GdipSetMatrixElements(self.world.get(), a, b, c, d, e, f);
            GdipSetWorldTransform(self.g(), self.world.get());
        }
    }

    const fn len(&self) -> usize {
        self.rect[2] as usize * self.rect[3] as usize * 4
    }

    /// The pixels after the drawing so far.
    fn plane(&self) -> Plane<'_> {
        unsafe {
            GdipFlush(self.g(), FlushIntentionSync);
            Plane {
                pixels: std::slice::from_raw_parts(self.pixels, self.len()),
                rect: self.rect,
            }
        }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        unsafe {
            GdipFlush(self.g(), FlushIntentionSync);
            std::slice::from_raw_parts_mut(self.pixels, self.len())
        }
    }

    /// Fills a device rectangle with a pattern brush through GDI, which copies many times
    /// faster than GDI+ fills. False for a layer, which has no device context. It ignores the
    /// GDI+ clip.
    fn pattern_fill(&self, [x, y, width, height]: Rect, brush: &PatternBrush) -> bool {
        if self.dc.is_null() {
            return false;
        }
        unsafe {
            GdipFlush(self.g(), FlushIntentionSync);
            let previous = SelectObject(self.dc, brush.brush);
            let filled = PatBlt(
                self.dc,
                x - self.rect[0],
                y - self.rect[1],
                width,
                height,
                PATCOPY,
            );
            SelectObject(self.dc, previous);
            // GDI+ draws next into the same pixels
            GdiFlush();
            filled != FALSE
        }
    }

    // The clip in device pixels of the canvas.
    fn clip(&self) -> Option<Gp<GpRegion>> {
        unsafe {
            let region = Gp::create(|out| GdipCreateRegion(out))?;
            self.set_world(Transform::IDENTITY);
            (GdipGetClip(self.g(), region.get()) == GP_OK).then_some(region)
        }
    }

    fn set_clip(&self, region: &Gp<GpRegion>) {
        self.set_world(Transform::IDENTITY);
        unsafe { GdipSetClipRegion(self.g(), region.get(), CombineModeReplace) };
    }
}

// A drawing into a layer, which becomes a mask or is composited when it ends.
struct Offscreen {
    outer: Option<Surface>,
    outer_clips: Vec<Gp<GpRegion>>,
    alpha: f32,
    transform: Transform,
    operation: CompositeOperation,
    // The mask of a masked layer, while its content is drawn.
    mask: Option<Surface>,
}

// An open canvas layer: masked content, which can be inside a composited layer, or a
// composited layer alone.
#[derive(Clone, Copy)]
enum Layer {
    Group { composited: bool },
    Composited,
}

pub(crate) struct Context {
    surface: Surface,
    resources: Rc<Resources>,
    // Device pixels per logical pixel.
    scale: f32,
    transform: Transform,
    stack: Vec<(Transform, usize)>,
    // The clip of the surface before each clip, in device pixels.
    clips: Vec<Gp<GpRegion>>,
    offscreens: Vec<Offscreen>,
    layers: Vec<Layer>,
}

impl Context {
    pub(crate) const fn new(surface: Surface, resources: Rc<Resources>, scale: f32) -> Self {
        Self {
            surface,
            resources,
            scale,
            transform: Transform::IDENTITY,
            stack: Vec::new(),
            clips: Vec::new(),
            offscreens: Vec::new(),
            layers: Vec::new(),
        }
    }

    // Logical to device pixels.
    const fn device(&self) -> Transform {
        Transform([self.scale, 0.0, 0.0, self.scale, 0.0, 0.0])
    }

    // The device transform of user space.
    fn user(&self) -> Transform {
        self.device().concat(self.transform)
    }

    fn set_world(&self, transform: Transform) {
        self.surface.set_world(transform);
    }

    fn new_matrix(Transform([a, b, c, d, e, f]): Transform) -> Option<Gp<GpMatrix>> {
        unsafe { Gp::create(|out| GdipCreateMatrix2(a, b, c, d, e, f, out)) }
    }

    pub(crate) fn save(&mut self) {
        self.stack.push((self.transform, self.clips.len()));
    }

    pub(crate) fn restore(&mut self) {
        if let Some((transform, clips)) = self.stack.pop() {
            self.pop_clips(clips);
            self.transform = transform;
        }
    }

    fn pop_clips(&mut self, depth: usize) {
        if self.clips.len() <= depth {
            return;
        }
        let region = self.clips.drain(depth..).next().expect("saved clip");
        self.surface.set_clip(&region);
    }

    pub(crate) fn clip(&mut self, path: &Path, rule: FillRule) {
        let Some(saved) = self.surface.clip() else {
            return;
        };
        let Some(path) = self.gp_path(path, self.transform, rule) else {
            return;
        };
        self.set_world(self.device());
        unsafe { GdipSetClipPath(self.surface.g(), path.get(), CombineModeIntersect) };
        self.clips.push(saved);
    }

    // The device rectangle that drawing can change, the surface within the clip.
    fn clip_bounds(&self) -> Rect {
        if self.clips.is_empty() {
            return self.surface.rect;
        }
        let mut bounds = RectF::default();
        self.set_world(Transform::IDENTITY);
        if unsafe { GdipGetClipBounds(self.surface.g(), &mut bounds) } != GP_OK {
            return self.surface.rect;
        }
        let left = bounds.x.floor() as i32;
        let top = bounds.y.floor() as i32;
        let right = (bounds.x + bounds.width).ceil() as i32;
        let bottom = (bounds.y + bounds.height).ceil() as i32;
        intersect([left, top, right - left, bottom - top], self.surface.rect)
    }

    pub(crate) fn begin_layer(&mut self, s: &CanvasState) {
        self.begin_offscreen(s.alpha, s.composite, self.clip_bounds());
        self.layers.push(Layer::Composited);
    }

    pub(crate) fn begin_mask_layer(&mut self, s: &CanvasState) {
        // Other operations composite the masked layer as a whole.
        let composited = s.composite != CompositeOperation::SourceOver;
        let bounds = self.clip_bounds();
        let alpha = if composited {
            self.begin_offscreen(s.alpha, s.composite, bounds);
            1.0
        } else {
            s.alpha
        };
        self.begin_offscreen(alpha, CompositeOperation::SourceOver, bounds);
        self.layers.push(Layer::Group { composited });
    }

    pub(crate) fn end_mask(&mut self) {
        if !matches!(self.layers.last(), Some(Layer::Group { .. })) {
            return;
        }
        if self.offscreens.last().is_some_and(|o| o.mask.is_some()) {
            return;
        }
        // The mask drawing becomes the mask of the content drawn next, over the same area.
        let (offscreen, mask) = self.finish_offscreen();
        self.transform = offscreen.transform;
        self.begin_offscreen(offscreen.alpha, CompositeOperation::SourceOver, mask.rect);
        self.offscreens.last_mut().expect("content layer").mask = Some(mask);
    }

    pub(crate) fn end_layer(&mut self) {
        let Some(layer) = self.layers.pop() else {
            return;
        };
        if let Layer::Group { composited } = layer {
            if self.offscreens.last().is_some_and(|o| o.mask.is_none()) {
                self.layers.push(layer);
                self.end_mask();
                self.layers.pop();
            }
            let (offscreen, content) = self.finish_offscreen();
            let coverage = self.clip_coverage(content.rect);
            let destination = self.surface.rect;
            composite(
                CompositeOperation::SourceOver,
                Target {
                    pixels: self.surface.pixels_mut(),
                    rect: destination,
                },
                content.plane(),
                offscreen.alpha,
                offscreen.mask.as_ref().map(Surface::plane),
                coverage.as_ref().map(Surface::plane),
                content.rect,
            );
            self.transform = offscreen.transform;
            if !composited {
                return;
            }
        }
        self.end_composited();
    }

    // Redirects drawing into a transparent layer over the device rectangle `rect` of the
    // surface, which starts without clips.
    fn begin_offscreen(&mut self, alpha: f32, operation: CompositeOperation, rect: Rect) {
        let layer = Surface::owned(rect);
        let outer = std::mem::replace(&mut self.surface, layer);
        let outer_clips = std::mem::take(&mut self.clips);
        self.offscreens.push(Offscreen {
            outer: Some(outer),
            outer_clips,
            alpha,
            transform: self.transform,
            operation,
            mask: None,
        });
    }

    // Returns drawing to the surface of before the layer, with the layer.
    fn finish_offscreen(&mut self) -> (Offscreen, Surface) {
        self.pop_clips(0);
        let mut offscreen = self.offscreens.pop().expect("open offscreen");
        let outer = offscreen.outer.take().expect("outer surface");
        let layer = std::mem::replace(&mut self.surface, outer);
        self.clips = std::mem::take(&mut offscreen.outer_clips);
        (offscreen, layer)
    }

    fn end_composited(&mut self) {
        let (offscreen, layer) = self.finish_offscreen();
        let coverage = self.clip_coverage(layer.rect);
        let destination = self.surface.rect;
        composite(
            offscreen.operation,
            Target {
                pixels: self.surface.pixels_mut(),
                rect: destination,
            },
            layer.plane(),
            offscreen.alpha,
            None,
            coverage.as_ref().map(Surface::plane),
            layer.rect,
        );
        self.transform = offscreen.transform;
    }

    // The coverage of a white fill under the current clip over a device rectangle.
    fn coverage(&self, rect: Rect, fill: impl FnOnce(&Surface, *mut GpBrush)) -> Option<Surface> {
        let surface = Surface::owned(rect);
        if !self.clips.is_empty() {
            surface.set_clip(&self.surface.clip()?);
        }
        unsafe {
            let brush = Gp::create(|out| GdipCreateSolidFill(0xffff_ffff, out))?;
            fill(&surface, brush.get());
        }
        Some(surface)
    }

    // The coverage of the clip over a device rectangle, `None` without a clip.
    fn clip_coverage(&self, rect: Rect) -> Option<Surface> {
        if self.clips.is_empty() {
            return None;
        }
        self.coverage(rect, |surface, brush| unsafe {
            surface.set_world(Transform::IDENTITY);
            GdipFillRectangle(
                surface.g(),
                brush,
                rect[0] as f32,
                rect[1] as f32,
                rect[2] as f32,
                rect[3] as f32,
            );
        })
    }

    // Draws with the composite operation. Source over draws directly, other operations
    // composite a layer of the drawing limited to the shape of `bounds` in logical pixels,
    // so they only change the drawn area like on the other backends.
    fn draw_with(
        &mut self,
        state: &CanvasState,
        bounds: impl FnOnce(&Self) -> Option<Gp<GpPath>>,
        draw: impl FnOnce(&Self, &CanvasState),
    ) {
        if state.composite == CompositeOperation::SourceOver {
            draw(self, state);
            return;
        }
        let Some(bounds) = bounds(self) else {
            return;
        };
        let rect = self.path_bounds(&bounds, None);
        if rect[2] == 0 || rect[3] == 0 {
            return;
        }
        self.begin_offscreen(1.0, state.composite, rect);
        draw(
            self,
            &CanvasState {
                composite: CompositeOperation::SourceOver,
                ..state.clone()
            },
        );
        let (offscreen, layer) = self.finish_offscreen();
        let device = self.device();
        let Some(coverage) = self.coverage(rect, |surface, brush| unsafe {
            surface.set_world(device);
            GdipFillPath(surface.g(), brush, bounds.get());
        }) else {
            return;
        };
        let destination = self.surface.rect;
        composite(
            offscreen.operation,
            Target {
                pixels: self.surface.pixels_mut(),
                rect: destination,
            },
            layer.plane(),
            1.0,
            None,
            Some(coverage.plane()),
            rect,
        );
    }

    pub(crate) fn clear_rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        let (x, y, w, h) = normalized(x, y, w, h);
        if let Some(rect) = self.pixel_rect(x, y, w, h)
            && self.clips.is_empty()
            && self.pattern_fill(rect, 0)
        {
            return;
        }
        self.set_world(self.user());
        unsafe {
            let Some(brush) = Gp::create(|out| GdipCreateSolidFill(0, out)) else {
                return;
            };
            let g = self.surface.g();
            GdipSetCompositingMode(g, CompositingModeSourceCopy);
            GdipSetSmoothingMode(g, SmoothingModeNone);
            GdipFillRectangle(g, brush.get(), x, y, w, h);
            GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
            GdipSetCompositingMode(g, CompositingModeSourceOver);
        }
    }

    // The logical geometry of a user-space rectangle.
    fn rect_path(&self, x: f32, y: f32, w: f32, h: f32) -> Option<Gp<GpPath>> {
        let mut path = Path::default();
        path.rect(x, y, w, h);
        self.gp_path(&path, self.transform, FillRule::NonZero)
    }

    // Fills device pixels with one premultiplied BGRA pixel through GDI, false when the
    // surface is a layer.
    fn pattern_fill(&self, rect: Rect, pixel: u32) -> bool {
        let mut pattern = self.resources.pattern.borrow_mut();
        if pattern.as_ref().is_none_or(|brush| brush.pixel != pixel) {
            *pattern = None;
            *pattern = PatternBrush::new(pixel);
        }
        pattern
            .as_ref()
            .is_some_and(|brush| self.surface.pattern_fill(rect, brush))
    }

    // The device pixels of a user-space rectangle that lies on whole device pixels, whose
    // fills look the same without antialiasing
    fn pixel_rect(&self, x: f32, y: f32, w: f32, h: f32) -> Option<Rect> {
        let user = self.user();
        if user.0[1] != 0.0 || user.0[2] != 0.0 {
            return None;
        }
        let [x0, y0] = user.point([x, y]);
        let [x1, y1] = user.point([x + w, y + h]);
        let edges = [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)];
        if !edges
            .iter()
            .all(|v| (v - v.round()).abs() < 1e-3 && v.abs() < 1e9)
        {
            return None;
        }
        let [left, top, right, bottom] = edges.map(|v| v.round() as i32);
        Some([left, top, right - left, bottom - top])
    }

    pub(crate) fn fill_rect(&mut self, state: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let rect = self.pixel_rect(x, y, w, h);
        // A solid color on whole device pixels fills directly without a path, like a page
        // background: GDI copies an opaque color without the clip, GDI+ without antialiasing
        if let Some(rect) = rect
            && state.composite == CompositeOperation::SourceOver
            && let CanvasStyle::Color(color) = &state.fill
        {
            let argb = Self::argb(*color, state.alpha);
            let opaque = argb >> 24 == 0xff;
            if opaque && self.clips.is_empty() && self.pattern_fill(rect, argb) {
                return;
            }
            let Some(brush) = Self::color_brush(*color, state.alpha) else {
                return;
            };
            let g = self.surface.g();
            self.set_world(Transform::IDENTITY);
            unsafe {
                // An opaque color replaces the pixels, which skips blending
                if opaque {
                    GdipSetCompositingMode(g, CompositingModeSourceCopy);
                }
                GdipSetSmoothingMode(g, SmoothingModeNone);
                GdipFillRectangle(
                    g,
                    brush.get(),
                    rect[0] as f32,
                    rect[1] as f32,
                    rect[2] as f32,
                    rect[3] as f32,
                );
                GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
                GdipSetCompositingMode(g, CompositingModeSourceOver);
            }
            return;
        }
        let Some(path) = self.rect_path(x, y, w, h) else {
            return;
        };
        if rect.is_some() {
            unsafe { GdipSetSmoothingMode(self.surface.g(), SmoothingModeNone) };
        }
        self.fill_path(state, path);
        if rect.is_some() {
            unsafe { GdipSetSmoothingMode(self.surface.g(), SmoothingModeAntiAlias) };
        }
    }

    pub(crate) fn stroke_rect(&mut self, state: &CanvasState, x: f32, y: f32, w: f32, h: f32) {
        let mut path = Path::default();
        path.rect(x, y, w, h);
        let Some(path) = self.gp_path(&path, Transform::IDENTITY, FillRule::NonZero) else {
            return;
        };
        self.stroke_path(state, path);
    }

    // A GDI+ path of logical path commands mapped through `transform`.
    fn gp_path(&self, path: &Path, transform: Transform, rule: FillRule) -> Option<Gp<GpPath>> {
        let mode = match rule {
            FillRule::NonZero => FillModeWinding,
            FillRule::EvenOdd => FillModeAlternate,
        };
        let gp = unsafe { Gp::create(|out| GdipCreatePath(mode, out)) }?;
        let p = gp.get();
        let mut start = [0.0; 2];
        let mut current = [0.0; 2];
        unsafe {
            for command in &path.commands {
                match *command {
                    Command::Move(point) => {
                        GdipStartPathFigure(p);
                        start = transform.point(point);
                        current = start;
                    }
                    Command::Line(point) => {
                        let point = transform.point(point);
                        GdipAddPathLine(p, current[0], current[1], point[0], point[1]);
                        current = point;
                    }
                    Command::Cubic(a, b, end) => {
                        let [a, b, end] = [a, b, end].map(|point| transform.point(point));
                        GdipAddPathBezier(
                            p, current[0], current[1], a[0], a[1], b[0], b[1], end[0], end[1],
                        );
                        current = end;
                    }
                    Command::Quadratic(control, end) => {
                        let [control, end] = [control, end].map(|point| transform.point(point));
                        // The cubic Bezier curve of a quadratic one.
                        let a = [
                            current[0] + (control[0] - current[0]) * 2.0 / 3.0,
                            current[1] + (control[1] - current[1]) * 2.0 / 3.0,
                        ];
                        let b = [
                            end[0] + (control[0] - end[0]) * 2.0 / 3.0,
                            end[1] + (control[1] - end[1]) * 2.0 / 3.0,
                        ];
                        GdipAddPathBezier(
                            p, current[0], current[1], a[0], a[1], b[0], b[1], end[0], end[1],
                        );
                        current = end;
                    }
                    Command::Close => {
                        GdipClosePathFigure(p);
                        current = start;
                    }
                }
            }
        }
        Some(gp)
    }

    pub(crate) fn fill(&mut self, state: &CanvasState, path: &Path, rule: FillRule) {
        // A lone ellipse in a solid color fills without building a path, like particles
        if let Some(([x, y], [rx, ry])) = path.lone_ellipse()
            && state.composite == CompositeOperation::SourceOver
            && let CanvasStyle::Color(color) = &state.fill
        {
            if let Some(brush) = Self::color_brush(*color, state.alpha) {
                self.set_world(self.user());
                unsafe {
                    GdipFillEllipse(
                        self.surface.g(),
                        brush.get(),
                        x - rx,
                        y - ry,
                        rx * 2.0,
                        ry * 2.0,
                    )
                };
            }
            return;
        }
        let Some(path) = self.gp_path(path, self.transform, rule) else {
            return;
        };
        self.fill_path(state, path);
    }

    // Fills a logical path.
    fn fill_path(&mut self, state: &CanvasState, path: Gp<GpPath>) {
        // Only gradients paint over the bounds
        let bounds = if matches!(state.fill, CanvasStyle::Gradient(_)) {
            self.path_bounds(&path, None)
        } else {
            [0; 4]
        };
        self.draw_with(
            state,
            |_| clone_path(&path),
            |this, state| {
                let world = this.device();
                let Some(brush) = this.brush(&state.fill, state.alpha, world, bounds) else {
                    return;
                };
                this.set_world(world);
                unsafe { GdipFillPath(this.surface.g(), brush.get(), path.get()) };
            },
        );
    }

    pub(crate) fn stroke(&mut self, state: &CanvasState, path: &Path) {
        let Some(path) = self.gp_path(path, Transform::IDENTITY, FillRule::NonZero) else {
            return;
        };
        self.stroke_path(state, path);
    }

    // Strokes a user-space path.
    fn stroke_path(&mut self, state: &CanvasState, path: Gp<GpPath>) {
        // Only gradients and composite operations need the outline of the stroke.
        let outline = (state.composite != CompositeOperation::SourceOver
            || matches!(state.stroke, CanvasStyle::Gradient(_)))
        .then(|| self.widened(&path, state))
        .flatten();
        let bounds = outline.as_ref().map_or(self.clip_bounds(), |outline| {
            self.path_bounds(outline, None)
        });
        self.draw_with(
            state,
            |_| outline.as_ref().and_then(clone_path),
            |this, state| {
                let world = this.user();
                let Some(brush) = this.brush(&state.stroke, state.alpha, world, bounds) else {
                    return;
                };
                let Some(pen) = this.pen(state, &brush) else {
                    return;
                };
                this.set_world(world);
                unsafe { GdipDrawPath(this.surface.g(), pen.get(), path.get()) };
            },
        );
    }

    // The logical outline of a stroke of a user-space path.
    fn widened(&self, path: &Gp<GpPath>, state: &CanvasState) -> Option<Gp<GpPath>> {
        let outline = clone_path(path)?;
        let brush = Self::color_brush(Color::rgb(0, 0, 0), 1.0)?;
        let pen = self.pen(state, &brush)?;
        let matrix = Self::new_matrix(self.transform)?;
        let status = unsafe { GdipWidenPath(outline.get(), pen.get(), matrix.get(), 0.25) };
        // A path without segments has no outline to widen.
        (status == GP_OK).then_some(outline)
    }

    // The device rectangle of a logical path, inflated for antialiasing and limited to the
    // area that drawing can change.
    fn path_bounds(&self, path: &Gp<GpPath>, extra: Option<f32>) -> Rect {
        let mut rect = RectF::default();
        let matrix = Self::new_matrix(self.device());
        let status = unsafe {
            GdipGetPathWorldBounds(
                path.get(),
                &mut rect,
                matrix.as_ref().map_or(null_mut(), Gp::get),
                null_mut(),
            )
        };
        let clip = self.clip_bounds();
        if status != GP_OK || !(rect.width.is_finite() && rect.height.is_finite()) {
            return clip;
        }
        let inflate = 2.0 + extra.unwrap_or(0.0);
        let limit = |value: f32| value.clamp(-1.0e8, 1.0e8) as i32;
        let left = limit((rect.x - inflate).floor());
        let top = limit((rect.y - inflate).floor());
        let right = limit((rect.x + rect.width + inflate).ceil());
        let bottom = limit((rect.y + rect.height + inflate).ceil());
        intersect([left, top, right - left, bottom - top], clip)
    }

    fn pen(&self, state: &CanvasState, brush: &Brush) -> Option<Gp<GpPen>> {
        let pen = unsafe {
            Gp::create(|out| GdipCreatePen2(brush.get(), state.line_width, UnitWorld, out))
        }?;
        let p = pen.get();
        let (cap, dash_cap) = match state.line_cap {
            LineCap::Butt => (LineCapFlat, DashCapFlat),
            LineCap::Square => (LineCapSquare, DashCapFlat),
            LineCap::Round => (LineCapRound, DashCapRound),
        };
        let join = match state.line_join {
            LineJoin::Miter => LineJoinMiterClipped,
            LineJoin::Bevel => LineJoinBevel,
            LineJoin::Round => LineJoinRound,
        };
        unsafe {
            GdipSetPenLineCap197819(p, cap, cap, dash_cap);
            GdipSetPenLineJoin(p, join);
            GdipSetPenMiterLimit(p, state.miter_limit);
            if !state.line_dash.is_empty() && state.line_width > 0.0 {
                // GDI+ dashes are positive and in units of the line width.
                let dashes: Vec<f32> = state
                    .line_dash
                    .iter()
                    .map(|length| (length / state.line_width).max(1.0e-3))
                    .collect();
                GdipSetPenDashArray(p, dashes.as_ptr(), dashes.len() as i32);
                GdipSetPenDashOffset(p, state.line_dash_offset / state.line_width);
            }
        }
        Some(pen)
    }

    pub(crate) fn draw_image(
        &mut self,
        state: &CanvasState,
        image: &PlatformImage,
        source: [f32; 4],
        [x, y, w, h]: [f32; 4],
    ) {
        let [sx, sy, sw, sh] = source;
        // A copy pixel for pixel looks the same smoothed or not and is much faster without
        let user = self.user();
        let [left, top] = user.point([x, y]);
        let exact = user.0[1] == 0.0
            && user.0[2] == 0.0
            && (user.0[0] * w - sw).abs() < 1e-3
            && (user.0[3] * h - sh).abs() < 1e-3
            && (left - left.round()).abs() < 1e-3
            && (top - top.round()).abs() < 1e-3;
        let smooth = state.image_smoothing && !exact;
        self.draw_with(
            state,
            |this| this.rect_path(x, y, w, h),
            |this, state| unsafe {
                // GDI+ only reads the shared pixels.
                let Some(bitmap) = Gp::create(|out| {
                    GdipCreateBitmapFromScan0(
                        image.width as i32,
                        image.height as i32,
                        image.width as i32 * 4,
                        PixelFormat32bppPARGB,
                        image.pixels.as_ptr().cast_mut(),
                        out,
                    )
                }) else {
                    return;
                };
                // Image attributes take GDI+'s slow path, an opaque copy without smoothing needs none
                let attributes = if state.alpha < 1.0 || smooth {
                    let Some(attributes) = Gp::create(|out| GdipCreateImageAttributes(out)) else {
                        return;
                    };
                    // Mirror the edge pixels, so scaled edges do not fade.
                    GdipSetImageAttributesWrapMode(attributes.get(), WrapModeTileFlipXY, 0, FALSE);
                    Some(attributes)
                } else {
                    None
                };
                if let Some(attributes) = &attributes
                    && state.alpha < 1.0
                {
                    let mut matrix = ColorMatrix { m: [[0.0; 5]; 5] };
                    for (i, row) in matrix.m.iter_mut().enumerate() {
                        row[i] = 1.0;
                    }
                    matrix.m[3][3] = state.alpha;
                    GdipSetImageAttributesColorMatrix(
                        attributes.get(),
                        ColorAdjustTypeDefault,
                        TRUE,
                        &matrix,
                        null(),
                        ColorMatrixFlagsDefault,
                    );
                }
                let g = this.surface.g();
                GdipSetInterpolationMode(
                    g,
                    if smooth {
                        InterpolationModeHighQualityBicubic
                    } else {
                        InterpolationModeNearestNeighbor
                    },
                );
                this.set_world(this.user());
                GdipDrawImageRectRect(
                    g,
                    bitmap.get(),
                    x,
                    y,
                    w,
                    h,
                    sx,
                    sy,
                    sw,
                    sh,
                    UnitPixel,
                    attributes.as_ref().map_or(null_mut(), Gp::get),
                    null(),
                    null(),
                );
            },
        );
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
    pub(crate) const fn set_transform(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.transform = Transform([a, b, c, d, e, f]);
    }
    pub(crate) const fn reset_transform(&mut self) {
        self.transform = Transform::IDENTITY;
    }
    fn concat(&mut self, transform: Transform) {
        self.transform = self.transform.concat(transform);
    }

    pub(crate) fn fill_text(&mut self, s: &CanvasState, text: &str, x: f32, y: f32) {
        let font = self.font(s);
        let length = self.encode(text);
        // Measuring is slow, solid text that starts at x only needs its width for a layer
        let gradient = matches!(s.fill, CanvasStyle::Gradient(_));
        let width = if gradient
            || s.composite != CompositeOperation::SourceOver
            || !matches!(s.text_align, TextAlign::Start | TextAlign::Left)
        {
            self.measure(&font, length)
        } else {
            0.0
        };
        let size = s.font_size;
        let extents = Self::extents(&font, size, width);
        let line_height = font.line_height * size;
        // GDI+ draws from the top of the line.
        let [dx, dy] = extents.origin(s);
        let (origin_x, origin_y) = (x + dx, y + dy - extents.ascent);
        let bounds = if gradient {
            let Some(rect) = self.rect_path(origin_x, origin_y, width, line_height) else {
                return;
            };
            self.path_bounds(&rect, Some(size * self.scale))
        } else {
            [0; 4]
        };
        self.draw_with(
            s,
            |this| this.rect_path(origin_x, origin_y, width, line_height),
            |this, s| {
                let world = this.user();
                let Some(brush) = this.brush(&s.fill, s.alpha, world, bounds) else {
                    return;
                };
                this.set_world(world);
                let text = this.resources.text.borrow();
                let layout = RectF {
                    x: origin_x,
                    y: origin_y,
                    width: 0.0,
                    height: 0.0,
                };
                unsafe {
                    GdipDrawString(
                        this.surface.g(),
                        text.as_ptr(),
                        length,
                        font.font.get(),
                        &layout,
                        this.resources.format.get(),
                        brush.get(),
                    )
                };
            },
        );
    }

    pub(crate) fn measure_text(&mut self, s: &CanvasState, text: &str) -> TextExtents {
        let font = self.font(s);
        let length = self.encode(text);
        let mut extents = Self::extents(&font, s.font_size, self.measure(&font, length));
        if length > 0 {
            extents.ink = self
                .text_ink(&font, length, s.font_size, extents.ascent)
                .unwrap_or_default();
        }
        extents
    }

    fn text_ink(&self, font: &Font, length: i32, size: f32, ascent: f32) -> Option<[f32; 4]> {
        let text = self.resources.text.borrow();
        let layout = RectF::default();
        let mut bounds = RectF::default();
        unsafe {
            let family = Gp::create(|out| GdipGetFamily(font.font.get(), out))?;
            let path = Gp::create(|out| GdipCreatePath(FillModeWinding, out))?;
            // Measure the outline with the same font, origin and format as GdipDrawString.
            if GdipAddPathString(
                path.get(),
                text.as_ptr(),
                length,
                family.get(),
                font.style,
                size,
                &layout,
                self.resources.format.get(),
            ) != GP_OK
                || GdipGetPathWorldBounds(path.get(), &mut bounds, null_mut(), null_mut()) != GP_OK
            {
                return None;
            }
        }
        Some([
            bounds.x,
            bounds.y - ascent,
            bounds.x + bounds.width,
            bounds.y + bounds.height - ascent,
        ])
    }

    fn extents(font: &Font, size: f32, width: f32) -> TextExtents {
        let (ascent, line_height) = (font.ascent * size, font.line_height * size);
        let descent = line_height - ascent;
        TextExtents {
            width,
            ink: [0.0; 4],
            ascent,
            descent,
        }
    }

    // Encodes text into the shared buffer, canvas line breaks render as spaces.
    fn encode(&self, text: &str) -> i32 {
        let mut wide = self.resources.text.borrow_mut();
        wide.clear();
        wide.extend(text.encode_utf16().map(|unit| match unit {
            0x09..=0x0d | 0x85 | 0x2028 | 0x2029 => 0x20,
            unit => unit,
        }));
        wide.len() as i32
    }

    // The width of the encoded text in logical pixels.
    fn measure(&self, font: &Font, length: i32) -> f32 {
        if length == 0 {
            return 0.0;
        }
        let text = self.resources.text.borrow();
        let layout = RectF {
            width: 1.0e6,
            height: 1.0e6,
            ..Default::default()
        };
        let mut bounds = RectF::default();
        self.set_world(Transform::IDENTITY);
        unsafe {
            GdipMeasureString(
                self.surface.g(),
                text.as_ptr(),
                length,
                font.font.get(),
                &layout,
                self.resources.format.get(),
                &mut bounds,
                null_mut(),
                null_mut(),
            )
        };
        bounds.width
    }

    fn font(&self, s: &CanvasState) -> Rc<Font> {
        let size = s.font_size.to_bits();
        let mut fonts = self.resources.fonts.borrow_mut();
        if let Some((_, font)) = fonts.iter().find(|((family, key_size, weight), _)| {
            *family == s.font_family && *key_size == size && *weight == s.font_weight
        }) {
            return font.clone();
        }
        let style = match s.font_weight {
            FontWeight::Normal => FontStyleRegular,
            FontWeight::Bold => FontStyleBold,
        };
        let font = Rc::new(new_font(&s.font_family, s.font_size, style));
        if fonts.len() == FONTS {
            fonts.remove(0);
        }
        fonts.push(((s.font_family.clone(), size, s.font_weight), font.clone()));
        font
    }

    // A color with the global alpha as GDI+ ARGB, which is premultiplied when opaque.
    fn argb(color: Color, alpha: f32) -> u32 {
        let (r, g, b, a) = color.components(alpha);
        let byte = |value: f64| u32::from((value.clamp(0.0, 1.0) * 255.0).round() as u8);
        (byte(a) << 24) | (byte(r) << 16) | (byte(g) << 8) | byte(b)
    }

    fn color_brush(color: Color, alpha: f32) -> Option<Brush> {
        let argb = Self::argb(color, alpha);
        let brush = unsafe { Gp::create(|out| GdipCreateSolidFill(argb, out)) }?;
        Some(Brush {
            brush,
            _bitmap: None,
            _pixels: Vec::new(),
        })
    }

    // A brush for a style drawn with the world transform `world`, painting gradients over
    // the device rectangle `bounds`.
    fn brush(
        &self,
        style: &CanvasStyle,
        alpha: f32,
        world: Transform,
        bounds: Rect,
    ) -> Option<Brush> {
        let gradient = match style {
            CanvasStyle::Color(color) => return Self::color_brush(*color, alpha),
            CanvasStyle::Gradient(gradient) => &gradient.0,
        };
        let transparent = || Self::color_brush(Color::rgba(0, 0, 0, 0), 1.0);
        let [left, top, width, height] = bounds;
        if width == 0 || height == 0 {
            return transparent();
        }
        let to_device = self.user().concat(Transform(gradient.transform));
        let Some(mut pixels) = gradient::paint(gradient, alpha, to_device, bounds) else {
            return transparent();
        };
        unsafe {
            let bitmap = Gp::create(|out| {
                GdipCreateBitmapFromScan0(
                    width,
                    height,
                    width * 4,
                    PixelFormat32bppPARGB,
                    pixels.as_mut_ptr(),
                    out,
                )
            })?;
            let brush = Gp::create(|out| GdipCreateTexture(bitmap.get(), WrapModeClamp, out))?;
            // The texture maps its pixels onto the device rectangle.
            let texture =
                world
                    .inverse()?
                    .concat(Transform([1.0, 0.0, 0.0, 1.0, left as f32, top as f32]));
            let matrix = Self::new_matrix(texture)?;
            GdipSetTextureTransform(brush.get(), matrix.get());
            Some(Brush {
                brush,
                _bitmap: Some(bitmap),
                _pixels: pixels,
            })
        }
    }
}

/// A GDI+ brush with the texture pixels it may read while it is alive.
struct Brush {
    brush: Gp<GpBrush>,
    _bitmap: Option<Gp<GpImage>>,
    _pixels: Vec<u8>,
}

impl Brush {
    const fn get(&self) -> *mut GpBrush {
        self.brush.get()
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // Composite layers left open, then restore the clip of the surface.
        while !self.layers.is_empty() {
            self.end_layer();
        }
        self.pop_clips(0);
    }
}

fn clone_path(path: &Gp<GpPath>) -> Option<Gp<GpPath>> {
    unsafe { Gp::create(|out| GdipClonePath(path.get(), out)) }
}

fn normalized(x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    (x.min(x + w), y.min(y + h), w.abs(), h.abs())
}

// A font of a canvas family, with the system fonts of the generic families.
fn new_font(family: &str, size: f32, style: i32) -> Font {
    let candidates: &[&[u16]] = match family {
        "sans-serif" => &[w!("Segoe UI"), w!("Tahoma"), w!("Arial")],
        "serif" => &[w!("Times New Roman")],
        "monospace" => &[w!("Consolas"), w!("Courier New")],
        _ => &[],
    };
    let custom = wide(family);
    let named = candidates
        .iter()
        .copied()
        .chain((candidates.is_empty()).then_some(custom.as_slice()))
        .find_map(|name| unsafe {
            Gp::create(|out| GdipCreateFontFamilyFromName(name.as_ptr(), null_mut(), out))
        });
    unsafe {
        // The generic family is owned by GDI+ and never deleted.
        let mut generic = null_mut();
        GdipGetGenericFontFamilySansSerif(&mut generic);
        // GDI+ fails to create a style that a family lacks, like bold of a family with only
        // a regular face, so fall back to the regular style and then the generic family.
        let families = named.as_ref().map(Gp::get).into_iter().chain([generic]);
        for family_ptr in families.filter(|family| !family.is_null()) {
            for style in [style, FontStyleRegular, FontStyleBold] {
                let Some(font) =
                    Gp::create(|out| GdipCreateFont(family_ptr, size, style, UnitPixel, out))
                else {
                    continue;
                };
                let metric =
                    |get: unsafe extern "system" fn(*mut GpFontFamily, i32, *mut u16) -> i32| {
                        let mut value = 0u16;
                        get(family_ptr, style, &mut value);
                        f32::from(value)
                    };
                let em = metric(GdipGetEmHeight).max(1.0);
                return Font {
                    font,
                    style,
                    ascent: metric(GdipGetCellAscent) / em,
                    line_height: metric(GdipGetLineSpacing) / em,
                };
            }
        }
        panic!("GDI+ font");
    }
}
