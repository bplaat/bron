/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(missing_docs)]

use std::fmt::{self, Display, Formatter};

#[cfg(feature = "svg")]
mod svg;
#[cfg(feature = "tinyvg")]
mod tinyvg;

#[cfg(all(test, any(feature = "svg", feature = "tinyvg")))]
mod test_support;

/// The interpretation of color channels. Alpha is always linear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorSpace {
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
pub struct Size {
    /// Horizontal extent.
    pub width: f64,
    /// Vertical extent.
    pub height: f64,
}

/// A point in the document's top-left-origin coordinate system.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

/// An axis-aligned rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Minimum horizontal coordinate.
    pub x: f64,
    /// Minimum vertical coordinate.
    pub y: f64,
    /// Horizontal extent.
    pub width: f64,
    /// Vertical extent.
    pub height: f64,
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

    #[cfg(any(feature = "svg", feature = "tinyvg"))]
    pub(crate) fn expand(self, amount: f64) -> Self {
        Self {
            x: self.x - amount,
            y: self.y - amount,
            width: self.width + amount * 2.0,
            height: self.height + amount * 2.0,
        }
    }
}

/// A two-dimensional affine transform using Core Graphics' component order.
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(C)]
pub struct Transform {
    /// Horizontal scale/rotation component.
    pub a: f64,
    /// Vertical rotation component.
    pub b: f64,
    /// Horizontal rotation component.
    pub c: f64,
    /// Vertical scale/rotation component.
    pub d: f64,
    /// Horizontal translation.
    pub e: f64,
    /// Vertical translation.
    pub f: f64,
}

impl Transform {
    /// The identity transform.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// Concatenates `other` after this transform.
    pub fn then(self, other: Self) -> Self {
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
    pub fn map(self, point: Point) -> Point {
        Point {
            x: self.a * point.x + self.c * point.y + self.e,
            y: self.b * point.x + self.d * point.y + self.f,
        }
    }
}

/// An opaque path index within a [`VectorImage`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PathId(pub(crate) usize);

impl PathId {
    #[doc(hidden)]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// An opaque paint index within a [`VectorImage`].
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PaintId(pub(crate) usize);

impl PaintId {
    #[doc(hidden)]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// A normalized absolute path operation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathSegment {
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
pub struct Color {
    /// Red component.
    pub red: f64,
    /// Green component.
    pub green: f64,
    /// Blue component.
    pub blue: f64,
    /// Alpha component.
    pub alpha: f64,
    /// RGB transfer function.
    pub color_space: ColorSpace,
}

/// A gradient color stop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    /// Clamped, nondecreasing position.
    pub offset: f64,
    /// Stop color.
    pub color: Color,
}

/// Gradient behavior outside its first and last stops.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpreadMethod {
    Pad,
    Reflect,
    Repeat,
}

/// A resolved vector paint.
#[derive(Clone, Debug, PartialEq)]
pub enum Paint {
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
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// The shape at the ends of open strokes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

/// The shape at stroke corners.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

/// Resolved stroke parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    /// Stroke width.
    pub width: f64,
    /// End cap.
    pub line_cap: LineCap,
    /// Corner join.
    pub line_join: LineJoin,
    /// Miter limit.
    pub miter_limit: f64,
    /// Alternating dash and gap lengths.
    pub dash_array: Vec<f64>,
    /// Dash phase.
    pub dash_offset: f64,
}

/// A resolved clipping path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clip {
    /// Clip geometry.
    pub path: PathId,
    /// Geometry transform.
    pub transform: Transform,
    /// Clip fill rule.
    pub rule: FillRule,
    /// Conservative document-space bounds.
    pub bounds: Rect,
}

/// The channel used to derive a mask's coverage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MaskType {
    /// Multiply coverage by the mask content's alpha channel.
    Alpha,
    /// Multiply coverage by the mask content's luminance and alpha channels.
    Luminance,
}

/// Compositing operation used when drawing a vector element or group.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BlendMode {
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

/// A resolved local SVG mask.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    /// Mask drawing operations in paint order.
    pub commands: Vec<DrawCommand>,
    /// The mask region, including its transform and conservative bounds.
    pub region: Clip,
    /// The channel used to derive coverage.
    pub mask_type: MaskType,
}

/// One operation in a vector display list.
#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    /// Saves graphics state, applies optional clipping, and starts opacity isolation.
    PushScope {
        opacity: f64,
        blend_mode: BlendMode,
        isolated: bool,
        clips: Vec<Clip>,
        mask: Option<Mask>,
        bounds: Rect,
    },
    /// Ends the most recently pushed scope.
    PopScope,
    /// Fills a path.
    Fill {
        path: PathId,
        paint: PaintId,
        transform: Transform,
        rule: FillRule,
        bounds: Rect,
    },
    /// Strokes a path.
    Stroke {
        path: PathId,
        paint: PaintId,
        transform: Transform,
        style: StrokeStyle,
        bounds: Rect,
    },
}

/// An immutable decoded vector display list.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorImage {
    format: VectorFormat,
    size: Size,
    paths: Vec<Vec<PathSegment>>,
    paints: Vec<Paint>,
    commands: Vec<DrawCommand>,
}

impl VectorImage {
    /// Returns the encoded format.
    pub const fn format(&self) -> VectorFormat {
        self.format
    }
    /// Returns the intrinsic size in display units.
    pub const fn size(&self) -> Size {
        self.size
    }
    /// Returns a normalized path by identifier.
    pub fn path(&self, id: PathId) -> &[PathSegment] {
        &self.paths[id.0]
    }
    /// Returns a paint by identifier.
    pub fn paint(&self, id: PaintId) -> &Paint {
        &self.paints[id.0]
    }
    /// Returns drawing operations in paint order.
    pub fn commands(&self) -> &[DrawCommand] {
        &self.commands
    }
    /// Iterates over normalized paths and their identifiers.
    pub fn paths(&self) -> impl ExactSizeIterator<Item = (PathId, &[PathSegment])> {
        self.paths
            .iter()
            .enumerate()
            .map(|(index, path)| (PathId(index), path.as_slice()))
    }
    /// Iterates over resolved paints and their identifiers.
    pub fn paints(&self) -> impl ExactSizeIterator<Item = (PaintId, &Paint)> {
        self.paints
            .iter()
            .enumerate()
            .map(|(index, paint)| (PaintId(index), paint))
    }

    #[cfg(any(feature = "svg", feature = "tinyvg"))]
    pub(crate) const fn new(format: VectorFormat, size: Size) -> Self {
        Self {
            format,
            size,
            paths: Vec::new(),
            paints: Vec::new(),
            commands: Vec::new(),
        }
    }
    #[cfg(any(feature = "svg", feature = "tinyvg"))]
    pub(crate) fn add_path(&mut self, path: Vec<PathSegment>) -> PathId {
        let id = PathId(self.paths.len());
        self.paths.push(path);
        id
    }
    #[cfg(any(feature = "svg", feature = "tinyvg"))]
    pub(crate) fn add_paint(&mut self, paint: Paint) -> PaintId {
        let id = PaintId(self.paints.len());
        self.paints.push(paint);
        id
    }
    #[cfg(any(feature = "svg", feature = "tinyvg"))]
    pub(crate) fn push(&mut self, command: DrawCommand) {
        self.commands.push(command);
    }
    #[cfg(feature = "svg")]
    pub(crate) fn insert_command(&mut self, index: usize, command: DrawCommand) {
        self.commands.insert(index, command);
    }
    #[cfg(feature = "svg")]
    pub(crate) const fn command_len(&self) -> usize {
        self.commands.len()
    }
    #[cfg(feature = "svg")]
    pub(crate) const fn resource_lengths(&self) -> (usize, usize) {
        (self.paths.len(), self.paints.len())
    }
    #[cfg(feature = "svg")]
    pub(crate) fn truncate_resources(&mut self, paths: usize, paints: usize) {
        self.paths.truncate(paths);
        self.paints.truncate(paints);
    }
    #[cfg(feature = "svg")]
    pub(crate) fn drain_commands(&mut self, start: usize) -> Vec<DrawCommand> {
        self.commands.drain(start..).collect()
    }
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
    if svg::is_svg(_data) {
        return Some(VectorFormat::Svg);
    }
    None
}

/// Decodes a supported vector image to an immutable backend-neutral display list.
pub fn decode_vector(data: &[u8]) -> Result<VectorImage, VectorDecodeError> {
    if data.len() > 64 * 1024 * 1024 {
        return Err(VectorDecodeError::ResourceLimit);
    }
    match vector_format(data) {
        #[cfg(feature = "svg")]
        Some(VectorFormat::Svg) => svg::decode(data),
        #[cfg(feature = "tinyvg")]
        Some(VectorFormat::TinyVg) => decode_tinyvg(data),
        _ => Err(VectorDecodeError::InvalidMagic),
    }
}

#[cfg(feature = "tinyvg")]
pub(crate) fn decode_tinyvg(data: &[u8]) -> Result<VectorImage, VectorDecodeError> {
    let document = tinyvg::parse(data).map_err(|error| match error {
        tinyvg::Error::InvalidMagic => VectorDecodeError::InvalidMagic,
        tinyvg::Error::UnsupportedVersion(_) | tinyvg::Error::UnsupportedColorEncoding => {
            VectorDecodeError::UnsupportedFeature("TinyVG encoding")
        }
        tinyvg::Error::TooManyElements => VectorDecodeError::ResourceLimit,
        tinyvg::Error::UnexpectedEnd | tinyvg::Error::InvalidData(_) => {
            VectorDecodeError::InvalidData
        }
    })?;
    let mut image = VectorImage::new(VectorFormat::TinyVg, document.size);
    let command_capacity = document
        .commands
        .iter()
        .try_fold(0usize, |count, command| {
            count
                .checked_add(if matches!(command, tinyvg::Command::FillStroke { .. }) {
                    2
                } else {
                    1
                })
                .ok_or(VectorDecodeError::ResourceLimit)
        })?;
    let path_capacity = document
        .commands
        .iter()
        .try_fold(0usize, |count, command| {
            let paths = match command {
                tinyvg::Command::FillStroke {
                    path, line_width, ..
                } if has_variable_width(path, *line_width) => 2,
                _ => 1,
            };
            count
                .checked_add(paths)
                .ok_or(VectorDecodeError::ResourceLimit)
        })?;
    image
        .paths
        .try_reserve(path_capacity)
        .map_err(|_| VectorDecodeError::ResourceLimit)?;
    image
        .paints
        .try_reserve(command_capacity)
        .map_err(|_| VectorDecodeError::ResourceLimit)?;
    image
        .commands
        .try_reserve(command_capacity)
        .map_err(|_| VectorDecodeError::ResourceLimit)?;
    for command in document.commands {
        match command {
            tinyvg::Command::Fill { path, style } => {
                let paint = image.add_paint(convert_paint(style)?);
                let segments = convert_path(&path)?;
                let bounds = path_bounds(&segments).ok_or(VectorDecodeError::InvalidData)?;
                let path = image.add_path(segments);
                image.push(DrawCommand::Fill {
                    path,
                    paint,
                    transform: Transform::IDENTITY,
                    rule: FillRule::EvenOdd,
                    bounds,
                });
            }
            tinyvg::Command::Stroke {
                path,
                style,
                line_width,
            } => {
                let paint = image.add_paint(convert_paint(style)?);
                let variable_width = has_variable_width(&path, line_width);
                let segments = if variable_width {
                    TinyVgStrokeBuilder::build(&path, line_width)?
                } else {
                    convert_path(&path)?
                };
                let mut bounds = path_bounds(&segments).ok_or(VectorDecodeError::InvalidData)?;
                if !variable_width {
                    bounds = bounds.expand(line_width / 2.0);
                }
                let path = image.add_path(segments);
                image.push(if variable_width {
                    DrawCommand::Fill {
                        path,
                        paint,
                        transform: Transform::IDENTITY,
                        rule: FillRule::NonZero,
                        bounds,
                    }
                } else {
                    DrawCommand::Stroke {
                        path,
                        paint,
                        transform: Transform::IDENTITY,
                        style: tinyvg_stroke(line_width),
                        bounds,
                    }
                });
            }
            tinyvg::Command::FillStroke {
                path,
                fill_style,
                line_style,
                line_width,
            } => {
                let variable_width = has_variable_width(&path, line_width);
                let segments = convert_path(&path)?;
                let bounds = path_bounds(&segments).ok_or(VectorDecodeError::InvalidData)?;
                let fill_path = image.add_path(segments);
                let fill_paint = image.add_paint(convert_paint(fill_style)?);
                image.push(DrawCommand::Fill {
                    path: fill_path,
                    paint: fill_paint,
                    transform: Transform::IDENTITY,
                    rule: FillRule::EvenOdd,
                    bounds,
                });
                let line_paint = image.add_paint(convert_paint(line_style)?);
                if variable_width {
                    let stroke = TinyVgStrokeBuilder::build(&path, line_width)?;
                    let bounds = path_bounds(&stroke).ok_or(VectorDecodeError::InvalidData)?;
                    let stroke_path = image.add_path(stroke);
                    image.push(DrawCommand::Fill {
                        path: stroke_path,
                        paint: line_paint,
                        transform: Transform::IDENTITY,
                        rule: FillRule::NonZero,
                        bounds,
                    });
                } else {
                    image.push(DrawCommand::Stroke {
                        path: fill_path,
                        paint: line_paint,
                        transform: Transform::IDENTITY,
                        style: tinyvg_stroke(line_width),
                        bounds: bounds.expand(line_width / 2.0),
                    });
                }
            }
        }
    }
    let segment_count = image
        .paths()
        .try_fold(0usize, |count, (_, path)| count.checked_add(path.len()));
    if segment_count.is_none_or(|count| count > 1_000_000) || image.commands().len() > 1_000_000 {
        return Err(VectorDecodeError::ResourceLimit);
    }
    Ok(image)
}

#[cfg(feature = "tinyvg")]
fn has_variable_width(path: &tinyvg::Path, initial_width: f64) -> bool {
    path.subpaths.iter().any(|subpath| {
        let mut width = initial_width;
        subpath.nodes.iter().any(|node| {
            let next = node.line_width.unwrap_or(width);
            let changed = next != width;
            width = next;
            changed
        })
    })
}

#[cfg(feature = "tinyvg")]
struct TinyVgStrokeBuilder {
    output: Vec<PathSegment>,
    arc: Vec<PathSegment>,
}

#[cfg(feature = "tinyvg")]
impl TinyVgStrokeBuilder {
    const CURVE_STEPS: usize = 16;
    const ARC_STEPS: usize = 100;
    const MAX_SEGMENTS: usize = 1_000_000;
    const MIN_RADIUS: f64 = 0.35;

    fn build(
        path: &tinyvg::Path,
        initial_width: f64,
    ) -> Result<Vec<PathSegment>, VectorDecodeError> {
        let mut builder = Self {
            output: Vec::new(),
            arc: Vec::new(),
        };
        builder
            .arc
            .try_reserve_exact(4)
            .map_err(|_| VectorDecodeError::ResourceLimit)?;
        for subpath in &path.subpaths {
            builder.append_subpath(subpath, initial_width)?;
        }
        if builder.output.is_empty() {
            let start = path
                .subpaths
                .first()
                .ok_or(VectorDecodeError::InvalidData)?
                .start;
            builder.reserve(1)?;
            builder.output.push(PathSegment::MoveTo(start));
        }
        Ok(builder.output)
    }

    fn append_subpath(
        &mut self,
        subpath: &tinyvg::Subpath,
        initial_width: f64,
    ) -> Result<(), VectorDecodeError> {
        let mut current = subpath.start;
        let start = current;
        let mut width = initial_width;
        for node in &subpath.nodes {
            let next_width = node.line_width.unwrap_or(width);
            self.append_operation(current, start, width, next_width, &node.operation)?;
            current = operation_end(subpath.start, &node.operation);
            width = next_width;
        }
        Ok(())
    }

    fn append_operation(
        &mut self,
        from: Point,
        start: Point,
        width: f64,
        next_width: f64,
        operation: &tinyvg::PathOperation,
    ) -> Result<(), VectorDecodeError> {
        match *operation {
            tinyvg::PathOperation::LineTo(to) => self.append_capsule(from, to, width, next_width),
            tinyvg::PathOperation::Close => self.append_capsule(from, start, width, next_width),
            tinyvg::PathOperation::QuadraticTo { control, to } => {
                self.append_curve(from, width, next_width, Self::CURVE_STEPS, |t| {
                    let u = 1.0 - t;
                    Point {
                        x: u * u * from.x + 2.0 * u * t * control.x + t * t * to.x,
                        y: u * u * from.y + 2.0 * u * t * control.y + t * t * to.y,
                    }
                })
            }
            tinyvg::PathOperation::CubicTo {
                control_0,
                control_1,
                to,
            } => self.append_curve(from, width, next_width, Self::CURVE_STEPS, |t| {
                let u = 1.0 - t;
                let uu = u * u;
                let tt = t * t;
                Point {
                    x: uu * u * from.x
                        + 3.0 * uu * t * control_0.x
                        + 3.0 * u * tt * control_1.x
                        + tt * t * to.x,
                    y: uu * u * from.y
                        + 3.0 * uu * t * control_0.y
                        + 3.0 * u * tt * control_1.y
                        + tt * t * to.y,
                }
            }),
            tinyvg::PathOperation::ArcTo {
                radius_x,
                radius_y,
                rotation,
                large_arc,
                sweep,
                to,
            } => {
                self.arc.clear();
                append_arc(
                    &mut self.arc,
                    from,
                    to,
                    radius_x,
                    radius_y,
                    rotation,
                    large_arc,
                    !sweep,
                );
                let part_count = self.arc.len();
                let mut part_start = from;
                let mut completed_steps = 0;
                for index in 0..part_count {
                    let segment = self.arc[index];
                    match segment {
                        PathSegment::LineTo(to) => {
                            self.append_capsule(part_start, to, width, next_width)?;
                            part_start = to;
                        }
                        PathSegment::CubicTo {
                            control_0,
                            control_1,
                            to,
                        } => {
                            let steps = Self::ARC_STEPS / part_count
                                + usize::from(index < Self::ARC_STEPS % part_count);
                            let begin = completed_steps as f64 / Self::ARC_STEPS as f64;
                            completed_steps += steps;
                            let end = completed_steps as f64 / Self::ARC_STEPS as f64;
                            let begin_width = lerp(width, next_width, begin);
                            let end_width = lerp(width, next_width, end);
                            self.append_curve(part_start, begin_width, end_width, steps, |t| {
                                let u = 1.0 - t;
                                let uu = u * u;
                                let tt = t * t;
                                Point {
                                    x: uu * u * part_start.x
                                        + 3.0 * uu * t * control_0.x
                                        + 3.0 * u * tt * control_1.x
                                        + tt * t * to.x,
                                    y: uu * u * part_start.y
                                        + 3.0 * uu * t * control_0.y
                                        + 3.0 * u * tt * control_1.y
                                        + tt * t * to.y,
                                }
                            })?;
                            part_start = to;
                        }
                        _ => return Err(VectorDecodeError::InvalidData),
                    }
                }
                Ok(())
            }
        }
    }

    fn append_curve(
        &mut self,
        from: Point,
        width: f64,
        next_width: f64,
        steps: usize,
        point_at: impl Fn(f64) -> Point,
    ) -> Result<(), VectorDecodeError> {
        let mut previous = from;
        for index in 1..=steps {
            let t = index as f64 / steps as f64;
            let next = point_at(t);
            self.append_capsule(
                previous,
                next,
                lerp(width, next_width, (index - 1) as f64 / steps as f64),
                lerp(width, next_width, t),
            )?;
            previous = next;
        }
        Ok(())
    }

    fn append_capsule(
        &mut self,
        from: Point,
        to: Point,
        width: f64,
        next_width: f64,
    ) -> Result<(), VectorDecodeError> {
        let radius = (width / 2.0).max(Self::MIN_RADIUS);
        let next_radius = (next_width / 2.0).max(Self::MIN_RADIUS);
        let dx = to.x - from.x;
        let dy = to.y - from.y;
        let distance = dx.hypot(dy);
        if distance <= f64::EPSILON {
            return Ok(());
        }
        if distance <= (radius - next_radius).abs() {
            let (center, radius) = if radius >= next_radius {
                (from, radius)
            } else {
                (to, next_radius)
            };
            self.reserve(6)?;
            self.append_circle(center, radius);
            return Ok(());
        }

        let ux = dx / distance;
        let uy = dy / distance;
        let sin = (radius - next_radius) / distance;
        let cos = (1.0 - sin * sin).sqrt();
        let side_0 = Point {
            x: ux * sin - uy * cos,
            y: uy * sin + ux * cos,
        };
        let side_1 = Point {
            x: ux * sin + uy * cos,
            y: uy * sin - ux * cos,
        };
        let offset = |center: Point, side: Point, radius: f64| Point {
            x: center.x + side.x * radius,
            y: center.y + side.y * radius,
        };
        let from_0 = offset(from, side_0, radius);
        let from_1 = offset(from, side_1, radius);
        let to_1 = offset(to, side_1, next_radius);
        let to_0 = offset(to, side_0, next_radius);
        let first_cap = Self::cap_segments(from, from_0, from_1);
        let second_cap = Self::cap_segments(to, to_1, to_0);
        self.reserve(3 + first_cap + second_cap)?;
        self.output.push(PathSegment::MoveTo(from_0));
        self.append_cap(from, radius, from_0, from_1, first_cap);
        self.output.push(PathSegment::LineTo(to_1));
        self.append_cap(to, next_radius, to_1, to_0, second_cap);
        self.output.push(PathSegment::Close);
        Ok(())
    }

    fn cap_segments(center: Point, from: Point, to: Point) -> usize {
        let start = (from.y - center.y).atan2(from.x - center.x);
        let end = (to.y - center.y).atan2(to.x - center.x);
        let delta = (end - start).rem_euclid(std::f64::consts::TAU);
        (delta / std::f64::consts::FRAC_PI_2).ceil() as usize
    }

    fn append_cap(&mut self, center: Point, radius: f64, from: Point, to: Point, count: usize) {
        let start = (from.y - center.y).atan2(from.x - center.x);
        let end = (to.y - center.y).atan2(to.x - center.x);
        let delta = (end - start).rem_euclid(std::f64::consts::TAU);
        let step = delta / count as f64;
        for index in 0..count {
            let first = start + step * index as f64;
            let second = first + step;
            let alpha = 4.0 / 3.0 * (step / 4.0).tan();
            let (sin_0, cos_0) = first.sin_cos();
            let (sin_1, cos_1) = second.sin_cos();
            self.output.push(PathSegment::CubicTo {
                control_0: Point {
                    x: center.x + radius * (cos_0 - alpha * sin_0),
                    y: center.y + radius * (sin_0 + alpha * cos_0),
                },
                control_1: Point {
                    x: center.x + radius * (cos_1 + alpha * sin_1),
                    y: center.y + radius * (sin_1 - alpha * cos_1),
                },
                to: if index + 1 == count {
                    to
                } else {
                    Point {
                        x: center.x + radius * cos_1,
                        y: center.y + radius * sin_1,
                    }
                },
            });
        }
    }

    fn append_circle(&mut self, center: Point, radius: f64) {
        const KAPPA: f64 = 0.552_284_749_830_793_6;
        let control = radius * KAPPA;
        self.output.extend([
            PathSegment::MoveTo(Point {
                x: center.x + radius,
                y: center.y,
            }),
            PathSegment::CubicTo {
                control_0: Point {
                    x: center.x + radius,
                    y: center.y + control,
                },
                control_1: Point {
                    x: center.x + control,
                    y: center.y + radius,
                },
                to: Point {
                    x: center.x,
                    y: center.y + radius,
                },
            },
            PathSegment::CubicTo {
                control_0: Point {
                    x: center.x - control,
                    y: center.y + radius,
                },
                control_1: Point {
                    x: center.x - radius,
                    y: center.y + control,
                },
                to: Point {
                    x: center.x - radius,
                    y: center.y,
                },
            },
            PathSegment::CubicTo {
                control_0: Point {
                    x: center.x - radius,
                    y: center.y - control,
                },
                control_1: Point {
                    x: center.x - control,
                    y: center.y - radius,
                },
                to: Point {
                    x: center.x,
                    y: center.y - radius,
                },
            },
            PathSegment::CubicTo {
                control_0: Point {
                    x: center.x + control,
                    y: center.y - radius,
                },
                control_1: Point {
                    x: center.x + radius,
                    y: center.y - control,
                },
                to: Point {
                    x: center.x + radius,
                    y: center.y,
                },
            },
            PathSegment::Close,
        ]);
    }

    fn reserve(&mut self, additional: usize) -> Result<(), VectorDecodeError> {
        if self.output.len().saturating_add(additional) > Self::MAX_SEGMENTS {
            return Err(VectorDecodeError::ResourceLimit);
        }
        self.output
            .try_reserve(additional)
            .map_err(|_| VectorDecodeError::ResourceLimit)
    }
}

#[cfg(feature = "tinyvg")]
fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}

#[cfg(feature = "tinyvg")]
const fn tinyvg_stroke(width: f64) -> StrokeStyle {
    StrokeStyle {
        width,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        miter_limit: 4.0,
        dash_array: Vec::new(),
        dash_offset: 0.0,
    }
}

#[cfg(feature = "tinyvg")]
fn convert_paint(style: tinyvg::Style) -> Result<Paint, VectorDecodeError> {
    Ok(match style {
        tinyvg::Style::Solid(value) => Paint::Solid(value),
        tinyvg::Style::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => {
            let mut stops = Vec::new();
            stops
                .try_reserve_exact(2)
                .map_err(|_| VectorDecodeError::ResourceLimit)?;
            stops.extend([
                GradientStop {
                    offset: 0.0,
                    color: start_color,
                },
                GradientStop {
                    offset: 1.0,
                    color: end_color,
                },
            ]);
            Paint::LinearGradient {
                start,
                end,
                stops,
                spread: SpreadMethod::Pad,
                transform: Transform::IDENTITY,
            }
        }
        tinyvg::Style::RadialGradient {
            center,
            edge,
            center_color,
            edge_color,
        } => {
            let mut stops = Vec::new();
            stops
                .try_reserve_exact(2)
                .map_err(|_| VectorDecodeError::ResourceLimit)?;
            stops.extend([
                GradientStop {
                    offset: 0.0,
                    color: center_color,
                },
                GradientStop {
                    offset: 1.0,
                    color: edge_color,
                },
            ]);
            Paint::RadialGradient {
                center,
                focal: center,
                radius: (edge.x - center.x).hypot(edge.y - center.y),
                stops,
                spread: SpreadMethod::Pad,
                transform: Transform::IDENTITY,
            }
        }
    })
}

#[cfg(feature = "tinyvg")]
fn convert_path(path: &tinyvg::Path) -> Result<Vec<PathSegment>, VectorDecodeError> {
    let mut output = Vec::new();
    let capacity = path.subpaths.iter().try_fold(0usize, |count, subpath| {
        let nodes = subpath
            .nodes
            .len()
            .checked_mul(4)
            .ok_or(VectorDecodeError::ResourceLimit)?;
        count
            .checked_add(1)
            .and_then(|count| count.checked_add(nodes))
            .ok_or(VectorDecodeError::ResourceLimit)
    })?;
    output
        .try_reserve_exact(capacity)
        .map_err(|_| VectorDecodeError::ResourceLimit)?;
    for subpath in &path.subpaths {
        output.push(PathSegment::MoveTo(subpath.start));
        let mut current = subpath.start;
        for node in &subpath.nodes {
            append_tinyvg_operation(&mut output, current, subpath.start, &node.operation);
            current = operation_end(subpath.start, &node.operation);
        }
    }
    Ok(output)
}

#[cfg(feature = "tinyvg")]
const fn operation_end(start: Point, operation: &tinyvg::PathOperation) -> Point {
    match operation {
        tinyvg::PathOperation::LineTo(to)
        | tinyvg::PathOperation::CubicTo { to, .. }
        | tinyvg::PathOperation::QuadraticTo { to, .. }
        | tinyvg::PathOperation::ArcTo { to, .. } => *to,
        tinyvg::PathOperation::Close => start,
    }
}

#[cfg(feature = "tinyvg")]
fn append_tinyvg_operation(
    output: &mut Vec<PathSegment>,
    current: Point,
    start: Point,
    operation: &tinyvg::PathOperation,
) {
    match *operation {
        tinyvg::PathOperation::LineTo(to) => output.push(PathSegment::LineTo(to)),
        tinyvg::PathOperation::QuadraticTo { control, to } => {
            output.push(PathSegment::QuadraticTo { control, to })
        }
        tinyvg::PathOperation::CubicTo {
            control_0,
            control_1,
            to,
        } => output.push(PathSegment::CubicTo {
            control_0,
            control_1,
            to,
        }),
        tinyvg::PathOperation::ArcTo {
            radius_x,
            radius_y,
            rotation,
            large_arc,
            sweep,
            to,
        } => append_arc(
            output, current, to, radius_x, radius_y, rotation, large_arc, !sweep,
        ),
        tinyvg::PathOperation::Close => {
            let _ = start;
            output.push(PathSegment::Close);
        }
    }
}

#[cfg(any(feature = "svg", feature = "tinyvg"))]
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
