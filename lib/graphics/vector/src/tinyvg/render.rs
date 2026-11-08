/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Draws TinyVG documents with a canvas.

use bcanvas::{CanvasRenderingContext2d, OffscreenCanvas};

use super::{Command, Error, Path, PathOperation, Style, Subpath, parse};
use crate::draw::{RenderMode, add_path, fill, new_canvas, stroke};
use crate::{
    Color, FillRule, GradientStop, LineCap, LineJoin, Paint, PathSegment, Point, SpreadMethod,
    StrokeStyle, Transform, VectorDecodeError, append_arc,
};

const MAX_SEGMENTS: usize = 1_000_000;

/// Decodes a TinyVG document into a recording canvas.
pub(crate) fn decode(data: &[u8]) -> Result<OffscreenCanvas, VectorDecodeError> {
    let document = parse(data).map_err(|error| match error {
        Error::InvalidMagic => VectorDecodeError::InvalidMagic,
        Error::UnsupportedVersion(_) | Error::UnsupportedColorEncoding => {
            VectorDecodeError::UnsupportedFeature("TinyVG encoding")
        }
        Error::TooManyElements => VectorDecodeError::ResourceLimit,
        Error::UnexpectedEnd | Error::InvalidData(_) => VectorDecodeError::InvalidData,
    })?;
    if document.commands.len() > MAX_SEGMENTS {
        return Err(VectorDecodeError::ResourceLimit);
    }
    let mut canvas = new_canvas(document.size);
    let mut result = Ok(());
    canvas.draw(|context| result = draw(context, &document.commands));
    result.map(|()| canvas)
}

fn draw(
    context: &mut CanvasRenderingContext2d<'_>,
    commands: &[Command],
) -> Result<(), VectorDecodeError> {
    let mut segments = 0_usize;
    let mut add = |context: &mut CanvasRenderingContext2d<'_>, path: &[PathSegment]| {
        segments += path.len();
        if segments > MAX_SEGMENTS {
            return Err(VectorDecodeError::ResourceLimit);
        }
        add_path(context, path);
        Ok(())
    };
    for command in commands {
        match command {
            Command::Fill { path, style } => {
                add(context, &convert_path(path)?)?;
                fill(
                    context,
                    &convert_paint(*style),
                    FillRule::EvenOdd,
                    RenderMode::Normal,
                );
            }
            Command::Stroke {
                path,
                style,
                line_width,
            } => {
                let paint = convert_paint(*style);
                if has_variable_width(path, *line_width) {
                    add(context, &TinyVgStrokeBuilder::build(path, *line_width)?)?;
                    fill(context, &paint, FillRule::NonZero, RenderMode::Normal);
                } else {
                    add(context, &convert_path(path)?)?;
                    stroke(
                        context,
                        &paint,
                        &tinyvg_stroke(*line_width),
                        RenderMode::Normal,
                    );
                }
            }
            Command::FillStroke {
                path,
                fill_style,
                line_style,
                line_width,
            } => {
                add(context, &convert_path(path)?)?;
                fill(
                    context,
                    &convert_paint(*fill_style),
                    FillRule::EvenOdd,
                    RenderMode::Normal,
                );
                let paint = convert_paint(*line_style);
                if has_variable_width(path, *line_width) {
                    add(context, &TinyVgStrokeBuilder::build(path, *line_width)?)?;
                    fill(context, &paint, FillRule::NonZero, RenderMode::Normal);
                } else {
                    stroke(
                        context,
                        &paint,
                        &tinyvg_stroke(*line_width),
                        RenderMode::Normal,
                    );
                }
            }
        }
    }
    Ok(())
}

fn has_variable_width(path: &Path, initial_width: f64) -> bool {
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

struct TinyVgStrokeBuilder {
    output: Vec<PathSegment>,
    arc: Vec<PathSegment>,
}

impl TinyVgStrokeBuilder {
    const CURVE_STEPS: usize = 16;
    const ARC_STEPS: usize = 100;
    const MAX_SEGMENTS: usize = 1_000_000;
    const MIN_RADIUS: f64 = 0.35;

    fn build(path: &Path, initial_width: f64) -> Result<Vec<PathSegment>, VectorDecodeError> {
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
        subpath: &Subpath,
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
        operation: &PathOperation,
    ) -> Result<(), VectorDecodeError> {
        match *operation {
            PathOperation::LineTo(to) => self.append_capsule(from, to, width, next_width),
            PathOperation::Close => self.append_capsule(from, start, width, next_width),
            PathOperation::QuadraticTo { control, to } => {
                self.append_curve(from, width, next_width, Self::CURVE_STEPS, |t| {
                    let u = 1.0 - t;
                    Point {
                        x: u * u * from.x + 2.0 * u * t * control.x + t * t * to.x,
                        y: u * u * from.y + 2.0 * u * t * control.y + t * t * to.y,
                    }
                })
            }
            PathOperation::CubicTo {
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
            PathOperation::ArcTo {
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

fn lerp(start: f64, end: f64, amount: f64) -> f64 {
    start + (end - start) * amount
}

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

fn convert_paint(style: Style) -> Paint {
    let stops = |start: Color, end: Color| {
        vec![
            GradientStop {
                offset: 0.0,
                color: start,
            },
            GradientStop {
                offset: 1.0,
                color: end,
            },
        ]
    };
    match style {
        Style::Solid(color) => Paint::Solid(color),
        Style::LinearGradient {
            start,
            end,
            start_color,
            end_color,
        } => Paint::LinearGradient {
            start,
            end,
            stops: stops(start_color, end_color),
            spread: SpreadMethod::Pad,
            transform: Transform::IDENTITY,
        },
        Style::RadialGradient {
            center,
            edge,
            center_color,
            edge_color,
        } => Paint::RadialGradient {
            center,
            focal: center,
            radius: (edge.x - center.x).hypot(edge.y - center.y),
            stops: stops(center_color, edge_color),
            spread: SpreadMethod::Pad,
            transform: Transform::IDENTITY,
        },
    }
}

fn convert_path(path: &Path) -> Result<Vec<PathSegment>, VectorDecodeError> {
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

const fn operation_end(start: Point, operation: &PathOperation) -> Point {
    match operation {
        PathOperation::LineTo(to)
        | PathOperation::CubicTo { to, .. }
        | PathOperation::QuadraticTo { to, .. }
        | PathOperation::ArcTo { to, .. } => *to,
        PathOperation::Close => start,
    }
}

fn append_tinyvg_operation(
    output: &mut Vec<PathSegment>,
    current: Point,
    start: Point,
    operation: &PathOperation,
) {
    match *operation {
        PathOperation::LineTo(to) => output.push(PathSegment::LineTo(to)),
        PathOperation::QuadraticTo { control, to } => {
            output.push(PathSegment::QuadraticTo { control, to })
        }
        PathOperation::CubicTo {
            control_0,
            control_1,
            to,
        } => output.push(PathSegment::CubicTo {
            control_0,
            control_1,
            to,
        }),
        PathOperation::ArcTo {
            radius_x,
            radius_y,
            rotation,
            large_arc,
            sweep,
            to,
        } => append_arc(
            output, current, to, radius_x, radius_y, rotation, large_arc, !sweep,
        ),
        PathOperation::Close => {
            let _ = start;
            output.push(PathSegment::Close);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::header;
    use super::*;
    use crate::pixel;

    // The first path and initial line width of a document.
    fn stroke_path(data: &[u8]) -> (Path, f64) {
        match parse(data).expect("valid TinyVG").commands.remove(0) {
            Command::Stroke {
                path, line_width, ..
            }
            | Command::FillStroke {
                path, line_width, ..
            } => (path, line_width),
            Command::Fill { .. } => panic!("expected a stroked path"),
        }
    }

    fn outline(data: &[u8]) -> Vec<PathSegment> {
        let (path, line_width) = stroke_path(data);
        assert!(has_variable_width(&path, line_width));
        TinyVgStrokeBuilder::build(&path, line_width).expect("outline")
    }

    #[test]
    fn outlined_polygons_are_filled_then_stroked() {
        let mut data = header(10, 10, &[[255, 0, 0, 255], [0, 0, 0, 255]]);
        data.extend_from_slice(&[8, 2, 0, 1, 1, 0, 0, 9, 0, 0, 9, 0]);
        let mut image = crate::decode_vector(&data).expect("outlined triangle");
        assert_eq!(pixel(&mut image, 2, 2), [255, 0, 0, 255]);
        let edge = pixel(&mut image, 4, 4);
        assert!(edge[0] < 16 && edge[3] > 240, "{edge:?}");
        assert_eq!(pixel(&mut image, 7, 7)[3], 0);
    }

    #[test]
    fn converts_variable_width_strokes_to_tapered_fills() {
        for (colors, command) in [
            (
                &[[0, 0, 0, 255]][..],
                &[7, 0, 0, 1, 0, 0, 0, 0x10, 2, 9, 0, 0][..],
            ),
            (
                &[[255, 0, 0, 255], [0, 0, 0, 255]][..],
                &[10, 0, 0, 1, 1, 0, 0, 0, 0x10, 2, 9, 0, 0][..],
            ),
        ] {
            let data = [header(10, 10, colors), command.to_vec()].concat();
            let outline = outline(&data);
            assert!((7..=9).contains(&outline.len()));
            let points = outline.iter().flat_map(|segment| match *segment {
                PathSegment::MoveTo(point) | PathSegment::LineTo(point) => vec![point],
                PathSegment::QuadraticTo { control, to } => vec![control, to],
                PathSegment::CubicTo {
                    control_0,
                    control_1,
                    to,
                } => vec![control_0, control_1, to],
                PathSegment::Close => Vec::new(),
            });
            let (top, bottom) = points.fold((0.0_f64, 0.0_f64), |(top, bottom), point| {
                (top.min(point.y), bottom.max(point.y))
            });
            assert!(top <= -1.0 && bottom >= 1.0);
            // The black line widens along its length.
            let mut image = crate::decode_vector(&data).expect("variable-width path");
            let (start, end) = (pixel(&mut image, 1, 0), pixel(&mut image, 8, 0));
            assert!(start[0] == 0 && end[0] == 0 && start[3] < end[3]);
        }
    }

    #[test]
    fn variable_width_curves_follow_reference_subdivision_counts() {
        let mut quadratic = header(20, 20, &[[0, 0, 0, 255]]);
        quadratic.extend_from_slice(&[7, 0, 0, 2, 0, 1, 10, 0x17, 6, 10, 1, 19, 10, 0]);
        assert!((112..=144).contains(&outline(&quadratic).len()));

        let mut arc = header(20, 20, &[[0, 0, 0, 255]]);
        arc.extend_from_slice(&[7, 0, 0, 2, 0, 1, 10, 0x14, 6, 0, 9, 19, 10, 0]);
        assert!((700..=900).contains(&outline(&arc).len()));
    }
}
