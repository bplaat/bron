/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! What editing draws over the media of the viewport: the preview of an edit in its place, and
//! the crop rectangle and selection that drags draw, move and resize.

use std::f32::consts::TAU;

use bcanvas::{CanvasRenderingContext2d, Color, FillRule, Path2D};
use imageview_core::edit::{Selection, Shape};
use imageview_core::{Media, MediaRenderer};

use super::{Axis, Viewport};
use crate::marquee::Marquee;

// How close to an edge of a crop or selection a drag resizes it, in logical pixels.
const CROP_REACH: f64 = 10.0;
// The length and thickness of the corner handles of the crop rectangle.
const HANDLE_LENGTH: f32 = 18.0;
const HANDLE_WIDTH: f32 = 3.0;

/// The preview of an edit, the crop rectangle and the selection of a viewport.
#[derive(Default)]
pub(super) struct Marks {
    // A smaller preview of an edit, drawn over the media in its place.
    pub(super) preview: Option<MediaRenderer>,
    crop: Option<Marquee>,
    selection: Option<(Shape, Marquee)>,
    inverted: bool,
    // The shape a drag outside the selection draws.
    selecting: Option<Shape>,
}

impl Marks {
    /// Follows new media of a size: the preview is gone and the crop rectangle covers it. A
    /// selection only stays when asked, on a picture that changes its colors.
    pub(super) fn media_changed(&mut self, size: (f64, f64), keep_selection: bool) {
        self.preview = None;
        if self.crop.is_some() {
            self.crop = Some(Marquee::covering(size));
        }
        if !keep_selection {
            self.selection = None;
        }
    }
}

impl Viewport {
    /// Shows a preview of any size in the place of the media, or the media again.
    pub(crate) fn set_preview(&mut self, preview: Option<Media>) {
        self.marks.preview = preview.map(MediaRenderer::new);
    }

    /// Shows a crop rectangle over all of the media, or removes it.
    pub(crate) fn set_cropping(&mut self, cropping: bool) {
        self.marks.crop = cropping.then(|| Marquee::covering(self.media().size()));
    }

    /// The crop rectangle, while cropping.
    pub(crate) const fn crop(&mut self) -> Option<&mut Marquee> {
        self.marks.crop.as_mut()
    }

    /// Makes drags draw a selection of a shape, which the selection takes too, or selects
    /// all of the media again, where drags pan it.
    pub(crate) const fn set_selecting(&mut self, shape: Option<Shape>) {
        self.marks.selecting = shape;
        match shape {
            Some(shape) => {
                if let Some((selected, _)) = &mut self.marks.selection {
                    *selected = shape;
                }
            }
            None => {
                self.marks.selection = None;
                self.marks.inverted = false;
            }
        }
    }

    /// The shape drags draw a selection of.
    pub(crate) const fn selecting(&self) -> Option<Shape> {
        self.marks.selecting
    }

    /// Whether drags draw a crop or selection, instead of panning.
    pub(crate) const fn is_marking(&self) -> bool {
        self.marks.crop.is_some() || self.marks.selecting.is_some()
    }

    /// The selected part of the media.
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.marks
            .selection
            .as_ref()
            .map(|(shape, marquee)| Selection {
                shape: *shape,
                edges: marquee.edges(),
                inverted: self.marks.inverted,
            })
    }

    /// Selects everything outside the selection instead.
    pub(crate) const fn invert_selection(&mut self) {
        if self.marks.selection.is_some() {
            self.marks.inverted = !self.marks.inverted;
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
        if let Some(crop) = &mut self.marks.crop {
            crop.begin(point, reach);
            return true;
        }
        let held = self
            .marks
            .selection
            .as_ref()
            .is_some_and(|(_, marquee)| marquee.contains(point, reach));
        if !held {
            let Some(shape) = self.marks.selecting else {
                return false;
            };
            self.marks.inverted = false;
            self.marks.selection = Some((shape, Marquee::empty(self.media().size())));
        }
        if let Some((_, marquee)) = &mut self.marks.selection {
            marquee.begin(point, reach);
        }
        true
    }

    /// Moves the held rectangle along, as a square when asked, returns whether one is held.
    pub(crate) fn mark_drag(&mut self, pointer: (f64, f64), square: bool) -> bool {
        let point = self.media_point(pointer);
        let crop = self
            .marks
            .crop
            .as_mut()
            .is_some_and(|crop| crop.drag(point, square));
        crop || self
            .marks
            .selection
            .as_mut()
            .is_some_and(|(_, marquee)| marquee.drag(point, square))
    }

    /// Lets go of the held rectangle, a selection without a size is removed. Returns whether
    /// one was held.
    pub(crate) fn mark_end(&mut self) -> bool {
        if self.marks.crop.as_mut().is_some_and(Marquee::end) {
            return true;
        }
        let ended = self
            .marks
            .selection
            .as_mut()
            .is_some_and(|(_, marquee)| marquee.end());
        if self
            .marks
            .selection
            .as_ref()
            .is_some_and(|(_, marquee)| marquee.is_empty())
        {
            self.marks.selection = None;
        }
        ended
    }

    /// Whether there is a crop rectangle or selection to draw.
    #[cfg(any(target_os = "macos", windows))]
    pub(crate) const fn has_marks(&self) -> bool {
        self.marks.crop.is_some() || self.marks.selection.is_some()
    }

    /// Draws the crop rectangle or selection over the visible area, for a platform that draws the
    /// media itself.
    #[cfg(any(target_os = "macos", windows))]
    pub(crate) fn paint_marks(&self, context: &mut CanvasRenderingContext2d<'_>) {
        self.draw_marks(context, self.origin(self.scroll));
    }

    /// Draws the crop rectangle or selection over the document drawn by `paint_document`.
    #[cfg(not(any(target_os = "macos", windows)))]
    pub(crate) fn paint_document_marks(&self, context: &mut CanvasRenderingContext2d<'_>) {
        self.draw_marks(context, self.origin((0.0, 0.0)));
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
        if let Some((shape, marquee)) = &self.marks.selection
            && self.marks.crop.is_none()
        {
            draw_selection(
                context,
                *shape,
                place(marquee),
                [x, y, x + width as f32, y + height as f32],
                self.marks.inverted,
            );
        }
        if let Some(crop) = &self.marks.crop {
            draw_crop(context, place(crop));
        }
    }
}

// The path of a shape.
fn shape_path(shape: Shape, [left, top, right, bottom]: [f32; 4]) -> Path2D {
    let mut path = Path2D::new();
    match shape {
        Shape::Rectangle => path.rect(left, top, right - left, bottom - top),
        Shape::Ellipse => {
            path.ellipse(
                f32::midpoint(left, right),
                f32::midpoint(top, bottom),
                (right - left) / 2.0,
                (bottom - top) / 2.0,
                0.0,
                0.0,
                TAU,
                false,
            );
            path.close_path();
        }
    }
    path
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
    let mut tint = Path2D::new();
    if !inverted {
        tint.rect(media[0], media[1], media[2] - media[0], media[3] - media[1]);
    }
    tint.add_path(&shape_path(shape, edges));
    context.set_fill_style(Color::rgba(0, 0, 0, 70));
    context.fill_path_with(&tint, FillRule::EvenOdd);

    let outline = shape_path(shape, edges.map(|edge| edge + 0.5));
    context.set_line_width(1.0);
    context.set_stroke_style(Color::rgb(255, 255, 255));
    context.set_line_dash(&[]);
    context.stroke_path(&outline);
    context.set_stroke_style(Color::rgb(0, 0, 0));
    context.set_line_dash(&[4.0, 4.0]);
    context.stroke_path(&outline);
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

// MARK: Tests
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
    fn crop_drags_follow_the_magnification() {
        // The 100x100 media is shown at 3x, 50 pixels from the left.
        let mut viewport = Viewport::new(Some(media()), (400.0, 300.0));
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
}
