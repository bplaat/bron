/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The platform independent part of the media view: the magnification, scroll position, drag
//! and animation of the media inside the visible area, and drawing it above a checkerboard. The
//! platform views forward their input here and show the scroll bars it describes.

use std::time::{Duration, Instant};

#[cfg(not(windows))]
use bcanvas::CanvasRenderingContext2d;
#[cfg(not(any(target_os = "macos", windows)))]
use imageview_core::draw_checkerboard_clipped;
use imageview_core::{
    LoopCount, Media, MediaRenderer, Raster, clamp_magnification, fit_magnification,
};

#[cfg(feature = "edit")]
mod marks;

/// How far a scroll bar arrow, arrow key or scroll wheel line scrolls, in logical pixels.
pub(crate) const LINE: f64 = 48.0;

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
    // The preview of an edit, the crop rectangle and the selection.
    #[cfg(feature = "edit")]
    marks: marks::Marks,
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
            #[cfg(feature = "edit")]
            marks: marks::Marks::default(),
        };
        viewport.zoom_to_fit();
        viewport
    }

    pub(crate) const fn media(&self) -> &Media {
        self.renderer.media()
    }

    /// Replaces the media, keeping the zoom or fitting it, and restarts the animation.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        // A selection only stays on a picture that changes its colors.
        #[cfg(feature = "edit")]
        self.marks.media_changed(
            media.size(),
            media.size() == self.media().size() && !zoom_to_fit,
        );
        self.renderer = MediaRenderer::new(media);
        self.frame = 0;
        self.start = Instant::now();
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

    /// Where the shown media is in the visible area: its position and magnified size.
    #[cfg(target_os = "macos")]
    pub(crate) fn media_rect(&self) -> (f64, f64, f64, f64) {
        let (x, y) = self.origin(self.scroll);
        let (width, height) = self.content();
        (x, y, width, height)
    }

    /// Draws the shown media into a canvas that starts at a position of the visible area, for a
    /// platform that shows a bitmap of a part of it.
    #[cfg(target_os = "macos")]
    pub(crate) fn paint_media(
        &self,
        context: &mut CanvasRenderingContext2d<'_>,
        (x, y): (f64, f64),
    ) {
        let origin = self.origin(self.scroll);
        self.draw_media(context, (origin.0 - x, origin.1 - y));
    }

    /// Draws the part inside `clip` of the checkerboard and media of a native scrolled document as
    /// large as the scroll bar lengths, which scrolls itself.
    #[cfg(not(any(target_os = "macos", windows)))]
    pub(crate) fn paint_document(
        &self,
        context: &mut CanvasRenderingContext2d<'_>,
        dark: bool,
        clip: (f32, f32, f32, f32),
    ) {
        draw_checkerboard_clipped(context, (context.width(), context.height()), clip, dark);
        self.draw_media(context, self.origin((0.0, 0.0)));
    }

    // Where the media is drawn in a canvas scrolled to a position.
    fn origin(&self, scroll: (f64, f64)) -> (f64, f64) {
        (
            self.offset(Axis::Horizontal) - scroll.0,
            self.offset(Axis::Vertical) - scroll.1,
        )
    }

    // Draws the shown media at its magnified size from an origin.
    #[cfg(not(windows))]
    fn draw_media(&self, context: &mut CanvasRenderingContext2d<'_>, origin: (f64, f64)) {
        let (renderer, frame) = self.drawn();
        let (width, height) = self.content();
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
    }

    /// What is shown: the preview or the media, with its frame. A preview is a still image,
    /// whichever frame the media is at.
    pub(crate) const fn drawn(&self) -> (&MediaRenderer, usize) {
        #[cfg(feature = "edit")]
        if let Some(preview) = &self.marks.preview {
            return (preview, 0);
        }
        (&self.renderer, self.frame)
    }

    /// Whether the shown media is a bitmap enlarged enough at this display scale to be drawn
    /// with sharp pixels.
    #[cfg(any(target_os = "macos", windows))]
    pub(crate) fn has_pixels(&self, scale: f32) -> bool {
        self.drawn().0.is_pixelated(self.content().0 as f32 * scale)
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
}

#[cfg(test)]
mod tests {
    use imageview_core::decode_media;

    use super::*;

    #[cfg(windows)]
    #[test]
    fn followed_rasters_are_pixelated_when_enlarged() {
        use std::sync::Arc;

        use imageview_core::Frame;

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
