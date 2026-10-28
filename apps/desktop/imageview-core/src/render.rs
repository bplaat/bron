/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::RefCell;

use bcanvas::{
    CanvasGradient, CanvasImage, CanvasRenderingContext2d, CanvasStyle, Color as CanvasColor,
    CompositeOperation, FillRule as CanvasFillRule, GradientSpread, LineCap as CanvasLineCap,
    LineJoin as CanvasLineJoin, OffscreenCanvas,
};
use vector::{
    BlendMode, Color, ColorSpace, DrawCommand, FillRule, LineCap, LineJoin, MaskType, Paint,
    PathSegment, SpreadMethod, StrokeStyle, Transform, VectorImage,
};

use crate::Media;

/// Draws media with a canvas, keeping the canvas images of its frames.
pub struct MediaRenderer {
    media: Media,
    images: RefCell<Vec<Option<CanvasImage>>>,
}

impl MediaRenderer {
    /// Creates a renderer that owns the media.
    pub const fn new(media: Media) -> Self {
        Self {
            media,
            images: RefCell::new(Vec::new()),
        }
    }

    /// Returns the media.
    pub const fn media(&self) -> &Media {
        &self.media
    }

    /// Draws a frame of the media scaled into a rectangle.
    pub fn draw(
        &self,
        context: &mut CanvasRenderingContext2d<'_>,
        frame: usize,
        rect: (f32, f32, f32, f32),
    ) {
        let (x, y, width, height) = rect;
        match &self.media {
            Media::Raster(raster) => {
                let Some(pixels) = raster.frames.get(frame).map(|frame| &frame.pixels) else {
                    return;
                };
                let mut images = self.images.borrow_mut();
                images.resize(raster.frames.len(), None);
                let image = images[frame].get_or_insert_with(|| {
                    CanvasImage::from_premultiplied_bgra(
                        raster.width,
                        raster.height,
                        pixels.clone(),
                    )
                });
                context.draw_image(image, x, y, width, height);
            }
            Media::Vector(document) => draw_vector(context, document, rect),
        }
    }

    /// Draws the media scaled to fit inside a rectangle, centered and keeping its shape.
    pub fn draw_fitted(
        &self,
        context: &mut CanvasRenderingContext2d<'_>,
        frame: usize,
        rect: (f32, f32, f32, f32),
    ) {
        let (width, height) = self.media.size();
        if width <= 0.0 || height <= 0.0 {
            return;
        }
        let scale = (f64::from(rect.2) / width).min(f64::from(rect.3) / height);
        let (fitted_width, fitted_height) = ((width * scale) as f32, (height * scale) as f32);
        self.draw(
            context,
            frame,
            (
                rect.0 + (rect.2 - fitted_width) / 2.0,
                rect.1 + (rect.3 - fitted_height) / 2.0,
                fitted_width,
                fitted_height,
            ),
        );
    }
}

/// Renders media fitted within a square of `size` pixels, without enlarging it, as a bitmap of
/// straight-alpha RGBA pixels.
pub fn render_thumbnail(media: Media, size: u32) -> Option<image::Bitmap> {
    let (width, height) = media.size();
    if width <= 0.0 || height <= 0.0 || size == 0 {
        return None;
    }
    let scale = (f64::from(size) / width.max(height)).min(1.0);
    let (width, height) = (
        (width * scale).round().max(1.0) as u32,
        (height * scale).round().max(1.0) as u32,
    );
    let renderer = MediaRenderer::new(media);
    let mut canvas = OffscreenCanvas::new(width, height);
    canvas.draw(|context| {
        renderer.draw(context, 0, (0.0, 0.0, width as f32, height as f32));
    });
    image::Bitmap::new(width, height, canvas.pixels().to_vec()).ok()
}

// The width and height of a single checkerboard tile.
const TILE: f32 = 12.0;

/// Draws the checkerboard shown behind transparent media, in light or dark colors. Its tiles
/// start at the center, so they do not move while the area is resized.
pub fn draw_checkerboard(
    context: &mut CanvasRenderingContext2d<'_>,
    width: f32,
    height: f32,
    dark: bool,
) {
    let (base, alternate) = if dark {
        (CanvasColor::rgb(41, 41, 43), CanvasColor::rgb(59, 59, 64))
    } else {
        (
            CanvasColor::rgb(240, 240, 240),
            CanvasColor::rgb(219, 219, 219),
        )
    };
    context.set_fill_style(base);
    context.fill_rect(0.0, 0.0, width, height);
    context.set_fill_style(alternate);
    let (half_columns, half_rows) = checker_grid(width, height);
    for row in -half_rows..half_rows {
        for column in -half_columns..half_columns {
            if tile_is_filled(row, column) {
                context.fill_rect(
                    width / 2.0 + column as f32 * TILE,
                    height / 2.0 + row as f32 * TILE,
                    TILE,
                    TILE,
                );
            }
        }
    }
}

// Returns the number of tiles on each side of the center that cover the area.
fn checker_grid(width: f32, height: f32) -> (i32, i32) {
    (
        (width / (2.0 * TILE)).ceil().max(0.0) as i32,
        (height / (2.0 * TILE)).ceil().max(0.0) as i32,
    )
}

// Returns whether the tile at the given center-relative indices gets the alternate color.
const fn tile_is_filled(row: i32, column: i32) -> bool {
    (row + column).rem_euclid(2) == 0
}

// How paints are drawn: as themselves, or as the white coverage of an alpha or luminance mask.
#[derive(Clone, Copy)]
enum RenderMode {
    Normal,
    AlphaMask,
    LuminanceMask,
}

fn draw_vector(
    context: &mut CanvasRenderingContext2d<'_>,
    document: &VectorImage,
    (x, y, width, height): (f32, f32, f32, f32),
) {
    let size = document.size();
    if size.width <= 0.0 || size.height <= 0.0 {
        return;
    }
    context.save();
    context.begin_path();
    context.rect(x, y, width, height);
    context.clip();
    context.translate(x, y);
    context.scale(
        (f64::from(width) / size.width) as f32,
        (f64::from(height) / size.height) as f32,
    );
    let mut index = 0;
    draw_commands(
        context,
        document,
        document.commands(),
        &mut index,
        RenderMode::Normal,
    );
    context.restore();
}

// Draws commands until the end of the current scope.
fn draw_commands(
    context: &mut CanvasRenderingContext2d<'_>,
    document: &VectorImage,
    commands: &[DrawCommand],
    index: &mut usize,
    mode: RenderMode,
) {
    while let Some(command) = commands.get(*index) {
        *index += 1;
        match command {
            DrawCommand::PushScope {
                opacity,
                blend_mode,
                isolated,
                clips,
                mask,
                ..
            } => {
                context.save();
                for clip in clips {
                    add_path(context, document.path(clip.path), clip.transform);
                    context.clip_with(fill_rule(clip.rule));
                }
                if let Some(mask) = mask {
                    add_path(
                        context,
                        document.path(mask.region.path),
                        mask.region.transform,
                    );
                    context.clip();
                }
                context.begin_path();
                context.set_global_alpha(*opacity as f32);
                context.set_global_composite_operation(composite_operation(*blend_mode));
                if let Some(mask) = mask {
                    context.begin_mask_layer();
                    let mask_mode = match mask.mask_type {
                        MaskType::Alpha => RenderMode::AlphaMask,
                        MaskType::Luminance => RenderMode::LuminanceMask,
                    };
                    draw_commands(context, document, &mask.commands, &mut 0, mask_mode);
                    context.end_mask();
                    draw_commands(context, document, commands, index, mode);
                    context.end_layer();
                } else if *opacity < 1.0 || *isolated {
                    context.begin_layer();
                    draw_commands(context, document, commands, index, mode);
                    context.end_layer();
                } else {
                    draw_commands(context, document, commands, index, mode);
                }
                context.restore();
            }
            DrawCommand::PopScope => return,
            DrawCommand::Fill {
                path,
                paint,
                transform,
                rule,
                ..
            } => {
                context.save();
                set_mask_composite(context, mode);
                add_path(context, document.path(*path), *transform);
                context.set_fill_style(style(document.paint(*paint), *transform, mode));
                context.fill_with(fill_rule(*rule));
                context.restore();
            }
            DrawCommand::Stroke {
                path,
                paint,
                transform,
                style: stroke,
                ..
            } if stroke.width > 0.0 => {
                context.save();
                set_mask_composite(context, mode);
                apply_transform(context, *transform);
                add_path(context, document.path(*path), Transform::IDENTITY);
                set_stroke(context, stroke);
                context.set_stroke_style(style(document.paint(*paint), Transform::IDENTITY, mode));
                context.stroke();
                context.restore();
            }
            DrawCommand::Stroke { .. } => {}
        }
    }
}

// Replaces the current path, transforming it while it is added.
fn add_path(
    context: &mut CanvasRenderingContext2d<'_>,
    path: &[PathSegment],
    transform: Transform,
) {
    context.begin_path();
    let point = |point: vector::Point| {
        let point = transform.map(point);
        (point.x as f32, point.y as f32)
    };
    for segment in path {
        match *segment {
            PathSegment::MoveTo(to) => {
                let (x, y) = point(to);
                context.move_to(x, y);
            }
            PathSegment::LineTo(to) => {
                let (x, y) = point(to);
                context.line_to(x, y);
            }
            PathSegment::QuadraticTo { control, to } => {
                let ((cx, cy), (x, y)) = (point(control), point(to));
                context.quadratic_curve_to(cx, cy, x, y);
            }
            PathSegment::CubicTo {
                control_0,
                control_1,
                to,
            } => {
                let ((ax, ay), (bx, by), (x, y)) = (point(control_0), point(control_1), point(to));
                context.bezier_curve_to(ax, ay, bx, by, x, y);
            }
            PathSegment::Close => context.close_path(),
        }
    }
}

// Luminance mask paints are their coverage, so later shapes replace the coverage below them,
// like a black shape hides a white one. Alpha mask paints composite normally, so overlapping
// alpha adds up and transparent shapes keep the coverage below them.
const fn set_mask_composite(context: &mut CanvasRenderingContext2d<'_>, mode: RenderMode) {
    if matches!(mode, RenderMode::LuminanceMask) {
        context.set_global_composite_operation(CompositeOperation::Copy);
    }
}

fn apply_transform(context: &mut CanvasRenderingContext2d<'_>, transform: Transform) {
    let Transform { a, b, c, d, e, f } = transform;
    context.transform(a as f32, b as f32, c as f32, d as f32, e as f32, f as f32);
}

fn set_stroke(context: &mut CanvasRenderingContext2d<'_>, stroke: &StrokeStyle) {
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
}

// The canvas style of a paint, whose gradient space is `transform` after the paint transform.
fn style(paint: &Paint, transform: Transform, mode: RenderMode) -> CanvasStyle {
    let gradient = |mut gradient: CanvasGradient,
                    stops: &[vector::GradientStop],
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
        let Transform { a, b, c, d, e, f } = paint_transform.then(transform);
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

const fn composite_operation(mode: BlendMode) -> CompositeOperation {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::*;
    use crate::{Frame, LoopCount, Raster, decode_media};

    const VECTOR_TESTS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../lib/graphics/vector/tests"
    );

    // Renders media fitted into a canvas, as premultiplied RGBA8 pixels.
    fn render(media: Media, width: u32, height: u32, white_background: bool) -> Vec<u8> {
        let renderer = MediaRenderer::new(media);
        let mut canvas = OffscreenCanvas::new(width, height);
        canvas.draw(|context| {
            if white_background {
                context.set_fill_style(CanvasColor::rgb(255, 255, 255));
                context.fill_rect(0.0, 0.0, width as f32, height as f32);
            }
            renderer.draw_fitted(context, 0, (0.0, 0.0, width as f32, height as f32));
        });
        premultiplied_rgba(canvas.pixels())
    }

    fn render_svg(svg: &[u8], width: u32, height: u32) -> Vec<u8> {
        render(decode_media(svg).expect("SVG"), width, height, false)
    }

    fn premultiplied_rgba(pixels: &[u8]) -> Vec<u8> {
        let mut output = pixels.to_vec();
        for pixel in output.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        output
    }

    fn fixture_files(directory: &Path, extension: &str, output: &mut Vec<PathBuf>) {
        let mut entries = fs::read_dir(directory)
            .expect("read fixture directory")
            .map(|entry| entry.expect("read fixture entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                fixture_files(&path, extension, output);
            } else if path
                .extension()
                .is_some_and(|candidate| candidate == extension)
            {
                output.push(path);
            }
        }
    }

    fn attribute<'a>(tag: &'a str, wanted: &str) -> Option<&'a str> {
        let bytes = tag.as_bytes();
        let mut offset = 0;
        while let Some(found) = tag[offset..].find(wanted) {
            let start = offset + found;
            let before_is_boundary =
                start == 0 || bytes[start - 1].is_ascii_whitespace() || bytes[start - 1] == b'<';
            let mut cursor = start + wanted.len();
            if !before_is_boundary
                || (cursor < bytes.len()
                    && !bytes[cursor].is_ascii_whitespace()
                    && bytes[cursor] != b'=')
            {
                offset = cursor;
                continue;
            }
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if bytes.get(cursor) != Some(&b'=') {
                offset = cursor;
                continue;
            }
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            let quote = *bytes.get(cursor)?;
            if quote != b'\'' && quote != b'"' {
                return None;
            }
            cursor += 1;
            let end = bytes[cursor..].iter().position(|&byte| byte == quote)? + cursor;
            return Some(&tag[cursor..end]);
        }
        None
    }

    fn tag_with_attribute<'a>(source: &'a str, name: &str, value: &str) -> Option<&'a str> {
        source
            .split('<')
            .filter_map(|tail| tail.split_once('>').map(|pair| pair.0))
            .find(|tag| attribute(tag, name) == Some(value))
    }

    fn fuzzy_limits(source: &str) -> (u8, usize) {
        let Some(tag) = tag_with_attribute(source, "name", "fuzzy") else {
            return (0, 0);
        };
        let content = attribute(tag, "content").expect("WPT fuzzy content");
        let numbers = content
            .split(|character: char| !character.is_ascii_digit())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse::<usize>().expect("WPT fuzzy number"))
            .collect::<Vec<_>>();
        assert_eq!(numbers.len(), 4, "WPT fuzzy limits");
        (
            u8::try_from(numbers[1]).expect("WPT channel difference"),
            numbers[3],
        )
    }

    // The largest channel difference of each pixel.
    fn differences<'a>(actual: &'a [u8], expected: &'a [u8]) -> impl Iterator<Item = u8> + 'a {
        assert_eq!(actual.len(), expected.len());
        actual
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .map(|(actual, expected)| {
                actual
                    .iter()
                    .zip(expected)
                    .map(|(&actual, &expected)| actual.abs_diff(expected))
                    .max()
                    .expect("RGBA pixel")
            })
    }

    #[test]
    fn renders_wpt_svg_reference_pairs() {
        let root = Path::new(VECTOR_TESTS);
        let image_root = root.join("images/svg");
        let reference_root = root
            .join("reference/svg")
            .canonicalize()
            .expect("canonical SVG reference directory");
        let mut files = Vec::new();
        fixture_files(&image_root, "svg", &mut files);
        assert_eq!(files.len(), 93, "pinned WPT SVG test count changed");
        let mut failures = Vec::new();
        for path in files {
            let relative = path.strip_prefix(&image_root).expect("relative SVG test");
            let data = fs::read(&path).expect("read WPT SVG test");
            let source = std::str::from_utf8(&data).expect("WPT SVG is UTF-8");
            let link = tag_with_attribute(source, "rel", "match").expect("WPT match link");
            let reference_path = path
                .parent()
                .expect("SVG test directory")
                .join(attribute(link, "href").expect("WPT match href"))
                .canonicalize()
                .expect("resolve WPT SVG reference");
            assert!(reference_path.starts_with(&reference_root));
            let reference_data = fs::read(reference_path).expect("read WPT SVG reference");
            let document = decode_media(&data)
                .unwrap_or_else(|error| panic!("{}: {error}", relative.display()));
            let reference = decode_media(&reference_data)
                .unwrap_or_else(|error| panic!("{} reference: {error}", relative.display()));
            assert_eq!(document.size(), reference.size(), "{}", relative.display());
            let (width, height) = document.size();
            let (width, height) = (width.ceil() as u32, height.ceil() as u32);
            let actual = render(document, width, height, true);
            let expected = render(reference, width, height, true);
            let (allowed_difference, allowed_pixels) = fuzzy_limits(source);
            let (maximum, count) =
                differences(&actual, &expected).fold((0, 0), |(maximum, count), difference| {
                    (
                        maximum.max(difference),
                        count + usize::from(difference != 0),
                    )
                });
            if maximum > allowed_difference || count > allowed_pixels {
                failures.push(format!(
                    "{}: max difference {maximum} (allowed {allowed_difference}), different pixels {count} (allowed {allowed_pixels})",
                    relative.display()
                ));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn renders_tinyvg_reference_pngs() {
        let root = Path::new(VECTOR_TESTS);
        let image_root = root.join("images/tinyvg");
        let reference_root = root.join("reference/tinyvg");
        let mut files = Vec::new();
        fixture_files(&image_root, "tvg", &mut files);
        assert_eq!(files.len(), 7, "pinned TinyVG test count changed");
        let mut failures = Vec::new();
        for path in files {
            let relative = path
                .strip_prefix(&image_root)
                .expect("relative TinyVG test");
            let document = decode_media(&fs::read(&path).expect("read TinyVG test"))
                .unwrap_or_else(|error| panic!("{}: {error}", relative.display()));
            let reference_path = reference_root.join(relative).with_extension("tvg.png");
            let reference = image::decode(&fs::read(&reference_path).expect("read TinyVG PNG"))
                .unwrap_or_else(|error| panic!("{} reference: {error}", relative.display()));
            assert_eq!(
                document.size(),
                (f64::from(reference.width()), f64::from(reference.height())),
                "{}",
                relative.display()
            );
            let actual = render(document, reference.width(), reference.height(), false);
            let expected = premultiplied_rgba(reference.pixels());
            // The published PNGs come from another renderer. Bound both average error and
            // the number of large per-pixel errors so antialiasing may differ without
            // allowing missing geometry or broad color changes.
            let pixels = actual.len() / 4;
            let (total, outliers) = differences(&actual, &expected).fold(
                (0_u64, 0),
                |(total, outliers), difference| {
                    (
                        total + u64::from(difference),
                        outliers + usize::from(difference > 64),
                    )
                },
            );
            let mean = total as f64 / pixels as f64;
            let allowed_outliers = pixels.div_ceil(50);
            if mean > 5.0 || outliers > allowed_outliers {
                failures.push(format!(
                    "{}: mean max-channel difference {mean:.3} (allowed 5.000), pixels above 64: {outliers} (allowed {allowed_outliers})",
                    relative.display()
                ));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn renders_alpha_and_luminance_masks() {
        for svg in [
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
              <defs><mask id="half" mask-type="alpha" maskContentUnits="objectBoundingBox">
                <rect width=".5" height="1" fill="white"/>
              </mask></defs>
              <rect width="20" height="10" fill="red" mask="url(#half)"/>
            </svg>"##
                .as_slice(),
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">
              <defs><mask id="half" mask-type="luminance">
                <rect width="20" height="10" fill="white"/>
                <rect x="10" width="10" height="10" fill="black"/>
              </mask></defs>
              <rect width="20" height="10" fill="red" mask="url(#half)"/>
            </svg>"##
                .as_slice(),
        ] {
            let pixels = render_svg(svg, 20, 10);
            assert!(pixels[(5 * 20 + 5) * 4 + 3] > 240);
            assert!(pixels[(5 * 20 + 15) * 4 + 3] < 16);
        }
    }

    #[test]
    fn renders_transformed_mask_with_view_box() {
        let pixels = render_svg(
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 100 100">
              <mask id="cutout">
                <rect width="100" height="100" fill="white"/>
                <rect x="40" width="20" height="100" fill="black"/>
              </mask>
              <g transform="translate(10) scale(.8)">
                <rect width="100" height="100" fill="white" mask="url(#cutout)"/>
              </g>
            </svg>"##,
            200,
            200,
        );
        assert!(pixels[(100 * 200 + 40) * 4 + 3] > 240);
        assert!(pixels[(100 * 200 + 100) * 4 + 3] < 16);
    }

    #[test]
    fn renders_svg_gradients_in_srgb() {
        let pixels = render_svg(
            br##"<svg xmlns="http://www.w3.org/2000/svg" width="256" height="1">
              <linearGradient id="gradient"><stop/><stop offset="1" stop-color="white"/></linearGradient>
              <rect width="256" height="1" fill="url(#gradient)"/>
            </svg>"##,
            256,
            1,
        );
        let midpoint = pixels[128 * 4];
        assert!((120..=136).contains(&midpoint), "midpoint was {midpoint}");
    }

    const SIZES: [f32; 6] = [1.0, 23.0, 24.0, 25.0, 48.0, 137.0];

    #[test]
    fn checker_grid_covers_the_area_from_the_center() {
        for size in SIZES {
            let (half_columns, half_rows) = checker_grid(size, 70.0);
            assert!(half_columns as f32 * TILE >= size / 2.0);
            assert!(half_rows as f32 * TILE >= 35.0);
        }
    }

    #[test]
    fn tile_colors_do_not_flip_while_resizing() {
        for size in SIZES {
            let (half_columns, half_rows) = checker_grid(size, size);
            // Tile 0 is always next to the center, so center colors do not depend on the size.
            assert!((-half_columns..half_columns).contains(&0));
            assert!((-half_rows..half_rows).contains(&0));
            assert!(tile_is_filled(0, 0));
            assert!(!tile_is_filled(0, -1));
            assert!(tile_is_filled(-1, -1));
        }
    }

    #[test]
    fn thumbnails_fit_the_size_without_enlarging() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="200">
            <rect width="400" height="200" fill="red"/></svg>"#;
        let thumbnail = render_thumbnail(decode_media(svg).expect("SVG"), 100).expect("thumbnail");
        assert_eq!((thumbnail.width(), thumbnail.height()), (100, 50));
        assert_eq!(&thumbnail.data()[..4], [255, 0, 0, 255]);
        let small = render_thumbnail(decode_media(svg).expect("SVG"), 1000).expect("thumbnail");
        assert_eq!((small.width(), small.height()), (400, 200));
    }

    #[test]
    fn rasters_draw_the_requested_frame_fitted_and_centered() {
        let frame = |pixel: [u8; 4]| Frame {
            pixels: Arc::from(pixel),
            delay: std::time::Duration::from_millis(100),
        };
        let renderer = MediaRenderer::new(Media::Raster(Raster {
            width: 1,
            height: 1,
            frames: vec![frame([0, 0, 255, 255]), frame([255, 0, 0, 255])],
            loop_count: LoopCount::Infinite,
        }));
        let mut canvas = OffscreenCanvas::new(4, 2);
        canvas.draw(|context| renderer.draw_fitted(context, 1, (0.0, 0.0, 4.0, 2.0)));
        let pixels = canvas.pixels();
        assert_eq!(pixels[3], 0);
        assert_eq!(&pixels[(4 + 1) * 4..][..4], [0, 0, 255, 255]);
    }
}
