/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Device-independent path construction for backends without a mutable native path.
use std::f32::consts::{FRAC_PI_2, TAU};

pub(crate) type Point = [f32; 2];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Transform(pub [f32; 6]);

impl Transform {
    pub(crate) const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub(crate) fn point(self, [x, y]: Point) -> Point {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }

    pub(crate) fn concat(self, rhs: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = rhs.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }

    pub(crate) fn inverse(self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if !det.is_finite() || det == 0.0 {
            return None;
        }
        let inverse = Self([
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * f - d * e) / det,
            (b * e - a * f) / det,
        ]);
        inverse
            .0
            .iter()
            .all(|value| value.is_finite())
            .then_some(inverse)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Quadratic(Point, Point),
    Close,
}

#[derive(Default)]
pub(crate) struct Path {
    pub(crate) commands: Vec<Command>,
    current: Option<Point>,
    start: Option<Point>,
}

impl Path {
    pub(crate) fn move_to(&mut self, transform: Transform, point: Point) {
        let point = transform.point(point);
        self.commands.push(Command::Move(point));
        self.current = Some(point);
        self.start = Some(point);
    }

    pub(crate) fn line_to(&mut self, transform: Transform, point: Point) {
        if self.current.is_none() {
            self.move_to(transform, point);
        } else {
            let point = transform.point(point);
            self.commands.push(Command::Line(point));
            self.current = Some(point);
        }
    }

    pub(crate) fn cubic(&mut self, transform: Transform, a: Point, b: Point, end: Point) {
        if self.current.is_none() {
            self.move_to(transform, a);
        }
        let end = transform.point(end);
        self.commands
            .push(Command::Cubic(transform.point(a), transform.point(b), end));
        self.current = Some(end);
    }

    pub(crate) fn quadratic(&mut self, transform: Transform, control: Point, end: Point) {
        if self.current.is_none() {
            self.move_to(transform, control);
        }
        let end = transform.point(end);
        self.commands
            .push(Command::Quadratic(transform.point(control), end));
        self.current = Some(end);
    }

    pub(crate) fn close(&mut self) {
        if self.current.is_some() {
            self.commands.push(Command::Close);
            self.current = self.start;
        }
    }

    pub(crate) fn rect(&mut self, transform: Transform, x: f32, y: f32, w: f32, h: f32) {
        self.move_to(transform, [x, y]);
        self.line_to(transform, [x + w, y]);
        self.line_to(transform, [x + w, y + h]);
        self.line_to(transform, [x, y + h]);
        self.close();
    }

    pub(crate) fn ellipse(&mut self, transform: Transform, center: Point, radius: Point) {
        self.move_to(transform, [center[0] + radius[0], center[1]]);
        self.arc(transform, center, radius, 0.0, TAU, false);
        self.close();
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn arc(
        &mut self,
        transform: Transform,
        [x, y]: Point,
        [rx, ry]: Point,
        start: f32,
        end: f32,
        ccw: bool,
    ) {
        if rx < 0.0 || ry < 0.0 || ![x, y, rx, ry, start, end].iter().all(|v| v.is_finite()) {
            return;
        }
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
        self.line_to(transform, [x + rx * start.cos(), y + ry * start.sin()]);
        let steps = (sweep.abs() / FRAC_PI_2).ceil() as usize;
        for step in 0..steps {
            let a = start + sweep * step as f32 / steps as f32;
            let b = start + sweep * (step + 1) as f32 / steps as f32;
            let k = 4.0 / 3.0 * ((b - a) / 4.0).tan();
            self.cubic(
                transform,
                [
                    x + rx * (a.cos() - k * a.sin()),
                    y + ry * (a.sin() + k * a.cos()),
                ],
                [
                    x + rx * (b.cos() + k * b.sin()),
                    y + ry * (b.sin() - k * b.cos()),
                ],
                [x + rx * b.cos(), y + ry * b.sin()],
            );
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
    fn paths_capture_transform_at_construction() {
        let mut path = Path::default();
        path.move_to(Transform::IDENTITY, [1.0, 2.0]);
        path.line_to(Transform([1.0, 0.0, 0.0, 1.0, 10.0, 20.0]), [1.0, 2.0]);
        assert!(matches!(path.commands[0], Command::Move([1.0, 2.0])));
        assert!(matches!(path.commands[1], Command::Line([11.0, 22.0])));
    }

    #[test]
    fn full_circle_has_four_curves_and_counterclockwise_sweep_is_negative() {
        let mut path = Path::default();
        path.arc(Transform::IDENTITY, [0.0, 0.0], [1.0, 1.0], 0.0, TAU, false);
        assert_eq!(path.commands.len(), 5);
        let Command::Cubic(_, _, end) = path.commands[1] else {
            panic!("curve")
        };
        assert!((end[1] - 1.0).abs() < 0.00001);
        let mut path = Path::default();
        path.arc(
            Transform::IDENTITY,
            [0.0, 0.0],
            [1.0, 1.0],
            0.0,
            -FRAC_PI_2,
            true,
        );
        let Command::Cubic(_, _, end) = path.commands[1] else {
            panic!("curve")
        };
        assert!((end[1] + 1.0).abs() < 0.00001);
    }
}
