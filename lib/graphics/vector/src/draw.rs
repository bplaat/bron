/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Paints the shapes of the decoders with a canvas context.

use bcanvas::{
    CanvasGradient, CanvasRenderingContext2d, CanvasStyle, Color as CanvasColor,
    CompositeOperation, FillRule as CanvasFillRule, GradientSpread, LineCap as CanvasLineCap,
    LineJoin as CanvasLineJoin, OffscreenCanvas,
};

use crate::{
    BlendMode, Color, ColorSpace, FillRule, GradientStop, LineCap, LineJoin, Paint, PathSegment,
    Size, SpreadMethod, StrokeStyle, Transform,
};

/// How paints are drawn: as themselves, or as the white coverage of an alpha or luminance mask.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RenderMode {
    Normal,
    AlphaMask,
    LuminanceMask,
}

/// A recording canvas of a document size, without pixels for sizes beyond `f32`.
pub(crate) fn new_canvas(size: Size) -> OffscreenCanvas {
    let dimension = |value: f64| {
        let value = value.max(0.0) as f32;
        if value.is_finite() { value } else { 0.0 }
    };
    OffscreenCanvas::new_recording(dimension(size.width), dimension(size.height))
}

/// Sets the transform of the following paths, relative to the document.
pub(crate) fn set_transform(context: &mut CanvasRenderingContext2d<'_>, transform: Transform) {
    let Transform { a, b, c, d, e, f } = transform;
    context.set_transform(a as f32, b as f32, c as f32, d as f32, e as f32, f as f32);
}

/// Replaces the current path.
pub(crate) fn add_path(context: &mut CanvasRenderingContext2d<'_>, path: &[PathSegment]) {
    context.begin_path();
    for segment in path {
        match *segment {
            PathSegment::MoveTo(to) => context.move_to(to.x as f32, to.y as f32),
            PathSegment::LineTo(to) => context.line_to(to.x as f32, to.y as f32),
            PathSegment::QuadraticTo { control, to } => context.quadratic_curve_to(
                control.x as f32,
                control.y as f32,
                to.x as f32,
                to.y as f32,
            ),
            PathSegment::CubicTo {
                control_0,
                control_1,
                to,
            } => context.bezier_curve_to(
                control_0.x as f32,
                control_0.y as f32,
                control_1.x as f32,
                control_1.y as f32,
                to.x as f32,
                to.y as f32,
            ),
            PathSegment::Close => context.close_path(),
        }
    }
}

/// Fills the current path.
pub(crate) fn fill(
    context: &mut CanvasRenderingContext2d<'_>,
    paint: &Paint,
    rule: FillRule,
    mode: RenderMode,
) {
    set_mask_composite(context, mode);
    context.set_fill_style(style(paint, mode));
    context.fill_with(fill_rule(rule));
}

/// Strokes the current path.
pub(crate) fn stroke(
    context: &mut CanvasRenderingContext2d<'_>,
    paint: &Paint,
    stroke: &StrokeStyle,
    mode: RenderMode,
) {
    if stroke.width <= 0.0 {
        return;
    }
    set_mask_composite(context, mode);
    context.set_line_width(stroke.width as f32);
    context.set_line_cap(match stroke.line_cap {
        LineCap::Butt => CanvasLineCap::Butt,
        LineCap::Round => CanvasLineCap::Round,
        LineCap::Square => CanvasLineCap::Square,
    });
    context.set_line_join(match stroke.line_join {
        LineJoin::Miter => CanvasLineJoin::Miter,
        LineJoin::Round => CanvasLineJoin::Round,
        LineJoin::Bevel => CanvasLineJoin::Bevel,
    });
    context.set_miter_limit(stroke.miter_limit as f32);
    let dash: Vec<f32> = stroke
        .dash_array
        .iter()
        .map(|length| *length as f32)
        .collect();
    context.set_line_dash(&dash);
    context.set_line_dash_offset(stroke.dash_offset as f32);
    context.set_stroke_style(style(paint, mode));
    context.stroke();
}

/// Clips to a path in a transform.
pub(crate) fn clip(
    context: &mut CanvasRenderingContext2d<'_>,
    path: &[PathSegment],
    transform: Transform,
    rule: FillRule,
) {
    set_transform(context, transform);
    add_path(context, path);
    context.clip_with(fill_rule(rule));
}

// Luminance mask paints are their coverage, so later shapes replace the coverage below them,
// like a black shape hides a white one. Alpha mask paints composite normally, so overlapping
// alpha adds up and transparent shapes keep the coverage below them.
const fn set_mask_composite(context: &mut CanvasRenderingContext2d<'_>, mode: RenderMode) {
    if matches!(mode, RenderMode::LuminanceMask) {
        context.set_global_composite_operation(CompositeOperation::Copy);
    }
}

// The canvas style of a paint in the user space of the painted shape.
fn style(paint: &Paint, mode: RenderMode) -> CanvasStyle {
    let gradient = |mut gradient: CanvasGradient,
                    stops: &[GradientStop],
                    spread: SpreadMethod,
                    paint_transform: Transform| {
        for stop in stops {
            gradient.add_color_stop(stop.offset as f32, canvas_color(stop.color, mode));
        }
        gradient.set_spread(match spread {
            SpreadMethod::Pad => GradientSpread::Pad,
            SpreadMethod::Reflect => GradientSpread::Reflect,
            SpreadMethod::Repeat => GradientSpread::Repeat,
        });
        let Transform { a, b, c, d, e, f } = paint_transform;
        gradient.set_transform(a as f32, b as f32, c as f32, d as f32, e as f32, f as f32);
        CanvasStyle::Gradient(gradient)
    };
    match paint {
        Paint::Solid(color) => CanvasStyle::Color(canvas_color(*color, mode)),
        Paint::LinearGradient {
            start,
            end,
            stops,
            spread,
            transform: paint_transform,
        } => gradient(
            CanvasGradient::linear(start.x as f32, start.y as f32, end.x as f32, end.y as f32),
            stops,
            *spread,
            *paint_transform,
        ),
        Paint::RadialGradient {
            center,
            focal,
            radius,
            stops,
            spread,
            transform: paint_transform,
        } => gradient(
            CanvasGradient::radial(
                focal.x as f32,
                focal.y as f32,
                center.x as f32,
                center.y as f32,
                *radius as f32,
            ),
            stops,
            *spread,
            *paint_transform,
        ),
    }
}

fn canvas_color(color: Color, mode: RenderMode) -> CanvasColor {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let alpha = match mode {
        RenderMode::Normal => {
            let display = |value: f64| match color.color_space {
                ColorSpace::Srgb => value,
                ColorSpace::LinearSrgb => value.max(0.0).powf(1.0 / 2.2),
            };
            return CanvasColor::rgba(
                channel(display(color.red)),
                channel(display(color.green)),
                channel(display(color.blue)),
                channel(color.alpha),
            );
        }
        RenderMode::AlphaMask => color.alpha,
        RenderMode::LuminanceMask => {
            let linear = |value: f64| match color.color_space {
                ColorSpace::Srgb => value.max(0.0).powf(2.2),
                ColorSpace::LinearSrgb => value,
            };
            color.alpha
                * (linear(color.red) * 0.2126
                    + linear(color.green) * 0.7152
                    + linear(color.blue) * 0.0722)
        }
    };
    CanvasColor::rgba(255, 255, 255, channel(alpha))
}

const fn fill_rule(rule: FillRule) -> CanvasFillRule {
    match rule {
        FillRule::NonZero => CanvasFillRule::NonZero,
        FillRule::EvenOdd => CanvasFillRule::EvenOdd,
    }
}

pub(crate) const fn composite_operation(mode: BlendMode) -> CompositeOperation {
    match mode {
        BlendMode::Normal => CompositeOperation::SourceOver,
        BlendMode::Multiply => CompositeOperation::Multiply,
        BlendMode::Screen => CompositeOperation::Screen,
        BlendMode::Overlay => CompositeOperation::Overlay,
        BlendMode::Darken => CompositeOperation::Darken,
        BlendMode::Lighten => CompositeOperation::Lighten,
        BlendMode::ColorDodge => CompositeOperation::ColorDodge,
        BlendMode::ColorBurn => CompositeOperation::ColorBurn,
        BlendMode::HardLight => CompositeOperation::HardLight,
        BlendMode::SoftLight => CompositeOperation::SoftLight,
        BlendMode::Difference => CompositeOperation::Difference,
        BlendMode::Exclusion => CompositeOperation::Exclusion,
        BlendMode::Hue => CompositeOperation::Hue,
        BlendMode::Saturation => CompositeOperation::Saturation,
        BlendMode::Color => CompositeOperation::Color,
        BlendMode::Luminosity => CompositeOperation::Luminosity,
    }
}
