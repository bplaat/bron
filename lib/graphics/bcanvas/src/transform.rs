/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

pub(crate) type Point = [f32; 2];

/// An affine transform `[a, b, c, d, e, f]`, like the Canvas 2D `setTransform` arguments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Transform(pub [f32; 6]);

impl Transform {
    pub(crate) const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    #[cfg_attr(not(any(windows, target_os = "android", test)), allow(dead_code))]
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

    #[cfg_attr(not(any(windows, target_os = "android", test)), allow(dead_code))]
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
