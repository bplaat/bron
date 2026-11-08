/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::values::number;
use super::xml::Element;
use crate::{MaskType, PathSegment, Point, Rect, Transform, VectorDecodeError};

pub(super) struct PathMetrics<'a> {
    path: &'a [PathSegment],
}

impl<'a> PathMetrics<'a> {
    pub(super) const fn new(path: &'a [PathSegment]) -> Self {
        Self { path }
    }

    pub(super) fn length(&self) -> f64 {
        let curves = self
            .path
            .iter()
            .filter(|segment| {
                matches!(
                    segment,
                    PathSegment::QuadraticTo { .. } | PathSegment::CubicTo { .. }
                )
            })
            .count();
        let curve_steps = (65_536 / curves.max(1)).clamp(1, 32);
        let mut length = 0.0;
        let mut current = Point { x: 0.0, y: 0.0 };
        let mut start = current;
        for segment in self.path {
            match *segment {
                PathSegment::MoveTo(point) => {
                    current = point;
                    start = point;
                }
                PathSegment::LineTo(point) => {
                    length += distance(current, point);
                    current = point;
                }
                PathSegment::QuadraticTo { control, to } => {
                    length += curve_length(current, curve_steps, |t| {
                        let u = 1.0 - t;
                        Point {
                            x: u * u * current.x + 2.0 * u * t * control.x + t * t * to.x,
                            y: u * u * current.y + 2.0 * u * t * control.y + t * t * to.y,
                        }
                    });
                    current = to;
                }
                PathSegment::CubicTo {
                    control_0,
                    control_1,
                    to,
                } => {
                    length += curve_length(current, curve_steps, |t| {
                        let u = 1.0 - t;
                        Point {
                            x: u * u * u * current.x
                                + 3.0 * u * u * t * control_0.x
                                + 3.0 * u * t * t * control_1.x
                                + t * t * t * to.x,
                            y: u * u * u * current.y
                                + 3.0 * u * u * t * control_0.y
                                + 3.0 * u * t * t * control_1.y
                                + t * t * t * to.y,
                        }
                    });
                    current = to;
                }
                PathSegment::Close => {
                    length += distance(current, start);
                    current = start;
                }
            }
        }
        length
    }
}

fn curve_length(mut previous: Point, steps: usize, point: impl Fn(f64) -> Point) -> f64 {
    let mut length = 0.0;
    for step in 1..=steps {
        let current = point(step as f64 / steps as f64);
        length += distance(previous, current);
        previous = current;
    }
    length
}

fn distance(a: Point, b: Point) -> f64 {
    (b.x - a.x).hypot(b.y - a.y)
}

#[derive(Clone, Copy)]
pub(super) struct MarkerPlacement {
    pub(super) point: Point,
    pub(super) angle: f64,
    pub(super) is_start: bool,
}

impl PathMetrics<'_> {
    pub(super) fn marker_positions(
        &self,
    ) -> (
        Vec<MarkerPlacement>,
        Vec<MarkerPlacement>,
        Vec<MarkerPlacement>,
    ) {
        let mut starts = Vec::new();
        let mut mids = Vec::new();
        let mut ends = Vec::new();
        let mut vertices = Vec::<(Point, Option<Point>, Option<Point>)>::new();
        let flush = |vertices: &mut Vec<(Point, Option<Point>, Option<Point>)>,
                     starts: &mut Vec<MarkerPlacement>,
                     mids: &mut Vec<MarkerPlacement>,
                     ends: &mut Vec<MarkerPlacement>| {
            if vertices.len() < 2 {
                vertices.clear();
                return;
            }
            let angle = |incoming: Option<Point>, outgoing: Option<Point>| {
                let unit = |point: Point| {
                    let length = point.x.hypot(point.y);
                    (length > 0.0).then(|| Point {
                        x: point.x / length,
                        y: point.y / length,
                    })
                };
                match (incoming.and_then(unit), outgoing.and_then(unit)) {
                    (Some(a), Some(b)) if (a.x + b.x).hypot(a.y + b.y) > 1e-12 => {
                        (a.y + b.y).atan2(a.x + b.x)
                    }
                    (Some(a), _) => a.y.atan2(a.x),
                    (_, Some(b)) => b.y.atan2(b.x),
                    _ => 0.0,
                }
            };
            let first = vertices[0];
            let last = *vertices.last().expect("at least two marker vertices");
            starts.push(MarkerPlacement {
                point: first.0,
                angle: angle(None, first.2),
                is_start: true,
            });
            for vertex in &vertices[1..vertices.len() - 1] {
                mids.push(MarkerPlacement {
                    point: vertex.0,
                    angle: angle(vertex.1, vertex.2),
                    is_start: false,
                });
            }
            ends.push(MarkerPlacement {
                point: last.0,
                angle: angle(last.1, None),
                is_start: false,
            });
            vertices.clear();
        };
        let mut current = Point { x: 0.0, y: 0.0 };
        let mut start = current;
        for segment in self.path {
            match *segment {
                PathSegment::MoveTo(point) => {
                    flush(&mut vertices, &mut starts, &mut mids, &mut ends);
                    current = point;
                    start = point;
                    vertices.push((point, None, None));
                }
                PathSegment::LineTo(to) => {
                    append_marker_vertex(&mut vertices, current, to, current, to);
                    current = to;
                }
                PathSegment::QuadraticTo { control, to } => {
                    append_marker_vertex(&mut vertices, current, control, control, to);
                    current = to;
                }
                PathSegment::CubicTo {
                    control_0,
                    control_1,
                    to,
                } => {
                    append_marker_vertex(&mut vertices, current, control_0, control_1, to);
                    current = to;
                }
                PathSegment::Close => {
                    append_marker_vertex(&mut vertices, current, start, current, start);
                    current = start;
                }
            }
        }
        flush(&mut vertices, &mut starts, &mut mids, &mut ends);
        (starts, mids, ends)
    }
}

fn append_marker_vertex(
    vertices: &mut Vec<(Point, Option<Point>, Option<Point>)>,
    from: Point,
    first_control: Point,
    last_control: Point,
    to: Point,
) {
    let outgoing = Point {
        x: first_control.x - from.x,
        y: first_control.y - from.y,
    };
    if let Some(vertex) = vertices.last_mut() {
        vertex.2 = Some(outgoing);
    }
    vertices.push((
        to,
        Some(Point {
            x: to.x - last_control.x,
            y: to.y - last_control.y,
        }),
        None,
    ));
}

pub(super) fn transform_bounds(bounds: Rect, transform: Transform) -> Rect {
    let points = [
        transform.map(Point {
            x: bounds.x,
            y: bounds.y,
        }),
        transform.map(Point {
            x: bounds.x + bounds.width,
            y: bounds.y,
        }),
        transform.map(Point {
            x: bounds.x,
            y: bounds.y + bounds.height,
        }),
        transform.map(Point {
            x: bounds.x + bounds.width,
            y: bounds.y + bounds.height,
        }),
    ];
    let mut output = Rect::from_points(points[0], points[0]);
    for point in &points[1..] {
        output.include(*point);
    }
    output
}

pub(super) fn inverse_transform(transform: Transform) -> Option<Transform> {
    let determinant = transform.a * transform.d - transform.b * transform.c;
    if determinant.abs() <= f64::EPSILON {
        return None;
    }
    Some(Transform {
        a: transform.d / determinant,
        b: -transform.b / determinant,
        c: -transform.c / determinant,
        d: transform.a / determinant,
        e: (transform.c * transform.f - transform.d * transform.e) / determinant,
        f: (transform.b * transform.e - transform.a * transform.f) / determinant,
    })
}

// The bounds in object space of drawn paths, as their local bounds and transforms.
pub(super) fn object_bounds(
    drawn: &[(Rect, Transform)],
    document_to_object: Transform,
) -> Option<Rect> {
    drawn
        .iter()
        .map(|&(bounds, transform)| transform_bounds(bounds, transform.then(document_to_object)))
        .reduce(union_rect)
}

pub(super) fn union_rect(a: Rect, b: Rect) -> Rect {
    let max_x = (a.x + a.width).max(b.x + b.width);
    let max_y = (a.y + a.height).max(b.y + b.height);
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect {
        x,
        y,
        width: max_x - x,
        height: max_y - y,
    }
}

pub(super) fn object_bbox_length(value: &str) -> Result<f64, VectorDecodeError> {
    if let Some(value) = value.trim().strip_suffix('%') {
        Ok(number(value)? / 100.0)
    } else {
        number(value)
    }
}

pub(super) fn parse_mask_type(element: &Element<'_>) -> Result<MaskType, VectorDecodeError> {
    let mut mask_type = match element.attr("mask-type").unwrap_or("luminance") {
        "alpha" => MaskType::Alpha,
        "luminance" => MaskType::Luminance,
        _ => return Err(VectorDecodeError::InvalidData),
    };
    if let Some(style) = element.attr("style") {
        for declaration in style.split(';') {
            let Some((name, value)) = declaration.split_once(':') else {
                continue;
            };
            if name.trim() == "mask-type" {
                match value.trim() {
                    "alpha" => mask_type = MaskType::Alpha,
                    "luminance" => mask_type = MaskType::Luminance,
                    _ => {}
                }
            }
        }
    }
    Ok(mask_type)
}

pub(super) fn transform_segment(segment: &mut PathSegment, transform: Transform) {
    match segment {
        PathSegment::MoveTo(point) | PathSegment::LineTo(point) => {
            *point = transform.map(*point);
        }
        PathSegment::QuadraticTo { control, to } => {
            *control = transform.map(*control);
            *to = transform.map(*to);
        }
        PathSegment::CubicTo {
            control_0,
            control_1,
            to,
        } => {
            *control_0 = transform.map(*control_0);
            *control_1 = transform.map(*control_1);
            *to = transform.map(*to);
        }
        PathSegment::Close => {}
    }
}
