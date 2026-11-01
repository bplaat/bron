/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The platform independent part of the media view: the magnification, scroll position, drag
//! and animation of the media inside the visible area, and drawing it above a checkerboard. The
//! platform views forward their input here and show the scroll bars it describes.

use std::time::{Duration, Instant};

use bcanvas::CanvasRenderingContext2d;
use imageview_core::{
    Media, MediaRenderer, clamp_magnification, draw_checkerboard, fit_magnification,
};

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
}

impl Viewport {
    /// Shows the media zoomed to fit the visible area.
    pub(crate) fn new(media: Media, visible: (f64, f64)) -> Self {
        let mut viewport = Self {
            renderer: MediaRenderer::new(media),
            visible,
            magnification: 1.0,
            scroll: (0.0, 0.0),
            drag: None,
            frame: 0,
            start: Instant::now(),
        };
        viewport.zoom_to_fit();
        viewport
    }

    pub(crate) const fn media(&self) -> &Media {
        self.renderer.media()
    }

    /// Replaces the media, keeping the zoom or fitting it, and restarts the animation.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
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
        let (content, visible) = (self.content(), self.visible);
        // Ignore sub-pixel overflow from rounding.
        content.0 - visible.0 > 0.5 || content.1 - visible.1 > 0.5
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
            length: axis.of(self.content()).max(page),
        }
    }

    /// Moves to the current animation frame, returns whether it changed and how long until
    /// the next one.
    pub(crate) fn advance_frame(&mut self) -> (bool, Option<Duration>) {
        let (frame, next) = self.media().frame_at(self.start.elapsed());
        (std::mem::replace(&mut self.frame, frame) != frame, next)
    }

    /// Draws the checkerboard filling the visible area and the scrolled media above it.
    #[cfg(any(target_os = "macos", windows))]
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
        self.renderer.draw(
            context,
            self.frame,
            (
                (self.offset(Axis::Horizontal) - scroll.0) as f32,
                (self.offset(Axis::Vertical) - scroll.1) as f32,
                width as f32,
                height as f32,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use imageview_core::decode_media;

    use super::*;

    // An animation of 100x100 pixels.
    fn media() -> Media {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/examples/ball.gif"))
            .expect("read example");
        decode_media(&bytes).expect("decode example")
    }

    #[test]
    fn fitting_centers_the_media() {
        let viewport = Viewport::new(media(), (400.0, 300.0));
        assert_eq!(viewport.magnification, 3.0);
        assert_eq!(viewport.offset(Axis::Horizontal), 50.0);
        assert_eq!(viewport.scroll, (0.0, 0.0));
        assert!(!viewport.is_scrollable());
    }

    #[test]
    fn magnifying_keeps_the_anchor_in_place() {
        let mut viewport = Viewport::new(media(), (100.0, 100.0));
        viewport.magnify(2.0, Some((25.0, 25.0)));
        assert_eq!(viewport.scroll, (25.0, 25.0));
        viewport.magnify(1.0, Some((25.0, 25.0)));
        assert_eq!(viewport.scroll, (0.0, 0.0));
    }

    #[test]
    fn scrolling_stays_inside_the_media() {
        let mut viewport = Viewport::new(media(), (100.0, 100.0));
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
        let mut viewport = Viewport::new(media(), (100.0, 100.0));
        viewport.zoom_centered(4.0);
        assert_eq!(viewport.scroll, (150.0, 150.0));
        viewport.resize((80.0, 90.0));
        assert_eq!(viewport.scroll, (160.0, 155.0));
    }

    #[test]
    fn dragging_moves_the_media_with_the_pointer() {
        let mut viewport = Viewport::new(media(), (100.0, 100.0));
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
