/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The rectangle of a crop or selection dragged over media: moved from inside, resized from
//! its edges and corners, or drawn anew from outside, keeping an aspect ratio and inside the
//! media.

/// A rectangle as its left, top, right and bottom edges.
pub(crate) type Edges = [f64; 4];

// Which edges a drag moves: left, top, right and bottom.
type Sides = [bool; 4];

const LEFT: usize = 0;
const TOP: usize = 1;
const RIGHT: usize = 2;
const BOTTOM: usize = 3;
const MOVE: Sides = [true; 4];

// The edge of an axis that stays in place while the other one follows the aspect ratio.
#[derive(Clone, Copy)]
enum Anchor {
    Start,
    End,
    Middle,
}

impl Anchor {
    // The largest size from this anchor between two edges that stays inside the bound.
    fn room(self, low: f64, high: f64, bound: f64) -> f64 {
        match self {
            Self::Start => bound - low,
            Self::End => high,
            Self::Middle => {
                let middle = f64::midpoint(low, high);
                2.0 * middle.min(bound - middle)
            }
        }
    }

    // The edges of a size placed at this anchor.
    fn place(self, low: f64, high: f64, size: f64) -> (f64, f64) {
        match self {
            Self::Start => (low, low + size),
            Self::End => (high - size, high),
            Self::Middle => {
                let middle = f64::midpoint(low, high);
                (middle - size / 2.0, middle + size / 2.0)
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Grab {
    start: (f64, f64),
    edges: Edges,
    sides: Sides,
}

/// A rectangle in media pixels.
pub(crate) struct Marquee {
    bounds: (f64, f64),
    edges: Edges,
    aspect: Option<f64>,
    // Whether the rectangle covers all of the media when it has no size, like a crop.
    fills: bool,
    grab: Option<Grab>,
}

impl Marquee {
    /// A crop rectangle, covering all of media of a size.
    pub(crate) const fn covering(bounds: (f64, f64)) -> Self {
        Self {
            bounds,
            edges: [0.0, 0.0, bounds.0, bounds.1],
            aspect: None,
            fills: true,
            grab: None,
        }
    }

    /// A selection rectangle without a size yet, over media of a size.
    pub(crate) const fn empty(bounds: (f64, f64)) -> Self {
        Self {
            bounds,
            edges: [0.0; 4],
            aspect: None,
            fills: false,
            grab: None,
        }
    }

    /// Whether the rectangle has no size.
    pub(crate) fn is_empty(&self) -> bool {
        let [left, top, right, bottom] = self.edges;
        right - left < 1.0 || bottom - top < 1.0
    }

    /// Whether a media position is inside or near the rectangle, where a drag changes it.
    pub(crate) fn contains(&self, (x, y): (f64, f64), reach: f64) -> bool {
        let [left, top, right, bottom] = self.edges;
        !self.is_empty()
            && x > left - reach
            && x < right + reach
            && y > top - reach
            && y < bottom + reach
    }

    pub(crate) const fn edges(&self) -> Edges {
        self.edges
    }

    /// The rectangle in whole pixels: its position and size.
    pub(crate) fn rect(&self) -> (u32, u32, u32, u32) {
        let [left, top, right, bottom] = self.edges.map(|edge| edge.round() as u32);
        (left, top, (right - left).max(1), (bottom - top).max(1))
    }

    /// Keeps a width divided by height, shrinking the rectangle around its middle.
    pub(crate) fn set_aspect(&mut self, aspect: Option<f64>) {
        self.aspect = aspect;
        let Some(aspect) = aspect else {
            return;
        };
        let [left, top, right, bottom] = self.edges;
        let (width, height) = (right - left, bottom - top);
        let (fitted_width, fitted_height) = if width / height > aspect {
            (height * aspect, height)
        } else {
            (width, width / aspect)
        };
        let (x, y) = (
            left + (width - fitted_width) / 2.0,
            top + (height - fitted_height) / 2.0,
        );
        self.edges = [x, y, x + fitted_width, y + fitted_height];
    }

    /// Takes hold at a media position: the edges within `reach` of it, all of the rectangle
    /// from inside, or a new rectangle from outside.
    pub(crate) fn begin(&mut self, point: (f64, f64), reach: f64) {
        let [left, top, right, bottom] = self.edges;
        let within = |value: f64, low: f64, high: f64| value > low - reach && value < high + reach;
        let near = |value: f64, edge: f64| (value - edge).abs() < reach;
        let sides = [
            near(point.0, left) && within(point.1, top, bottom) && !self.is_empty(),
            near(point.1, top) && within(point.0, left, right),
            near(point.0, right) && within(point.1, top, bottom),
            near(point.1, bottom) && within(point.0, left, right),
        ];
        // A new rectangle starts at the nearest point of the media.
        let (start, edges, sides) = if self.is_empty() {
            let (x, y) = self.clamp(point);
            ((x, y), [x, y, x, y], [false, false, true, true])
        } else if sides.contains(&true) {
            (point, self.edges, sides)
        } else if within(point.0, left, right) && within(point.1, top, bottom) {
            (point, self.edges, MOVE)
        } else {
            let (x, y) = self.clamp(point);
            ((x, y), [x, y, x, y], [false, false, true, true])
        };
        self.edges = edges;
        self.grab = Some(Grab {
            start,
            edges,
            sides,
        });
    }

    /// Follows the pointer at a media position, as a square when asked, returns whether the
    /// rectangle is held.
    pub(crate) fn drag(&mut self, point: (f64, f64), square: bool) -> bool {
        let Some(grab) = self.grab else {
            return false;
        };
        let delta = (point.0 - grab.start.0, point.1 - grab.start.1);
        let [mut left, mut top, mut right, mut bottom] = grab.edges;
        if grab.sides == MOVE {
            let (width, height) = (right - left, bottom - top);
            left = (left + delta.0).clamp(0.0, self.bounds.0 - width);
            top = (top + delta.1).clamp(0.0, self.bounds.1 - height);
            self.edges = [left, top, left + width, top + height];
            return true;
        }
        let sides = grab.sides;
        if sides[LEFT] {
            left += delta.0;
        }
        if sides[RIGHT] {
            right += delta.0;
        }
        if sides[TOP] {
            top += delta.1;
        }
        if sides[BOTTOM] {
            bottom += delta.1;
        }
        // Dragging past the opposite edge flips the rectangle.
        let (mut sides, mut edges) = (sides, [left, top, right, bottom]);
        for (low, high) in [(LEFT, RIGHT), (TOP, BOTTOM)] {
            if edges[low] > edges[high] {
                edges.swap(low, high);
                sides.swap(low, high);
            }
        }
        edges[LEFT] = edges[LEFT].max(0.0);
        edges[TOP] = edges[TOP].max(0.0);
        edges[RIGHT] = edges[RIGHT].min(self.bounds.0);
        edges[BOTTOM] = edges[BOTTOM].min(self.bounds.1);
        if let Some(aspect) = self.aspect.or(square.then_some(1.0)) {
            edges = self.keep_aspect(edges, sides, aspect);
        }
        self.edges = edges;
        true
    }

    // Changes the size to the aspect ratio from the dragged edges, keeping the opposite edges
    // in place and staying inside the media.
    fn keep_aspect(&self, edges: Edges, sides: Sides, aspect: f64) -> Edges {
        let [left, top, right, bottom] = edges;
        let vertical_only = !sides[LEFT] && !sides[RIGHT];
        let horizontal_only = !sides[TOP] && !sides[BOTTOM];
        let (mut width, mut height) = (right - left, bottom - top);
        if vertical_only {
            width = height * aspect;
        } else {
            height = width / aspect;
        }
        let anchor = |low: bool, high: bool, centered: bool| {
            if low && !high {
                Anchor::End
            } else if centered {
                Anchor::Middle
            } else {
                Anchor::Start
            }
        };
        let x = anchor(sides[LEFT], sides[RIGHT], vertical_only);
        let y = anchor(sides[TOP], sides[BOTTOM], horizontal_only);
        let scale = (x.room(left, right, self.bounds.0) / width)
            .min(y.room(top, bottom, self.bounds.1) / height)
            .min(1.0);
        let (left, right) = x.place(left, right, width * scale);
        let (top, bottom) = y.place(top, bottom, height * scale);
        [left, top, right, bottom]
    }

    /// Lets go, a crop too small to see covers all of the media again. Returns whether the
    /// rectangle was held.
    pub(crate) fn end(&mut self) -> bool {
        if self.grab.take().is_none() {
            return false;
        }
        if self.fills && self.is_empty() {
            self.edges = [0.0, 0.0, self.bounds.0, self.bounds.1];
            self.set_aspect(self.aspect);
        }
        true
    }

    /// Places the rectangle at a position with a size, inside the media.
    pub(crate) fn set_rect(&mut self, (x, y, width, height): (u32, u32, u32, u32)) {
        let [x, y, width, height] = [x, y, width, height].map(f64::from);
        self.edges = [
            x.min(self.bounds.0),
            y.min(self.bounds.1),
            (x + width).min(self.bounds.0),
            (y + height).min(self.bounds.1),
        ];
    }

    const fn clamp(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (x.clamp(0.0, self.bounds.0), y.clamp(0.0, self.bounds.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl Marquee {
        fn drag_free(&mut self, point: (f64, f64)) -> bool {
            self.drag(point, false)
        }
    }

    #[test]
    fn edges_resize_and_inside_moves() {
        let mut crop = Marquee::covering((100.0, 50.0));
        crop.begin((100.0, 25.0), 4.0);
        assert!(crop.drag_free((80.0, 40.0)));
        assert_eq!(crop.edges(), [0.0, 0.0, 80.0, 50.0]);
        crop.end();
        crop.begin((40.0, 25.0), 4.0);
        crop.drag_free((100.0, 0.0));
        assert_eq!(crop.edges(), [20.0, 0.0, 100.0, 50.0]);
        assert!(crop.end());
        assert!(!crop.drag_free((0.0, 0.0)));
    }

    #[test]
    fn dragging_outside_draws_a_new_rectangle() {
        let mut crop = Marquee::covering((100.0, 50.0));
        crop.begin((50.0, 25.0), 4.0);
        crop.drag_free((60.0, 30.0));
        crop.end();
        crop.begin((0.0 - 20.0, 80.0), 4.0);
        crop.drag_free((10.0, 10.0));
        assert_eq!(crop.edges(), [0.0, 10.0, 10.0, 50.0]);
        crop.end();
        // A click without dragging leaves nothing, which covers all again.
        crop.begin((90.0, 10.0), 4.0);
        crop.end();
        assert_eq!(crop.rect(), (0, 0, 100, 50));
    }

    #[test]
    fn selections_start_empty_and_keep_squares() {
        let mut selection = Marquee::empty((100.0, 50.0));
        assert!(selection.is_empty() && !selection.contains((0.0, 0.0), 4.0));
        selection.begin((10.0, 10.0), 4.0);
        selection.drag((40.0, 20.0), true);
        assert_eq!(selection.edges(), [10.0, 10.0, 40.0, 40.0]);
        selection.end();
        assert!(selection.contains((20.0, 20.0), 4.0));
        // A click without dragging leaves no selection.
        selection.begin((90.0, 45.0), 4.0);
        selection.end();
        assert!(selection.is_empty());
    }

    #[test]
    fn aspect_ratios_are_kept() {
        let mut crop = Marquee::covering((100.0, 50.0));
        crop.set_aspect(Some(1.0));
        assert_eq!(crop.edges(), [25.0, 0.0, 75.0, 50.0]);
        crop.begin((75.0, 50.0), 4.0);
        crop.drag_free((55.0, 50.0));
        assert_eq!(crop.edges(), [25.0, 0.0, 55.0, 30.0]);
        crop.end();
        // The size stays inside the media.
        crop.begin((55.0, 30.0), 4.0);
        crop.drag_free((200.0, 30.0));
        assert_eq!(crop.edges(), [25.0, 0.0, 75.0, 50.0]);
    }
}
