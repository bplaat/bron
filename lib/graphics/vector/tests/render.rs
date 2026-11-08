/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Renders the decoded test images and compares them with their references.

use std::fs;
use std::path::{Path, PathBuf};

use bcanvas::{Color, OffscreenCanvas};
use vector::decode_vector;

const TESTS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");

const fn size(image: &OffscreenCanvas) -> (f32, f32) {
    (image.logical_width(), image.logical_height())
}

// Renders an image fitted into a canvas, as premultiplied RGBA8 pixels.
fn render(image: OffscreenCanvas, width: u32, height: u32, white_background: bool) -> Vec<u8> {
    let (image_width, image_height) = size(&image);
    let scale = (width as f32 / image_width).min(height as f32 / image_height);
    let (fitted_width, fitted_height) = (image_width * scale, image_height * scale);
    let mut canvas = OffscreenCanvas::new(width, height);
    canvas.draw(|context| {
        if white_background {
            context.set_fill_style(Color::rgb(255, 255, 255));
            context.fill_rect(0.0, 0.0, width as f32, height as f32);
        }
        context.draw_canvas(
            &image,
            (width as f32 - fitted_width) / 2.0,
            (height as f32 - fitted_height) / 2.0,
            fitted_width,
            fitted_height,
        );
    });
    premultiplied_rgba(canvas.pixels())
}

fn render_svg(svg: &[u8], width: u32, height: u32) -> Vec<u8> {
    render(decode_vector(svg).expect("SVG"), width, height, false)
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
    let root = Path::new(TESTS);
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
        let document =
            decode_vector(&data).unwrap_or_else(|error| panic!("{}: {error}", relative.display()));
        let reference = decode_vector(&reference_data)
            .unwrap_or_else(|error| panic!("{} reference: {error}", relative.display()));
        assert_eq!(size(&document), size(&reference), "{}", relative.display());
        let (width, height) = size(&document);
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
    let root = Path::new(TESTS);
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
        let document = decode_vector(&fs::read(&path).expect("read TinyVG test"))
            .unwrap_or_else(|error| panic!("{}: {error}", relative.display()));
        let reference_path = reference_root.join(relative).with_extension("tvg.png");
        let reference = image::decode(&fs::read(&reference_path).expect("read TinyVG PNG"))
            .unwrap_or_else(|error| panic!("{} reference: {error}", relative.display()));
        assert_eq!(
            size(&document),
            (reference.width() as f32, reference.height() as f32),
            "{}",
            relative.display()
        );
        let actual = render(document, reference.width(), reference.height(), false);
        let expected = premultiplied_rgba(reference.pixels());
        // The published PNGs come from another renderer. Bound both average error and
        // the number of large per-pixel errors so antialiasing may differ without
        // allowing missing geometry or broad color changes.
        let pixels = actual.len() / 4;
        let (total, outliers) =
            differences(&actual, &expected).fold((0_u64, 0), |(total, outliers), difference| {
                (
                    total + u64::from(difference),
                    outliers + usize::from(difference > 64),
                )
            });
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

#[test]
fn huge_images_decode_without_pixels() {
    let image = decode_vector(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="1e12" height="1e40"><rect width="10" height="10"/></svg>"#,
    )
    .expect("SVG");
    assert_eq!(size(&image), (1e12, 0.0));
}
