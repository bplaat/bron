/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The platform independent part of the media view: the magnification, scroll position, drag
//! and animation of the media inside the visible area, and drawing it above a checkerboard. The
//! platform views forward their input here and show the scroll bars it describes.

use std::time::{Duration, Instant};

use bcanvas::{CanvasRenderingContext2d, Color, FillRule};
use imageview_core::edit::{Selection, Shape};
use imageview_core::{
    LoopCount, Media, MediaRenderer, Raster, clamp_magnification, draw_checkerboard,
    fit_magnification,
};

use crate::marquee::Marquee;

/// How far a scroll bar arrow, arrow key or scroll wheel line scrolls, in logical pixels.
pub(crate) const LINE: f64 = 48.0;
// How close to an edge of a crop or selection a drag resizes it, in logical pixels.
const CROP_REACH: f64 = 10.0;
// The length and thickness of the corner handles of the crop rectangle.
const HANDLE_LENGTH: f32 = 18.0;
const HANDLE_WIDTH: f32 = 3.0;

/// One scroll axis of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    pub(crate) const BOTH: [Self; 2] = [Self::Horizontal, Self::Vertical];

    // Picks the component of a point or size along this axis.
    const fn of(self, (x, y): (f64, f64)) -> f64 {
        match self {
            Self::Horizontal => x,
            Self::Vertical => y,
        }
    }

    // Replaces the component of a point along this axis.
    const fn with(self, (x, y): (f64, f64), value: f64) -> (f64, f64) {
        match self {
            Self::Horizontal => (value, y),
            Self::Vertical => (x, value),
        }
    }
}

/// A scroll step of a scroll bar or key.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Step {
    LineBack,
    LineForward,
    PageBack,
    PageForward,
    Start,
    End,
}

/// What a scroll bar shows, in logical pixels: the scroll position, the visible length and the
/// length of the magnified media, at least the visible length.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScrollBar {
    pub(crate) position: f64,
    pub(crate) page: f64,
    pub(crate) length: f64,
}

/// The media inside the visible area of a view, in logical pixels.
pub(crate) struct Viewport {
    renderer: MediaRenderer,
    visible: (f64, f64),
    magnification: f64,
    scroll: (f64, f64),
    // The pointer position at the previous step of a drag.
    drag: Option<(f64, f64)>,
    // The shown animation frame and when the animation started.
    frame: usize,
    start: Instant,
    // A smaller preview of an edit, drawn over the media in its place.
    preview: Option<MediaRenderer>,
    crop: Option<Marquee>,
    selection: Option<(Shape, Marquee)>,
    inverted: bool,
    // The shape a drag outside the selection draws.
    selecting: Option<Shape>,
}

impl Viewport {
    /// Shows the media zoomed to fit the visible area, or nothing while it is loading.
    pub(crate) fn new(media: Option<Media>, visible: (f64, f64)) -> Self {
        let media = media.unwrap_or(Media::Raster(Raster {
            width: 0,
            height: 0,
            frames: Vec::new(),
            loop_count: LoopCount::Finite(1),
        }));
        let mut viewport = Self {
            renderer: MediaRenderer::new(media),
            visible,
            magnification: 1.0,
            scroll: (0.0, 0.0),
            drag: None,
            frame: 0,
            start: Instant::now(),
            preview: None,
            crop: None,
            selection: None,
            inverted: false,
            selecting: None,
        };
        viewport.zoom_to_fit();
        viewport
    }

    pub(crate) const fn media(&self) -> &Media {
        self.renderer.media()
    }

    /// Replaces the media, keeping the zoom or fitting it, and restarts the animation.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        let resized = media.size() != self.media().size();
        self.renderer = MediaRenderer::new(media);
        self.preview = None;
        self.frame = 0;
        self.start = Instant::now();
        if self.crop.is_some() {
            self.crop = Some(Marquee::covering(self.media().size()));
        }
        // A selection only stays on a picture that changes its colors.
        if resized || zoom_to_fit {
            self.selection = None;
        }
        if zoom_to_fit {
            self.zoom_to_fit();
        } else {
            self.scroll_to(self.scroll);
        }
    }

    /// Follows a new size of the visible area, keeping the media in its middle in place, so
    /// it does not shift when scroll bars appear.
    pub(crate) fn resize(&mut self, visible: (f64, f64)) {
        let scroll = (
            self.scroll.0 + (self.visible.0 - visible.0) / 2.0,
            self.scroll.1 + (self.visible.1 - visible.1) / 2.0,
        );
        self.visible = visible;
        self.scroll_to(scroll);
    }

    /// The magnified media size.
    pub(crate) fn content(&self) -> (f64, f64) {
        let (width, height) = self.media().size();
        (width * self.magnification, height * self.magnification)
    }

    // Where the media starts in the visible area, it is centered when smaller.
    fn offset(&self, axis: Axis) -> f64 {
        ((axis.of(self.visible) - axis.of(self.content())) / 2.0).max(0.0)
    }

    /// Scrolls to a position kept inside the media.
    pub(crate) fn scroll_to(&mut self, (x, y): (f64, f64)) {
        let clamp = |axis: Axis, value: f64| {
            value.clamp(
                0.0,
                (axis.of(self.content()) - axis.of(self.visible)).max(0.0),
            )
        };
        self.scroll = (clamp(Axis::Horizontal, x), clamp(Axis::Vertical, y));
    }

    pub(crate) fn scroll_by(&mut self, (x, y): (f64, f64)) {
        self.scroll_to((self.scroll.0 + x, self.scroll.1 + y));
    }

    /// Scrolls one axis to a position, like a dragged scroll bar.
    pub(crate) fn scroll_axis(&mut self, axis: Axis, position: f64) {
        self.scroll_to(axis.with(self.scroll, position));
    }

    /// Scrolls one axis by a step.
    pub(crate) fn step(&mut self, axis: Axis, step: Step) {
        let (current, page) = (axis.of(self.scroll), axis.of(self.visible));
        self.scroll_axis(
            axis,
            match step {
                Step::LineBack => current - LINE,
                Step::LineForward => current + LINE,
                Step::PageBack => current - page,
                Step::PageForward => current + page,
                Step::Start => 0.0,
                Step::End => f64::INFINITY,
            },
        );
    }

    /// Magnifies the media, keeping the media under `anchor` in the visible area in place, or
    /// the middle of the visible area.
    pub(crate) fn magnify(&mut self, magnification: f64, anchor: Option<(f64, f64)>) {
        let anchor = anchor.unwrap_or((self.visible.0 / 2.0, self.visible.1 / 2.0));
        let media = Axis::BOTH.map(|axis| {
            (axis.of(self.scroll) + axis.of(anchor) - self.offset(axis)) / self.magnification
        });
        self.magnification = clamp_magnification(magnification);
        self.scroll_to((
            media[0] * self.magnification + self.offset(Axis::Horizontal) - anchor.0,
            media[1] * self.magnification + self.offset(Axis::Vertical) - anchor.1,
        ));
    }

    /// Magnifies by `factor` around `anchor`, or the middle of the visible area.
    pub(crate) fn zoom_by(&mut self, factor: f64, anchor: Option<(f64, f64)>) {
        self.magnify(self.magnification * factor, anchor);
    }

    /// Shows the media at its natural size, scrolled to its middle.
    pub(crate) fn actual_size(&mut self) {
        self.zoom_centered(1.0);
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&mut self) {
        self.zoom_centered(fit_magnification(self.visible, self.media().size()));
    }

    fn zoom_centered(&mut self, magnification: f64) {
        self.magnification = clamp_magnification(magnification);
        let (content, visible) = (self.content(), self.visible);
        self.scroll_to(((content.0 - visible.0) / 2.0, (content.1 - visible.1) / 2.0));
    }

    /// Whether the media is larger than the visible area, so it can be dragged around.
    pub(crate) fn is_scrollable(&self) -> bool {
        Axis::BOTH.into_iter().any(|axis| self.overflows(axis))
    }

    // Whether the magnified media is longer than the visible area along an axis, ignoring
    // sub-pixel overflow from rounding, which would show scroll bars for fitted media.
    fn overflows(&self, axis: Axis) -> bool {
        axis.of(self.content()) - axis.of(self.visible) > 0.5
    }

    /// Takes hold of the media at `pointer`, returns whether it can be dragged.
    pub(crate) fn drag_begin(&mut self, pointer: (f64, f64)) -> bool {
        self.drag = self.is_scrollable().then_some(pointer);
        self.drag.is_some()
    }

    /// Moves held media along with the pointer, returns whether it is held.
    pub(crate) fn drag_to(&mut self, pointer: (f64, f64)) -> bool {
        let Some(previous) = self.drag.replace(pointer) else {
            return false;
        };
        self.scroll_by((previous.0 - pointer.0, previous.1 - pointer.1));
        true
    }

    /// Lets go of the media, returns whether it was held.
    pub(crate) const fn drag_end(&mut self) -> bool {
        self.drag.take().is_some()
    }

    /// Shows a preview of any size in the place of the media, or the media again.
    pub(crate) fn set_preview(&mut self, preview: Option<Media>) {
        self.preview = preview.map(MediaRenderer::new);
    }

    /// Shows a crop rectangle over all of the media, or removes it.
    pub(crate) fn set_cropping(&mut self, cropping: bool) {
        self.crop = cropping.then(|| Marquee::covering(self.media().size()));
    }

    /// The crop rectangle, while cropping.
    pub(crate) const fn crop(&mut self) -> Option<&mut Marquee> {
        self.crop.as_mut()
    }

    /// Makes drags draw a selection of a shape, which the selection takes too, or selects
    /// all of the media again, where drags pan it.
    pub(crate) const fn set_selecting(&mut self, shape: Option<Shape>) {
        self.selecting = shape;
        match shape {
            Some(shape) => {
                if let Some((selected, _)) = &mut self.selection {
                    *selected = shape;
                }
            }
            None => {
                self.selection = None;
                self.inverted = false;
            }
        }
    }

    /// The shape drags draw a selection of.
    pub(crate) const fn selecting(&self) -> Option<Shape> {
        self.selecting
    }

    /// Whether drags draw a crop or selection, instead of panning.
    pub(crate) const fn is_marking(&self) -> bool {
        self.crop.is_some() || self.selecting.is_some()
    }

    /// The selected part of the media.
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.selection.as_ref().map(|(shape, marquee)| Selection {
            shape: *shape,
            edges: marquee.edges(),
            inverted: self.inverted,
        })
    }

    /// Selects everything outside the selection instead.
    pub(crate) const fn invert_selection(&mut self) {
        if self.selection.is_some() {
            self.inverted = !self.inverted;
        }
    }

    // The media position under a position in the visible area.
    fn media_point(&self, pointer: (f64, f64)) -> (f64, f64) {
        let [x, y] = Axis::BOTH.map(|axis| {
            (axis.of(self.scroll) + axis.of(pointer) - self.offset(axis)) / self.magnification
        });
        (x, y)
    }

    /// Takes hold of the crop rectangle, the selection or a new selection at `pointer`,
    /// returns whether it is one of them instead of the media.
    pub(crate) fn mark_begin(&mut self, pointer: (f64, f64)) -> bool {
        let (point, reach) = (self.media_point(pointer), CROP_REACH / self.magnification);
        if let Some(crop) = &mut self.crop {
            crop.begin(point, reach);
            return true;
        }
        let held = self
            .selection
            .as_ref()
            .is_some_and(|(_, marquee)| marquee.contains(point, reach));
        if !held {
            let Some(shape) = self.selecting else {
                return false;
            };
            self.inverted = false;
            self.selection = Some((shape, Marquee::empty(self.media().size())));
        }
        if let Some((_, marquee)) = &mut self.selection {
            marquee.begin(point, reach);
        }
        true
    }

    /// Moves the held rectangle along, as a square when asked, returns whether one is held.
    pub(crate) fn mark_drag(&mut self, pointer: (f64, f64), square: bool) -> bool {
        let point = self.media_point(pointer);
        let crop = self
            .crop
            .as_mut()
            .is_some_and(|crop| crop.drag(point, square));
        crop || self
            .selection
            .as_mut()
            .is_some_and(|(_, marquee)| marquee.drag(point, square))
    }

    /// Lets go of the held rectangle, a selection without a size is removed. Returns whether
    /// one was held.
    pub(crate) fn mark_end(&mut self) -> bool {
        if self.crop.as_mut().is_some_and(Marquee::end) {
            return true;
        }
        let ended = self
            .selection
            .as_mut()
            .is_some_and(|(_, marquee)| marquee.end());
        if self
            .selection
            .as_ref()
            .is_some_and(|(_, marquee)| marquee.is_empty())
        {
            self.selection = None;
        }
        ended
    }

    /// What the scroll bar of an axis shows.
    pub(crate) fn scroll_bar(&self, axis: Axis) -> ScrollBar {
        let page = axis.of(self.visible);
        ScrollBar {
            position: axis.of(self.scroll),
            page,
            length: if self.overflows(axis) {
                axis.of(self.content())
            } else {
                page
            },
        }
    }

    /// Moves to the current animation frame, returns whether it changed and how long until
    /// the next one.
    pub(crate) fn advance_frame(&mut self) -> (bool, Option<Duration>) {
        let (frame, next) = self.media().frame_at(self.start.elapsed());
        (std::mem::replace(&mut self.frame, frame) != frame, next)
    }

    /// Draws the checkerboard filling the visible area and the scrolled media above it.
    #[cfg(target_os = "macos")]
    pub(crate) fn paint(&self, context: &mut CanvasRenderingContext2d<'_>, dark: bool) {
        self.draw(context, dark, self.scroll);
    }

    /// Draws the checkerboard and media into a native scrolled document as large as the scroll bar
    /// lengths, which scrolls itself.
    #[cfg(not(any(target_os = "macos", windows)))]
    pub(crate) fn paint_document(&self, context: &mut CanvasRenderingContext2d<'_>, dark: bool) {
        self.draw(context, dark, (0.0, 0.0));
    }

    fn draw(&self, context: &mut CanvasRenderingContext2d<'_>, dark: bool, scroll: (f64, f64)) {
        draw_checkerboard(context, context.width(), context.height(), dark);
        let (width, height) = self.content();
        let origin = (
            self.offset(Axis::Horizontal) - scroll.0,
            self.offset(Axis::Vertical) - scroll.1,
        );
        // A preview is a still image, whichever frame the media is at.
        let (renderer, frame) = self
            .preview
            .as_ref()
            .map_or((&self.renderer, self.frame), |preview| (preview, 0));
        renderer.draw(
            context,
            frame,
            (
                origin.0 as f32,
                origin.1 as f32,
                width as f32,
                height as f32,
            ),
        );
        self.draw_marks(context, origin);
    }

    /// Draws the crop rectangle or selection over the visible area, for a platform that draws the
    /// media itself.
    #[cfg(windows)]
    pub(crate) fn paint_marks(&self, context: &mut CanvasRenderingContext2d<'_>) {
        let origin = Axis::BOTH.map(|axis| self.offset(axis) - axis.of(self.scroll));
        self.draw_marks(context, (origin[0], origin[1]));
    }

    /// What a platform that draws the media itself shows: the preview or the media, with its
    /// frame.
    #[cfg(windows)]
    pub(crate) fn drawn(&self) -> (&MediaRenderer, usize) {
        // A preview is a still image, whichever frame the media is at.
        self.preview
            .as_ref()
            .map_or((&self.renderer, self.frame), |preview| (preview, 0))
    }

    /// Whether the shown raster needs sharp pixels at this display scale.
    #[cfg(windows)]
    pub(crate) fn has_pixels(&self, scale: f32) -> bool {
        let (renderer, _) = self.drawn();
        let Media::Raster(raster) = renderer.media() else {
            return false;
        };
        self.content().0 * f64::from(scale) >= 2.0 * f64::from(raster.width)
    }

    /// Draws enlarged raster pixels into the visible area, bypassing XAML image scaling.
    #[cfg(windows)]
    pub(crate) fn paint_pixels(&self, context: &mut CanvasRenderingContext2d<'_>) {
        let (renderer, frame) = self.drawn();
        let (width, height) = self.content();
        let origin = Axis::BOTH.map(|axis| self.offset(axis) - axis.of(self.scroll));
        renderer.draw(
            context,
            frame,
            (
                origin[0] as f32,
                origin[1] as f32,
                width as f32,
                height as f32,
            ),
        );
    }

    /// Whether there is a crop rectangle or selection to draw.
    #[cfg(windows)]
    pub(crate) const fn has_marks(&self) -> bool {
        self.crop.is_some() || self.selection.is_some()
    }

    /// Follows a platform view that zooms and scrolls by itself, like the WinUI scroll viewer:
    /// its visible area, magnification and scroll position.
    #[cfg(windows)]
    pub(crate) const fn follow(
        &mut self,
        visible: (f64, f64),
        magnification: f64,
        scroll: (f64, f64),
    ) {
        self.visible = visible;
        self.magnification = magnification;
        self.scroll = scroll;
    }

    // Draws the crop rectangle or selection over media drawn at an origin.
    fn draw_marks(&self, context: &mut CanvasRenderingContext2d<'_>, origin: (f64, f64)) {
        let (width, height) = self.content();
        let (x, y) = (origin.0 as f32, origin.1 as f32);
        let place = |marquee: &Marquee| {
            let [left, top, right, bottom] = marquee
                .edges()
                .map(|edge| (edge * self.magnification) as f32);
            [x + left, y + top, x + right, y + bottom]
        };
        if let Some((shape, marquee)) = &self.selection
            && self.crop.is_none()
        {
            draw_selection(
                context,
                *shape,
                place(marquee),
                [x, y, x + width as f32, y + height as f32],
                self.inverted,
            );
        }
        if let Some(crop) = &self.crop {
            draw_crop(context, place(crop));
        }
    }
}

// Adds a shape to the path.
fn shape_path(
    context: &mut CanvasRenderingContext2d<'_>,
    shape: Shape,
    [left, top, right, bottom]: [f32; 4],
) {
    match shape {
        Shape::Rectangle => context.rect(left, top, right - left, bottom - top),
        Shape::Ellipse => context.ellipse(
            f32::midpoint(left, right),
            f32::midpoint(top, bottom),
            (right - left) / 2.0,
            (bottom - top) / 2.0,
        ),
    }
}

// Tints what is not selected and draws the outline of the selection, dashed black on white so
// it shows on any colors, with handles at its corners.
fn draw_selection(
    context: &mut CanvasRenderingContext2d<'_>,
    shape: Shape,
    edges: [f32; 4],
    media: [f32; 4],
    inverted: bool,
) {
    context.begin_path();
    if !inverted {
        context.rect(media[0], media[1], media[2] - media[0], media[3] - media[1]);
    }
    shape_path(context, shape, edges);
    context.set_fill_style(Color::rgba(0, 0, 0, 70));
    context.fill_with(FillRule::EvenOdd);

    context.begin_path();
    shape_path(context, shape, edges.map(|edge| edge + 0.5));
    context.set_line_width(1.0);
    context.set_stroke_style(Color::rgb(255, 255, 255));
    context.set_line_dash(&[]);
    context.stroke();
    context.set_stroke_style(Color::rgb(0, 0, 0));
    context.set_line_dash(&[4.0, 4.0]);
    context.stroke();
    context.set_line_dash(&[]);

    let [left, top, right, bottom] = edges;
    let size = HANDLE_WIDTH * 2.0;
    for (x, y) in [(left, top), (right, top), (left, bottom), (right, bottom)] {
        context.set_fill_style(Color::rgb(0, 0, 0));
        context.fill_rect(
            x - size / 2.0 - 1.0,
            y - size / 2.0 - 1.0,
            size + 2.0,
            size + 2.0,
        );
        context.set_fill_style(Color::rgb(255, 255, 255));
        context.fill_rect(x - size / 2.0, y - size / 2.0, size, size);
    }
}

// Dims the media outside the crop rectangle and draws its border, thirds and corner handles.
fn draw_crop(context: &mut CanvasRenderingContext2d<'_>, [left, top, right, bottom]: [f32; 4]) {
    let (width, height) = (context.width(), context.height());
    context.set_fill_style(Color::rgba(0, 0, 0, 140));
    context.fill_rect(0.0, 0.0, width, top);
    context.fill_rect(0.0, bottom, width, height - bottom);
    context.fill_rect(0.0, top, left, bottom - top);
    context.fill_rect(right, top, width - right, bottom - top);

    let (crop_width, crop_height) = (right - left, bottom - top);
    context.set_fill_style(Color::rgba(255, 255, 255, 110));
    for third in [1.0, 2.0] {
        context.fill_rect(left + crop_width * third / 3.0, top, 1.0, crop_height);
        context.fill_rect(left, top + crop_height * third / 3.0, crop_width, 1.0);
    }
    context.set_stroke_style(Color::rgb(255, 255, 255));
    context.set_line_width(1.0);
    context.stroke_rect(left + 0.5, top + 0.5, crop_width - 1.0, crop_height - 1.0);

    let length = HANDLE_LENGTH.min(crop_width / 2.0).min(crop_height / 2.0);
    context.set_fill_style(Color::rgb(255, 255, 255));
    for (x, y, horizontal, vertical) in [
        (left, top, 1.0, 1.0),
        (right, top, -1.0, 1.0),
        (left, bottom, 1.0, -1.0),
        (right, bottom, -1.0, -1.0),
    ] {
        // Each corner is an L outside the border, pointing into the rectangle.
        let (outer_x, outer_y) = (x - HANDLE_WIDTH * horizontal, y - HANDLE_WIDTH * vertical);
        let rect = |x: f32, y: f32, width: f32, height: f32| {
            (
                x.min(x + width),
                y.min(y + height),
                width.abs(),
                height.abs(),
            )
        };
        for (x, y, width, height) in [
            rect(
                outer_x,
                outer_y,
                (length + HANDLE_WIDTH) * horizontal,
                HANDLE_WIDTH * vertical,
            ),
            rect(
                outer_x,
                outer_y,
                HANDLE_WIDTH * horizontal,
                (length + HANDLE_WIDTH) * vertical,
            ),
        ] {
            context.fill_rect(x, y, width, height);
        }
    }
}

#[cfg(test)]
mod crop_tests {
    use super::*;

    #[test]
    fn crop_drags_follow_the_magnification() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/ball.gif"))
            .expect("read example");
        let media = imageview_core::decode_media(&bytes).expect("decode example");
        // The 100x100 media is shown at 3x, 50 pixels from the left.
        let mut viewport = Viewport::new(Some(media), (400.0, 300.0));
        assert!(!viewport.mark_begin((0.0, 0.0)));
        viewport.set_cropping(true);
        assert!(viewport.mark_begin((350.0, 150.0)));
        assert!(viewport.mark_drag((200.0, 150.0), false));
        assert!(viewport.mark_end());
        assert_eq!(
            viewport.crop().map(|crop| crop.rect()),
            Some((0, 0, 50, 100))
        );
    }
}

#[cfg(test)]
mod tests {
    use imageview_core::decode_media;

    use super::*;

    #[cfg(windows)]
    #[test]
    fn display_pixels_stay_sharp_when_zoomed_and_scrolled() {
        use bcanvas::OffscreenCanvas;
        use imageview_core::Frame;
        use std::sync::Arc;

        let media = Media::Raster(Raster {
            width: 2,
            height: 1,
            frames: vec![Frame {
                pixels: Arc::from([0, 0, 0, 255, 255, 255, 255, 255]),
                delay: Duration::ZERO,
            }],
            loop_count: LoopCount::Finite(1),
        });
        let mut viewport = Viewport::new(Some(media), (4.0, 2.0));
        viewport.follow((4.0, 2.0), 4.0, (2.0, 0.0));
        assert!(viewport.has_pixels(1.0));
        let mut canvas = OffscreenCanvas::new(4, 2);
        canvas.draw(|context| viewport.paint_pixels(context));
        for row in canvas.pixels().chunks(4 * 4) {
            assert_eq!(
                row.chunks(4).map(|pixel| pixel[0]).collect::<Vec<_>>(),
                [0, 0, 255, 255]
            );
        }
        for scale in [1.25, 1.5, 1.75, 2.625] {
            let mut canvas = OffscreenCanvas::with_scale_factor(4, 2, scale);
            canvas.draw(|context| viewport.paint_pixels(context));
            assert!(
                canvas.pixels().chunks_exact(4).all(|pixel| {
                    (pixel[0] == 0 || pixel[0] == 255)
                        && pixel[0] == pixel[1]
                        && pixel[1] == pixel[2]
                        && pixel[3] == 255
                }),
                "pixels must stay sharp at display scale {scale}"
            );
        }
        viewport.follow((4.0, 2.0), 1.0, (0.0, 0.0));
        assert!(!viewport.has_pixels(1.0));
        assert!(viewport.has_pixels(2.0));
        let vector =
            decode_media(br#"<svg xmlns="http://www.w3.org/2000/svg" width="2" height="1"/>"#)
                .unwrap();
        viewport.set_media(vector, false);
        assert!(!viewport.has_pixels(2.0));
    }

    // An animation of 100x100 pixels.
    fn media() -> Media {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/ball.gif"))
            .expect("read example");
        decode_media(&bytes).expect("decode example")
    }

    #[test]
    fn selections_are_drawn_moved_and_switched() {
        // The 100x100 media is shown at 3x, 50 pixels from the left.
        let mut viewport = Viewport::new(Some(media()), (400.0, 300.0));
        assert!(!viewport.mark_begin((80.0, 30.0)));
        viewport.set_selecting(Some(Shape::Ellipse));
        assert!(viewport.mark_begin((80.0, 30.0)));
        viewport.mark_drag((140.0, 60.0), true);
        viewport.mark_end();
        let selection = viewport.selection().expect("selection");
        assert_eq!(selection.shape, Shape::Ellipse);
        assert_eq!(selection.edges, [10.0, 10.0, 30.0, 30.0]);
        // Dragging inside moves it.
        assert!(viewport.mark_begin((110.0, 60.0)));
        viewport.mark_drag((140.0, 60.0), false);
        viewport.mark_end();
        assert_eq!(
            viewport.selection().map(|selection| selection.edges),
            Some([20.0, 10.0, 40.0, 30.0])
        );
        viewport.invert_selection();
        assert!(
            viewport
                .selection()
                .is_some_and(|selection| selection.inverted)
        );
        // Another shape keeps the rectangle.
        viewport.set_selecting(Some(Shape::Rectangle));
        assert_eq!(
            viewport.selection().map(|selection| selection.shape),
            Some(Shape::Rectangle)
        );
        // A click outside leaves no selection.
        assert!(viewport.mark_begin((300.0, 250.0)));
        assert!(viewport.mark_end());
        assert!(viewport.selection().is_none());
        // The whole image again.
        viewport.mark_begin((80.0, 30.0));
        viewport.mark_drag((140.0, 60.0), false);
        viewport.mark_end();
        viewport.set_selecting(None);
        assert!(viewport.selection().is_none() && viewport.selecting().is_none());
        // Another size has no selection.
        viewport.set_selecting(Some(Shape::Rectangle));
        viewport.mark_begin((80.0, 30.0));
        viewport.mark_drag((140.0, 60.0), false);
        viewport.mark_end();
        assert!(viewport.selection().is_some());
        viewport.set_media(media(), true);
        assert!(viewport.selection().is_none());
    }

    #[test]
    fn fitting_centers_the_media() {
        let viewport = Viewport::new(Some(media()), (400.0, 300.0));
        assert_eq!(viewport.magnification, 3.0);
        assert_eq!(viewport.offset(Axis::Horizontal), 50.0);
        assert_eq!(viewport.scroll, (0.0, 0.0));
        assert!(!viewport.is_scrollable());
    }

    #[test]
    fn fitted_media_has_no_scroll_bars() {
        // A fitted magnification that rounds a hair above the visible height.
        let mut viewport = Viewport::new(Some(media()), (100.0, 100.0));
        viewport.magnification = 1.000_000_1;
        assert!(!viewport.is_scrollable());
        assert_eq!(viewport.scroll_bar(Axis::Vertical).length, 100.0);
        viewport.magnification = 1.01;
        assert!(viewport.is_scrollable());
        assert_eq!(viewport.scroll_bar(Axis::Vertical).length, 101.0);
    }

    #[test]
    fn magnifying_keeps_the_anchor_in_place() {
        let mut viewport = Viewport::new(Some(media()), (100.0, 100.0));
        viewport.magnify(2.0, Some((25.0, 25.0)));
        assert_eq!(viewport.scroll, (25.0, 25.0));
        viewport.magnify(1.0, Some((25.0, 25.0)));
        assert_eq!(viewport.scroll, (0.0, 0.0));
    }

    #[test]
    fn scrolling_stays_inside_the_media() {
        let mut viewport = Viewport::new(Some(media()), (100.0, 100.0));
        viewport.zoom_centered(4.0);
        assert_eq!(viewport.scroll, (150.0, 150.0));
        viewport.scroll_by((1000.0, -1000.0));
        assert_eq!(viewport.scroll, (300.0, 0.0));
        viewport.step(Axis::Vertical, Step::End);
        assert_eq!(viewport.scroll, (300.0, 300.0));
        viewport.resize((400.0, 400.0));
        assert_eq!(viewport.scroll, (0.0, 0.0));
    }

    #[test]
    fn resizing_keeps_the_middle_in_place() {
        let mut viewport = Viewport::new(Some(media()), (100.0, 100.0));
        viewport.zoom_centered(4.0);
        assert_eq!(viewport.scroll, (150.0, 150.0));
        viewport.resize((80.0, 90.0));
        assert_eq!(viewport.scroll, (160.0, 155.0));
    }

    #[test]
    fn dragging_moves_the_media_with_the_pointer() {
        let mut viewport = Viewport::new(Some(media()), (100.0, 100.0));
        assert!(!viewport.drag_begin((50.0, 50.0)));
        viewport.zoom_centered(2.0);
        assert!(viewport.drag_begin((50.0, 50.0)));
        assert!(viewport.drag_to((40.0, 60.0)));
        assert_eq!(viewport.scroll, (60.0, 40.0));
        assert!(viewport.drag_end());
        assert_eq!(
            viewport.scroll_bar(Axis::Horizontal),
            ScrollBar {
                position: 60.0,
                page: 100.0,
                length: 200.0
            }
        );
    }
}
