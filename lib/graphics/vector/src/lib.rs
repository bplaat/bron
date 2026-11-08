/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(missing_docs)]
// Each decoder uses its own part of the shared geometry and paints.
#![cfg_attr(not(all(feature = "svg", feature = "tinyvg")), allow(dead_code))]

use std::fmt::{self, Display, Formatter};

use bcanvas::OffscreenCanvas;

mod draw;
#[cfg(feature = "svg")]
mod svg;
#[cfg(feature = "tinyvg")]
mod tinyvg;

#[cfg(all(test, any(feature = "svg", feature = "tinyvg")))]
#[cfg_attr(not(feature = "tinyvg"), allow(dead_code))]
#[path = "../../image/src/fixture_corpus.rs"]
mod test_support;

/// The interpretation of color channels. Alpha is always linear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColorSpace {
    /// Standard RGB.
    Srgb,
    /// Linear extended sRGB.
    LinearSrgb,
}

/// A supported vector file format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VectorFormat {
    /// Scalable Vector Graphics.
    Svg,
    /// Gzip-compressed Scalable Vector Graphics.
    Svgz,
    /// Binary TinyVG.
    TinyVg,
}

/// A vector decoding failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VectorDecodeError {
    /// No supported vector signature was found.
    InvalidMagic,
    /// The document is malformed or internally inconsistent.
    InvalidData,
    /// A recognized document requires an intentionally unsupported rendering feature.
    UnsupportedFeature(&'static str),
    /// A decoder resource limit was exceeded.
    ResourceLimit,
}

impl Display for VectorDecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => formatter.write_str("unsupported vector image format"),
            Self::InvalidData => formatter.write_str("invalid vector image data"),
            Self::UnsupportedFeature(feature) => {
                write!(formatter, "unsupported vector image feature: {feature}")
            }
            Self::ResourceLimit => {
                formatter.write_str("vector image exceeds decoding resource limits")
            }
        }
    }
}

impl std::error::Error for VectorDecodeError {}

/// A two-dimensional size in display units.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub(crate) struct Size {
    /// Horizontal extent.
    pub(crate) width: f64,
    /// Vertical extent.
    pub(crate) height: f64,
}

/// A point in the document's top-left-origin coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub(crate) struct Point {
    /// Horizontal coordinate.
    pub(crate) x: f64,
    /// Vertical coordinate.
    pub(crate) y: f64,
}

/// An axis-aligned rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Rect {
    /// Minimum horizontal coordinate.
    pub(crate) x: f64,
    /// Minimum vertical coordinate.
    pub(crate) y: f64,
    /// Horizontal extent.
    pub(crate) width: f64,
    /// Vertical extent.
    pub(crate) height: f64,
}

impl Rect {
    #[cfg(any(feature = "svg", feature = "tinyvg"))]
    pub(crate) fn from_points(a: Point, b: Point) -> Self {
        Self {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            width: (a.x - b.x).abs(),
            height: (a.y - b.y).abs(),
        }
    }

    #[cfg(any(feature = "svg", feature = "tinyvg"))]
    pub(crate) fn include(&mut self, point: Point) {
        let max_x = (self.x + self.width).max(point.x);
        let max_y = (self.y + self.height).max(point.y);
        self.x = self.x.min(point.x);
        self.y = self.y.min(point.y);
        self.width = max_x - self.x;
        self.height = max_y - self.y;
    }
}

/// A two-dimensional affine transform using Core Graphics' component order.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub(crate) struct Transform {
    /// Horizontal scale/rotation component.
    pub(crate) a: f64,
    /// Vertical rotation component.
    pub(crate) b: f64,
    /// Horizontal rotation component.
    pub(crate) c: f64,
    /// Vertical scale/rotation component.
    pub(crate) d: f64,
    /// Horizontal translation.
    pub(crate) e: f64,
    /// Vertical translation.
    pub(crate) f: f64,
}

impl Transform {
    /// The identity transform.
    pub(crate) const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Concatenates `other` after this transform.
    pub(crate) fn then(self, other: Self) -> Self {
        Self {
            a: other.a * self.a + other.c * self.b,
            b: other.b * self.a + other.d * self.b,
            c: other.a * self.c + other.c * self.d,
            d: other.b * self.c + other.d * self.d,
            e: other.a * self.e + other.c * self.f + other.e,
            f: other.b * self.e + other.d * self.f + other.f,
        }
    }

    /// Transforms a point.
    pub(crate) fn map(self, point: Point) -> Point {
        Point {
            x: self.a * point.x + self.c * point.y + self.e,
            y: self.b * point.x + self.d * point.y + self.f,
        }
    }
}

/// A normalized absolute path operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PathSegment {
    /// Starts a new subpath.
    MoveTo(Point),
    /// Adds a straight line.
    LineTo(Point),
    /// Adds a quadratic Bezier curve.
    QuadraticTo { control: Point, to: Point },
    /// Adds a cubic Bezier curve.
    CubicTo {
        control_0: Point,
        control_1: Point,
        to: Point,
    },
    /// Closes the current subpath.
    Close,
}

/// A straight-alpha color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Color {
    /// Red component.
    pub(crate) red: f64,
    /// Green component.
    pub(crate) green: f64,
    /// Blue component.
    pub(crate) blue: f64,
    /// Alpha component.
    pub(crate) alpha: f64,
    /// RGB transfer function.
    pub(crate) color_space: ColorSpace,
}

/// A gradient color stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GradientStop {
    /// Clamped, nondecreasing position.
    pub(crate) offset: f64,
    /// Stop color.
    pub(crate) color: Color,
}

/// Gradient behavior outside its first and last stops.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SpreadMethod {
    Pad,
    Reflect,
    Repeat,
}

/// A resolved vector paint.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Paint {
    /// A uniform color.
    Solid(Color),
    /// A linear gradient in paint space.
    LinearGradient {
        start: Point,
        end: Point,
        stops: Vec<GradientStop>,
        spread: SpreadMethod,
        transform: Transform,
    },
    /// A radial gradient in paint space.
    RadialGradient {
        center: Point,
        focal: Point,
        radius: f64,
        stops: Vec<GradientStop>,
        spread: SpreadMethod,
        transform: Transform,
    },
}

/// The rule used to determine the inside of a path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FillRule {
    NonZero,
    EvenOdd,
}

/// The shape at the ends of open strokes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LineCap {
    Butt,
    Round,
    Square,
}

/// The shape at stroke corners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// Resolved stroke parameters.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StrokeStyle {
    /// Stroke width.
    pub(crate) width: f64,
    /// End cap.
    pub(crate) line_cap: LineCap,
    /// Corner join.
    pub(crate) line_join: LineJoin,
    /// Miter limit.
    pub(crate) miter_limit: f64,
    /// Alternating dash and gap lengths.
    pub(crate) dash_array: Vec<f64>,
    /// Dash phase.
    pub(crate) dash_offset: f64,
}

/// The channel used to derive a mask's coverage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MaskType {
    /// Multiply coverage by the mask content's alpha channel.
    Alpha,
    /// Multiply coverage by the mask content's luminance and alpha channels.
    Luminance,
}

/// Compositing operation used when drawing a vector element or group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum BlendMode {
    /// Source-over compositing without color blending.
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// Detects a supported vector format without fully decoding it.
#[cfg_attr(
    not(any(feature = "svg", feature = "tinyvg")),
    allow(clippy::missing_const_for_fn)
)]
pub fn vector_format(_data: &[u8]) -> Option<VectorFormat> {
    #[cfg(feature = "tinyvg")]
    if tinyvg::is_tinyvg(_data) {
        return Some(VectorFormat::TinyVg);
    }
    #[cfg(feature = "svg")]
    if _data.starts_with(&[0x1f, 0x8b, 8]) {
        return Some(VectorFormat::Svgz);
    }
    #[cfg(feature = "svg")]
    if svg::is_svg(_data) {
        return Some(VectorFormat::Svg);
    }
    None
}

/// Decodes a supported vector image to a recording canvas of its intrinsic size, which
/// `draw_canvas` draws sharp at any scale.
pub fn decode_vector(data: &[u8]) -> Result<OffscreenCanvas, VectorDecodeError> {
    if data.len() > 64 * 1024 * 1024 {
        return Err(VectorDecodeError::ResourceLimit);
    }
    match vector_format(data) {
        #[cfg(feature = "svg")]
        Some(VectorFormat::Svg) => svg::decode(data),
        #[cfg(feature = "svg")]
        Some(VectorFormat::Svgz) => {
            let svg = gzip::decompress_with_limit(data, 64 * 1024 * 1024)
                .ok_or(VectorDecodeError::InvalidData)?;
            if !svg::is_svg(&svg) {
                return Err(VectorDecodeError::InvalidData);
            }
            svg::decode(&svg)
        }
        #[cfg(feature = "tinyvg")]
        Some(VectorFormat::TinyVg) => tinyvg::decode(data),
        _ => Err(VectorDecodeError::InvalidMagic),
    }
}

#[cfg(feature = "svg")]
pub(crate) fn path_bounds(path: &[PathSegment]) -> Option<Rect> {
    let mut bounds = None;
    for segment in path {
        let points = match *segment {
            PathSegment::MoveTo(point) | PathSegment::LineTo(point) => [Some(point), None, None],
            PathSegment::QuadraticTo { control, to } => [Some(control), Some(to), None],
            PathSegment::CubicTo {
                control_0,
                control_1,
                to,
            } => [Some(control_0), Some(control_1), Some(to)],
            PathSegment::Close => [None, None, None],
        };
        for point in points.into_iter().flatten() {
            if let Some(bounds) = &mut bounds {
                Rect::include(bounds, point);
            } else {
                bounds = Some(Rect::from_points(point, point));
            }
        }
    }
    bounds
}

#[cfg(any(feature = "svg", feature = "tinyvg"))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn append_arc(
    output: &mut Vec<PathSegment>,
    from: Point,
    to: Point,
    mut rx: f64,
    mut ry: f64,
    rotation: f64,
    large_arc: bool,
    sweep: bool,
) {
    if from == to {
        return;
    }
    if rx == 0.0 || ry == 0.0 {
        output.push(PathSegment::LineTo(to));
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    let angle = rotation.to_radians();
    let (sin, cos) = angle.sin_cos();
    let dx = (from.x - to.x) / 2.0;
    let dy = (from.y - to.y) / 2.0;
    let x = cos * dx + sin * dy;
    let y = -sin * dx + cos * dy;
    let scale = x * x / (rx * rx) + y * y / (ry * ry);
    if scale > 1.0 {
        let scale = scale.sqrt();
        rx *= scale;
        ry *= scale;
    }
    let numerator = (rx * ry).powi(2) - (rx * y).powi(2) - (ry * x).powi(2);
    let denominator = (rx * y).powi(2) + (ry * x).powi(2);
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let factor = if denominator == 0.0 {
        0.0
    } else {
        sign * (numerator.max(0.0) / denominator).sqrt()
    };
    let cx0 = factor * rx * y / ry;
    let cy0 = factor * -ry * x / rx;
    let center = Point {
        x: cos * cx0 - sin * cy0 + (from.x + to.x) / 2.0,
        y: sin * cx0 + cos * cy0 + (from.y + to.y) / 2.0,
    };
    let vector_angle =
        |ux: f64, uy: f64, vx: f64, vy: f64| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let start = ((x - cx0) / rx, (y - cy0) / ry);
    let end = ((-x - cx0) / rx, (-y - cy0) / ry);
    let start_angle = vector_angle(1.0, 0.0, start.0, start.1);
    let mut delta = vector_angle(start.0, start.1, end.0, end.1);
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    } else if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    let count = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize;
    let step = delta / count as f64;
    for index in 0..count {
        let first = start_angle + step * index as f64;
        let second = first + step;
        let alpha = 4.0 / 3.0 * (step / 4.0).tan();
        let map = |x: f64, y: f64| Point {
            x: center.x + cos * rx * x - sin * ry * y,
            y: center.y + sin * rx * x + cos * ry * y,
        };
        let (s0, c0) = first.sin_cos();
        let (s1, c1) = second.sin_cos();
        output.push(PathSegment::CubicTo {
            control_0: map(c0 - alpha * s0, s0 + alpha * c0),
            control_1: map(c1 + alpha * s1, s1 - alpha * c1),
            to: if index + 1 == count { to } else { map(c1, s1) },
        });
    }
}

// MARK: Tests

/// Reads a pixel of a decoded image drawn at its size, as straight RGBA.
#[cfg(test)]
pub(crate) fn pixel(image: &mut OffscreenCanvas, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * image.width() + x) * 4) as usize;
    image.pixels()[offset..offset + 4]
        .try_into()
        .expect("RGBA pixel")
}

#[cfg(all(test, feature = "svg"))]
mod svgz_tests {
    use super::*;

    #[test]
    fn svgz_draws_like_svg() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="8"><rect width="6" height="8" fill="red"/></svg>"#;
        let compressed = gzip::compress(svg);
        assert_eq!(vector_format(&compressed), Some(VectorFormat::Svgz));
        let mut image = decode_vector(&compressed).expect("SVGZ");
        assert_eq!(image.pixels(), decode_vector(svg).expect("SVG").pixels());
        assert_eq!(pixel(&mut image, 2, 2), [255, 0, 0, 255]);
    }

    #[test]
    fn damaged_gzip_and_non_svg_payloads_fail() {
        let mut compressed = gzip::compress(b"<svg/>");
        compressed[10] ^= 255;
        assert!(decode_vector(&compressed).is_err());
        assert!(decode_vector(&[0x1f, 0x8b, 8]).is_err());
        assert!(decode_vector(&gzip::compress(b"<html/>")).is_err());
        assert!(decode_vector(&gzip::compress(b"<svg>")).is_err());
    }
}
