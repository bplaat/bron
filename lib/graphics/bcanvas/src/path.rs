/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Device-independent path construction, shared by the current path, Path2D and every backend.
use std::f32::consts::{FRAC_PI_2, TAU};

use crate::canvas::FillRule;
pub(crate) use crate::transform::{Point, Transform};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Quadratic(Point, Point),
    Close,
}

/// A path whose every subpath starts with a move, so backends never continue a closed one.
#[derive(Clone, Debug, Default)]
pub(crate) struct Path {
    pub(crate) commands: Vec<Command>,
    // A unique id of this version of a reusable path, which backends key native paths on, or
    // zero for a path that is not reused.
    pub(crate) id: u64,
    current: Option<Point>,
    start: Option<Point>,
    // Whether the last subpath has a move and is not closed.
    open: bool,
    // The command count, center and radii of an axis-aligned ellipse that is the whole path.
    ellipse: Option<(usize, Point, Point)>,
}

impl PartialEq for Path {
    fn eq(&self, other: &Self) -> bool {
        self.commands == other.commands
            && self.current == other.current
            && self.start == other.start
            && self.open == other.open
    }
}

impl Path {
    pub(crate) fn clear(&mut self) {
        let mut commands = std::mem::take(&mut self.commands);
        commands.clear();
        *self = Self {
            commands,
            ..Self::default()
        };
    }

    #[cfg(test)]
    pub(crate) const fn current(&self) -> Option<Point> {
        self.current
    }

    // Continue the current subpath, or start one at the current point after a close.
    fn ensure_open(&mut self, point: Point) {
        if !self.open {
            self.move_to(self.current.unwrap_or(point));
        }
    }

    pub(crate) fn move_to(&mut self, point: Point) {
        self.commands.push(Command::Move(point));
        self.current = Some(point);
        self.start = Some(point);
        self.open = true;
    }

    pub(crate) fn line_to(&mut self, point: Point) {
        if self.current.is_none() {
            self.move_to(point);
            return;
        }
        self.ensure_open(point);
        self.commands.push(Command::Line(point));
        self.current = Some(point);
    }

    pub(crate) fn quadratic(&mut self, control: Point, end: Point) {
        self.ensure_open(control);
        self.commands.push(Command::Quadratic(control, end));
        self.current = Some(end);
    }

    pub(crate) fn cubic(&mut self, a: Point, b: Point, end: Point) {
        self.ensure_open(a);
        self.commands.push(Command::Cubic(a, b, end));
        self.current = Some(end);
    }

    pub(crate) fn close(&mut self) {
        if self.open {
            self.commands.push(Command::Close);
            self.current = self.start;
            self.open = false;
        }
    }

    pub(crate) fn rect(&mut self, x: f32, y: f32, w: f32, h: f32) {
        self.move_to([x, y]);
        self.line_to([x + w, y]);
        self.line_to([x + w, y + h]);
        self.line_to([x, y + h]);
        self.close();
    }

    /// Add a rectangle with rounded corners whose radii are, like the HTML canvas, those of
    /// the upper left, upper right, lower right and lower left corners. The corners follow the
    /// winding of `rect` when the width or height is negative.
    pub(crate) fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radii: [f32; 4]) {
        // The radii of the corners in drawing order, which a negative size mirrors.
        let role = |right: bool, bottom: bool| match (right, bottom) {
            (false, false) => 0,
            (true, false) => 1,
            (true, true) => 2,
            (false, true) => 3,
        };
        let (flip_x, flip_y) = (w < 0.0, h < 0.0);
        let mut r = [
            role(flip_x, flip_y),
            role(!flip_x, flip_y),
            role(!flip_x, !flip_y),
            role(flip_x, !flip_y),
        ]
        .map(|corner| radii[corner]);
        // Overlapping radii scale down together, like the HTML canvas.
        let (width, height) = (w.abs(), h.abs());
        let scale = [
            width / (r[0] + r[1]),
            height / (r[1] + r[2]),
            width / (r[2] + r[3]),
            height / (r[3] + r[0]),
        ]
        .into_iter()
        .filter(|scale| scale.is_finite())
        .fold(1.0f32, f32::min);
        r = r.map(|radius| radius * scale);

        let corners = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
        let (sx, sy) = (w.signum(), h.signum());
        // The unit directions of the edges leaving each corner.
        let edges = [[sx, 0.0], [0.0, sy], [-sx, 0.0], [0.0, -sy]];
        let offset =
            |[px, py]: Point, [dx, dy]: Point, length: f32| [px + dx * length, py + dy * length];
        // Cubic approximation of a quarter circle.
        let k = 4.0 / 3.0 * (FRAC_PI_2 / 4.0).tan();
        self.move_to(offset(corners[0], edges[0], r[0]));
        for index in 1..=4 {
            let (corner, radius) = (corners[index % 4], r[index % 4]);
            let (incoming, outgoing) = (edges[index - 1], edges[index % 4]);
            let back = [-incoming[0], -incoming[1]];
            self.line_to(offset(corner, back, radius));
            if radius > 0.0 {
                self.cubic(
                    offset(corner, back, radius * (1.0 - k)),
                    offset(corner, outgoing, radius * (1.0 - k)),
                    offset(corner, outgoing, radius),
                );
            }
        }
        self.close();
        self.current = Some([x, y]);
        self.start = Some([x, y]);
    }

    /// Add an elliptical arc, rotated around its center, with a line from the current point.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ellipse(
        &mut self,
        center: Point,
        [rx, ry]: Point,
        rotation: f32,
        start: f32,
        end: f32,
        ccw: bool,
    ) {
        let raw = end - start;
        let sweep = if !ccw && raw >= TAU {
            TAU
        } else if ccw && -raw >= TAU {
            -TAU
        } else if ccw {
            -(-raw).rem_euclid(TAU)
        } else {
            raw.rem_euclid(TAU)
        };
        let empty = self.commands.is_empty();
        let (sin, cos) = rotation.sin_cos();
        let local = Transform([cos, sin, -sin, cos, center[0], center[1]]);
        let point = |x: f32, y: f32| local.point([rx * x, ry * y]);
        self.line_to(point(start.cos(), start.sin()));
        let steps = (sweep.abs() / FRAC_PI_2).ceil() as usize;
        for step in 0..steps {
            let a = start + sweep * step as f32 / steps as f32;
            let b = start + sweep * (step + 1) as f32 / steps as f32;
            let k = 4.0 / 3.0 * ((b - a) / 4.0).tan();
            let ((sin_a, cos_a), (sin_b, cos_b)) = (a.sin_cos(), b.sin_cos());
            self.cubic(
                point(cos_a - k * sin_a, sin_a + k * cos_a),
                point(cos_b + k * sin_b, sin_b - k * cos_b),
                point(cos_b, sin_b),
            );
        }
        if empty && rotation == 0.0 && sweep.abs() == TAU {
            self.ellipse = Some((self.commands.len(), center, [rx, ry]));
        }
    }

    /// Add an arc of a circle tangent to the lines from the current point through `a` to `b`,
    /// with a line to its start, like the HTML canvas `arcTo`.
    pub(crate) fn arc_to(&mut self, a: Point, b: Point, radius: f32) {
        let Some(p) = self.current else {
            self.move_to(a);
            return;
        };
        let unit = |[x, y]: Point| {
            let length = x.hypot(y);
            [x / length, y / length]
        };
        let u = unit([p[0] - a[0], p[1] - a[1]]);
        let v = unit([b[0] - a[0], b[1] - a[1]]);
        let cross = u[0] * v[1] - u[1] * v[0];
        if radius == 0.0 || p == a || a == b || cross.abs() < 1e-6 || !cross.is_finite() {
            self.line_to(a);
            return;
        }
        // The tangent points lie on both lines at the same distance from the corner.
        let half = (u[0] * v[0] + u[1] * v[1]).clamp(-1.0, 1.0).acos() / 2.0;
        let distance = radius / half.tan();
        let bisector = unit([u[0] + v[0], u[1] + v[1]]);
        let center_distance = radius / half.sin();
        let center = [
            a[0] + bisector[0] * center_distance,
            a[1] + bisector[1] * center_distance,
        ];
        let angle = |[x, y]: Point| (y - center[1]).atan2(x - center[0]);
        let start = angle([a[0] + u[0] * distance, a[1] + u[1] * distance]);
        let end = angle([a[0] + v[0] * distance, a[1] + v[1] * distance]);
        self.ellipse(center, [radius, radius], 0.0, start, end, cross > 0.0);
    }

    /// Append another path mapped through a transform.
    pub(crate) fn add_path(&mut self, other: &Self, transform: Transform) {
        if self.commands.is_empty() && transform == Transform::IDENTITY {
            self.clone_from(other);
            return;
        }
        self.commands
            .extend(other.commands.iter().map(|command| command.map(transform)));
        if other.current.is_some() {
            self.current = other.current.map(|point| transform.point(point));
            self.start = other.start.map(|point| transform.point(point));
            self.open = other.open;
        }
        self.ellipse = None;
    }

    /// Map every point through a transform.
    pub(crate) fn transform(&mut self, transform: Transform) {
        for command in &mut self.commands {
            *command = command.map(transform);
        }
        self.current = self.current.map(|point| transform.point(point));
        self.start = self.start.map(|point| transform.point(point));
        self.ellipse = None;
    }

    /// The center and radii when the path is one axis-aligned ellipse, which backends can
    /// draw without building a path.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn lone_ellipse(&self) -> Option<(Point, Point)> {
        self.ellipse
            .filter(|(count, ..)| *count == self.commands.len())
            .map(|(_, center, radii)| (center, radii))
    }

    /// Whether a point is inside the path, with every subpath closed.
    pub(crate) fn contains(&self, [px, py]: Point, rule: FillRule) -> bool {
        const STEPS: usize = 16;
        let mut winding = 0;
        let mut edge = |a: Point, b: Point| {
            if (a[1] <= py) != (b[1] <= py) {
                let t = (py - a[1]) / (b[1] - a[1]);
                if a[0] + t * (b[0] - a[0]) > px {
                    winding += if b[1] > a[1] { 1 } else { -1 };
                }
            }
        };
        let (mut start, mut current) = ([0.0; 2], [0.0; 2]);
        for command in &self.commands {
            match *command {
                Command::Move(point) => {
                    edge(current, start);
                    (start, current) = (point, point);
                }
                Command::Line(point) => {
                    edge(current, point);
                    current = point;
                }
                Command::Quadratic(c, end) => {
                    let p0 = current;
                    for step in 1..=STEPS {
                        let t = step as f32 / STEPS as f32;
                        let s = 1.0 - t;
                        let point =
                            [0, 1].map(|i| s * s * p0[i] + 2.0 * s * t * c[i] + t * t * end[i]);
                        edge(current, point);
                        current = point;
                    }
                }
                Command::Cubic(a, b, end) => {
                    let p0 = current;
                    for step in 1..=STEPS {
                        let t = step as f32 / STEPS as f32;
                        let s = 1.0 - t;
                        let point = [0, 1].map(|i| {
                            s * s * s * p0[i]
                                + 3.0 * s * s * t * a[i]
                                + 3.0 * s * t * t * b[i]
                                + t * t * t * end[i]
                        });
                        edge(current, point);
                        current = point;
                    }
                }
                Command::Close => {
                    edge(current, start);
                    current = start;
                }
            }
        }
        edge(current, start);
        match rule {
            FillRule::NonZero => winding != 0,
            FillRule::EvenOdd => winding % 2 != 0,
        }
    }
}

impl Command {
    fn map(self, transform: Transform) -> Self {
        let point = |point| transform.point(point);
        match self {
            Self::Move(to) => Self::Move(point(to)),
            Self::Line(to) => Self::Line(point(to)),
            Self::Cubic(a, b, to) => Self::Cubic(point(a), point(b), point(to)),
            Self::Quadratic(control, to) => Self::Quadratic(point(control), point(to)),
            Self::Close => Self::Close,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_compose_and_invert() {
        let transform = Transform([1.0, 0.0, 0.0, 1.0, 10.0, 20.0])
            .concat(Transform([2.0, 0.0, 0.0, 3.0, 0.0, 0.0]));
        assert_eq!(transform.point([2.0, 3.0]), [14.0, 29.0]);
        let point = transform.inverse().expect("invertible").point([14.0, 29.0]);
        assert!((point[0] - 2.0).abs() < 0.00001 && (point[1] - 3.0).abs() < 0.00001);
        assert!(Transform([0.0; 6]).inverse().is_none());
    }

    #[test]
    fn small_finite_scales_are_invertible() {
        let transform = Transform([0.0001, 0.0, 0.0, 0.0001, 0.0, 0.0]);
        let point = transform
            .inverse()
            .expect("small but invertible")
            .point(transform.point([2.0, 3.0]));
        assert!((point[0] - 2.0).abs() < 0.00001 && (point[1] - 3.0).abs() < 0.00001);
        assert!(
            Transform([f32::INFINITY, 0.0, 0.0, 1.0, 0.0, 0.0])
                .inverse()
                .is_none()
        );
        assert!(
            Transform([0.5, 0.0, 0.0, 0.5, f32::MAX, 0.0])
                .inverse()
                .is_none()
        );
    }

    #[test]
    fn full_circle_has_four_curves_and_counterclockwise_sweep_is_negative() {
        let mut path = Path::default();
        path.ellipse([0.0, 0.0], [1.0, 1.0], 0.0, 0.0, TAU, false);
        assert_eq!(path.commands.len(), 5);
        assert_eq!(path.lone_ellipse(), Some(([0.0, 0.0], [1.0, 1.0])));
        let Command::Cubic(_, _, end) = path.commands[1] else {
            panic!("curve")
        };
        assert!((end[1] - 1.0).abs() < 0.00001);
        let mut path = Path::default();
        path.ellipse([0.0, 0.0], [1.0, 1.0], 0.0, 0.0, -FRAC_PI_2, true);
        let Command::Cubic(_, _, end) = path.commands[1] else {
            panic!("curve")
        };
        assert!((end[1] + 1.0).abs() < 0.00001);
        path.line_to([5.0, 5.0]);
        assert_eq!(path.lone_ellipse(), None);
    }

    #[test]
    fn rotated_ellipse_starts_on_its_rotated_axis() {
        let mut path = Path::default();
        path.ellipse([10.0, 10.0], [4.0, 2.0], FRAC_PI_2, 0.0, TAU, false);
        let Command::Move([x, y]) = path.commands[0] else {
            panic!("move")
        };
        assert!((x - 10.0).abs() < 1e-5 && (y - 14.0).abs() < 1e-5);
        assert_eq!(path.lone_ellipse(), None);
    }

    #[test]
    fn segments_after_close_start_a_subpath_at_the_close_point() {
        let mut path = Path::default();
        path.rect(1.0, 2.0, 3.0, 4.0);
        path.line_to([9.0, 9.0]);
        assert_eq!(path.commands[5], Command::Move([1.0, 2.0]));
        assert_eq!(path.commands[6], Command::Line([9.0, 9.0]));
    }

    #[test]
    fn arc_to_rounds_a_corner_between_the_tangent_points() {
        let mut path = Path::default();
        path.move_to([0.0, 0.0]);
        path.arc_to([10.0, 0.0], [10.0, 10.0], 5.0);
        assert_eq!(path.commands[1], Command::Line([5.0, 0.0]));
        let end = path.current().expect("current point");
        assert!((end[0] - 10.0).abs() < 1e-4 && (end[1] - 5.0).abs() < 1e-4);
        // Collinear points only add a line to the corner.
        let mut path = Path::default();
        path.move_to([0.0, 0.0]);
        path.arc_to([5.0, 0.0], [10.0, 0.0], 5.0);
        assert_eq!(path.commands[1], Command::Line([5.0, 0.0]));
    }

    #[test]
    fn round_rect_scales_overlapping_radii_and_keeps_rect_winding() {
        let mut path = Path::default();
        path.round_rect(0.0, 0.0, 10.0, 10.0, [10.0; 4]);
        assert_eq!(path.commands[0], Command::Move([5.0, 0.0]));
        assert!(path.contains([5.0, 5.0], FillRule::NonZero));
        assert!(!path.contains([0.5, 0.5], FillRule::NonZero));
        assert_eq!(path.current(), Some([0.0, 0.0]));
        // A negative width flips the drawing direction, so it cancels a positive rect.
        let mut path = Path::default();
        path.rect(0.0, 0.0, 10.0, 10.0);
        path.round_rect(10.0, 0.0, -10.0, 10.0, [1.0, 0.0, 0.0, 0.0]);
        assert!(!path.contains([5.0, 5.0], FillRule::NonZero));
        assert!(path.contains([0.1, 0.1], FillRule::NonZero));
    }

    #[test]
    fn contains_follows_the_fill_rule() {
        let mut path = Path::default();
        path.rect(0.0, 0.0, 10.0, 10.0);
        path.rect(2.0, 2.0, 6.0, 6.0);
        assert!(path.contains([5.0, 5.0], FillRule::NonZero));
        assert!(!path.contains([5.0, 5.0], FillRule::EvenOdd));
        assert!(path.contains([1.0, 1.0], FillRule::EvenOdd));
        assert!(!path.contains([11.0, 5.0], FillRule::NonZero));
    }

    #[test]
    fn added_paths_map_through_their_transform() {
        let mut other = Path::default();
        other.move_to([1.0, 2.0]);
        let mut path = Path::default();
        path.add_path(&other, Transform([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]));
        assert_eq!(path.commands[0], Command::Move([11.0, 22.0]));
        path.transform(Transform([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]));
        assert_eq!(path.current(), Some([22.0, 44.0]));
    }
}
