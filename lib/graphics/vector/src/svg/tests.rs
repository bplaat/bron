/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashSet;
use std::fs;

use bcanvas::OffscreenCanvas;

use super::path::parse_path;
use super::xml::XmlDocument;
use super::{decode, is_svg};
use crate::test_support::FixtureCorpus;
use crate::{PathSegment, Point, VectorDecodeError, pixel};

fn render(svg: &[u8]) -> OffscreenCanvas {
    decode(svg).unwrap_or_else(|error| panic!("{error}"))
}

fn error(svg: &[u8]) -> Option<VectorDecodeError> {
    decode(svg).err()
}

fn alpha(image: &mut OffscreenCanvas, x: u32, y: u32) -> u8 {
    pixel(image, x, y)[3]
}

fn is_blank(image: &mut OffscreenCanvas) -> bool {
    image.pixels().iter().all(|value| *value == 0)
}

fn match_reference(source: &str) -> String {
    let document = XmlDocument::parse(source).expect("parse WPT SVG metadata");
    let mut references = document.elements.iter().filter_map(|element| {
        (element.name == "link" && element.attr("rel") == Some("match"))
            .then(|| element.attr("href"))
            .flatten()
    });
    let reference = references.next().expect("WPT SVG has a match reference");
    assert!(
        references.next().is_none(),
        "WPT SVG has exactly one match reference"
    );
    reference.to_owned()
}

#[test]
fn decodes_wpt_reference_pairs_offline() {
    let corpus = FixtureCorpus::new("svg");
    let canonical_reference_root = corpus
        .references
        .canonicalize()
        .expect("canonical SVG reference directory");
    let files = corpus.image_files("svg");
    assert_eq!(files.len(), 93, "pinned WPT SVG test count changed");
    let mut used_references = HashSet::new();
    for path in files {
        let relative = corpus.relative_name(&path);
        let data = fs::read(&path).expect("read SVG fixture");
        let source = std::str::from_utf8(&data).expect("WPT SVG is UTF-8");
        let reference = path
            .parent()
            .expect("SVG test directory")
            .join(match_reference(source))
            .canonicalize()
            .expect("resolve SVG match reference");
        assert!(
            reference.starts_with(&canonical_reference_root),
            "{}: match reference must be under tests/reference/svg",
            relative.display()
        );
        used_references.insert(
            reference
                .strip_prefix(&canonical_reference_root)
                .expect("relative canonical SVG reference")
                .to_owned(),
        );

        let image = decode(&data).unwrap_or_else(|error| panic!("{}: {error}", relative.display()));
        let reference_data = fs::read(&reference).expect("read SVG match reference");
        let reference_image = decode(&reference_data)
            .unwrap_or_else(|error| panic!("{} reference: {error}", relative.display()));
        assert_eq!(
            (image.logical_width(), image.logical_height()),
            (
                reference_image.logical_width(),
                reference_image.logical_height()
            ),
            "{}",
            relative.display()
        );
    }

    let references = corpus.reference_files("svg");
    assert_eq!(references.len(), 41, "pinned SVG reference count changed");
    let references = references
        .into_iter()
        .map(|reference| {
            reference
                .strip_prefix(&corpus.references)
                .expect("relative SVG reference")
                .to_owned()
        })
        .collect();
    assert_eq!(used_references, references);
}

#[test]
fn decodes_compound_vector_artwork() {
    let mut image = render(
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="60" height="40">
          <path d="M2 2h20v20H2z" fill="#f60"/>
          <g transform="translate(30 5)">
            <path d="M0 15C0 5 20 5 20 15Z" fill="#06f"/>
          </g>
        </svg>"##,
    );
    assert_eq!(image.logical_width(), 60.0);
    assert_eq!(pixel(&mut image, 10, 10), [255, 102, 0, 255]);
    assert_eq!(pixel(&mut image, 40, 18), [0, 102, 255, 255]);
    assert_eq!(alpha(&mut image, 26, 10), 0);
}

#[test]
fn parses_compact_path_numbers_and_arcs() {
    let path = parse_path("M.5-.5L1e2.5A10 5 0 01120 20z").expect("valid compact path");
    assert!(
        path.iter()
            .any(|segment| matches!(segment, PathSegment::CubicTo { .. }))
    );
}

#[test]
fn normalizes_path_length_dashes() {
    // A length of 4 makes dashes of .25 a quarter of each 100 unit side.
    let mut image = render(br#"<svg xmlns='http://www.w3.org/2000/svg' width='120' height='40'>
      <rect x='10' y='10' width='100' height='100' fill='none' stroke='black' stroke-width='2' stroke-dasharray='.25' pathLength='4'/>
      <path d='M10 30H110' stroke='black' stroke-width='2' stroke-dasharray='.25' pathLength='1'/>
    </svg>"#);
    assert_eq!(alpha(&mut image, 20, 10), 255);
    assert_eq!(alpha(&mut image, 45, 10), 0);
    assert_eq!(alpha(&mut image, 70, 10), 255);
    assert_eq!(alpha(&mut image, 20, 30), 255);
    assert_eq!(alpha(&mut image, 45, 30), 0);
}

#[test]
fn applies_svg_stacking_blending_and_isolation() {
    let mut stacked = render(
        br#"<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>
          <rect width='10' height='10' fill='red' style='z-index:1'/>
          <rect width='10' height='10' fill='lime'/>
        </svg>"#,
    );
    assert_eq!(pixel(&mut stacked, 5, 5), [255, 0, 0, 255]);
    let mut blended = render(
        br#"<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>
          <rect width='10' height='10' fill='red'/>
          <rect width='10' height='10' fill='lime' style='mix-blend-mode:screen'/>
        </svg>"#,
    );
    assert_eq!(pixel(&mut blended, 5, 5), [255, 255, 0, 255]);
    for (isolation, expected) in [("isolate", [0, 0, 0, 255]), ("auto", [0, 255, 0, 255])] {
        let source = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>
              <rect width='10' height='10' fill='lime'/>
              <g isolation='{isolation}'><rect width='10' height='10' style='mix-blend-mode:screen'/></g>
            </svg>"
        );
        assert_eq!(
            pixel(&mut render(source.as_bytes()), 5, 5),
            expected,
            "{isolation}"
        );
    }
}

#[test]
fn supports_css_transforms_paint_order_and_keyword_case() {
    let mut image = render(
        br#"<svg xmlns='http://www.w3.org/2000/svg' width='50' height='30'>
          <rect width='10' height='10' color='red' fill='CURRENTCOLOR' stroke='blue'
                style='transform:translate(29px,11px);paint-order:stroke fill'/>
        </svg>"#,
    );
    assert_eq!(pixel(&mut image, 34, 16), [255, 0, 0, 255]);
    // The fill covers the inner half of the stroke.
    assert_eq!(pixel(&mut image, 29, 16), [255, 0, 0, 255]);
    let outside = pixel(&mut image, 28, 16);
    assert!(outside[0] == 0 && outside[2] == 255 && outside[3] > 0);
}

#[test]
fn inherits_complete_paint_order_and_rejects_invalid_transform_units() {
    let mut image = render(
        br##"<svg xmlns='http://www.w3.org/2000/svg' width='20' height='10'>
          <defs><marker id='dot' markerUnits='userSpaceOnUse' markerWidth='4' markerHeight='4'>
            <rect width='4' height='4'/>
          </marker></defs>
          <g paint-order='markers stroke'>
            <path d='M0 2L10 2Z' fill='red' stroke='blue' stroke-width='2' marker-start='url(#dot)'/>
          </g>
        </svg>"##,
    );
    // The stroke is drawn over the marker.
    assert_eq!(pixel(&mut image, 1, 1), [0, 0, 255, 255]);
    assert_eq!(pixel(&mut image, 1, 3), [0, 0, 0, 255]);
    assert_eq!(
        error(b"<svg><rect width='1' height='1' transform='scale(2px)'/></svg>"),
        Some(VectorDecodeError::InvalidData)
    );
}

#[test]
fn clips_marker_viewports_and_measures_general_cubic_paths() {
    let mut image = render(
        br##"<svg xmlns='http://www.w3.org/2000/svg' width='120' height='120'>
          <defs><marker id='mark' markerUnits='userSpaceOnUse' markerWidth='2' markerHeight='2'>
            <rect x='-10' y='-10' width='20' height='20'/>
          </marker></defs>
          <path d='M0 0L10 0' marker-start='url(#mark)'/>
          <path d='M10 10C10 10 110 10 110 10C110 10 110 110 110 110C110 110 10 110 10 110C10 110 10 10 10 10Z'
                fill='none' stroke='black' stroke-width='2' stroke-dasharray='.25' pathLength='4'/>
        </svg>"##,
    );
    assert_eq!(alpha(&mut image, 1, 1), 255);
    assert_eq!(alpha(&mut image, 5, 5), 0);
    assert_eq!(alpha(&mut image, 20, 10), 255);
    assert_eq!(alpha(&mut image, 45, 10), 0);
}

#[test]
fn expands_local_markers_with_context_paint() {
    let mut image = render(
        br##"<svg xmlns='http://www.w3.org/2000/svg' width='40' height='20'>
          <defs><marker id='arrow' markerUnits='userSpaceOnUse' orient='auto'
            markerWidth='8' markerHeight='8' refX='4' refY='4'>
            <path d='M0 0L8 4L0 8Z' fill='context-stroke'/>
          </marker></defs>
          <path d='M10 10L30 10' fill='none' stroke='lime' marker-end='url(#arrow)'/>
        </svg>"##,
    );
    assert_eq!(pixel(&mut image, 27, 12), [0, 255, 0, 255]);
    assert_eq!(alpha(&mut image, 15, 12), 0);
}

#[test]
fn recovers_valid_path_prefix_and_bounds_source_size() {
    let path = parse_path("M0 0L10 0L20 Z ignored").expect("valid path prefix");
    assert!(path.contains(&PathSegment::LineTo(Point { x: 10.0, y: 0.0 })));
    assert_eq!(
        error(&vec![b' '; super::MAX_SOURCE_BYTES + 1]),
        Some(VectorDecodeError::ResourceLimit)
    );
}

#[test]
fn applies_gradient_color_interpolation() {
    let gradient = |interpolation: &str| {
        let source = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='10' height='10'>
              <defs><linearGradient id='g' color-interpolation='{interpolation}'>
                <stop stop-color='gray'/><stop offset='1' stop-color='gray'/>
              </linearGradient></defs>
              <rect width='10' height='10' fill='url(#g)'/>
            </svg>"
        );
        pixel(&mut render(source.as_bytes()), 5, 5)[0]
    };
    assert!(gradient("linearRGB") > gradient("sRGB") + 20);
}

#[test]
fn ignores_unsupported_content_and_keeps_supported_siblings() {
    for fragment in [
        "<?example ignored?>",
        "<script>ignored()</script>",
        "<text>ignored</text>",
        "<image href=\"https://example.invalid/image.png\"/>",
        "<filter id=\"blur\"/>",
        "<rect width=\"10\" height=\"10\" filter=\"url(#blur)\" marker-start=\"url(#missing)\"/>",
        "<circle cx=\"20\" cy=\"20\" r=\"1em\"/>",
    ] {
        let source = format!(
            "{prefix}<svg xmlns=\"http://www.w3.org/2000/svg\">{body}</svg>",
            prefix = if fragment.starts_with("<?") {
                fragment
            } else {
                ""
            },
            body = if fragment.starts_with("<?") {
                ""
            } else {
                fragment
            }
        );
        decode(source.as_bytes()).unwrap_or_else(|error| panic!("{fragment}: {error:?}"));
    }
    let mut image = render(
        br#"<?example ignored?><svg xmlns="http://www.w3.org/2000/svg">
          <script>ignored()</script>
          <text>ignored</text>
          <image href="https://example.invalid/image.png"/>
          <filter id="blur"/>
          <rect width="10" height="10" filter="url(#blur)" marker-start="url(#missing)"/>
          <circle cx="20" cy="20" r="1em"/>
        </svg>"#,
    );
    assert_eq!(alpha(&mut image, 5, 5), 255);
    assert_eq!(alpha(&mut image, 20, 20), 0);
}

#[test]
fn preserves_xml_structure_and_ignores_foreign_namespaces() {
    let mut image = render(
        br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:rdf="urn:rdf">
          <rdf:metadata><rdf:value>ignored</rdf:value></rdf:metadata>
          <g style="display:none"><rect x="20" width="5" height="5"/></g>
          <rect width="10" height="10"/>
        </svg>"#,
    );
    assert_eq!(alpha(&mut image, 5, 5), 255);
    assert_eq!(alpha(&mut image, 22, 2), 0);
    assert_eq!(
        error(b"outside<svg xmlns='http://www.w3.org/2000/svg'/>"),
        Some(VectorDecodeError::InvalidData)
    );
    assert_eq!(
        error(b"<svg xmlns='http://www.w3.org/2000/svg'><bad:item/></svg>"),
        Some(VectorDecodeError::InvalidData)
    );
    assert_eq!(
        error(br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:a="urn:x" xmlns:b="urn:x" a:value="1" b:value="2"/>"#),
        Some(VectorDecodeError::InvalidData)
    );
    assert_eq!(
        error(br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xml="urn:wrong"/>"#),
        Some(VectorDecodeError::InvalidData)
    );
}

#[test]
fn decodes_shapes_transforms_styles_and_clips() {
    let mut image = render(br#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="80">
            <defs><clipPath id="clip"><circle cx="20" cy="20" r="15"/></clipPath></defs>
            <g transform="translate(2, -3) rotate(10)" opacity=".5" clip-path="url(#clip)" color="blue">
              <rect x="1" y="2" width="30" height="20" rx="3" fill="currentColor"/>
              <ellipse cx="50" cy="20" rx="10" ry="5" fill="none" stroke="rgba(255,0,0,.5)" stroke-dasharray="2 3 4"/>
              <polygon points="0,0 10,0 5,10"/>
            </g></svg>"#);
    let inside = pixel(&mut image, 15, 15);
    assert!(
        inside[2] == 255 && (120..=136).contains(&inside[3]),
        "{inside:?}"
    );
    // The ellipse is outside the clip.
    assert_eq!(alpha(&mut image, 48, 27), 0);
}

#[test]
fn repeats_odd_dash_lists() {
    let mut image = render(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="10">
          <path d="M0 5H40" stroke="black" stroke-width="2" stroke-dasharray="10 5 5"/>
        </svg>"#,
    );
    for (x, expected) in [(5, 255), (12, 0), (17, 255), (25, 0), (32, 255), (37, 0)] {
        assert_eq!(alpha(&mut image, x, 5), expected, "{x}");
    }
}

#[test]
fn applies_transform_lists_in_svg_order() {
    let mut image = render(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
          <g transform="translate(20 30) scale(2)">
            <rect width="10" height="5"/>
          </g>
        </svg>"#,
    );
    for (x, y, expected) in [
        (21, 31, 255),
        (39, 39, 255),
        (19, 31, 0),
        (41, 31, 0),
        (21, 41, 0),
    ] {
        assert_eq!(alpha(&mut image, x, y), expected, "{x} {y}");
    }
}

#[test]
fn resolves_active_viewports_and_visibility() {
    let mut image = render(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100" viewBox="0 0 100 50" visibility="hidden">
          <rect width="50%" height="50%" fill="red"/>
          <rect width="50%" height="50%" visibility="visible"/>
        </svg>"#,
    );
    assert_eq!(pixel(&mut image, 99, 49), [0, 0, 0, 255]);
    assert_eq!(alpha(&mut image, 101, 51), 0);

    let mut nested = render(
        br#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">
          <svg x="10" y="5" width="50" height="20" viewBox="0 0 10 10">
            <rect width="100%" height="100%"/>
          </svg>
        </svg>"#,
    );
    for (x, y, expected) in [
        (26, 6, 255),
        (44, 24, 255),
        (24, 6, 0),
        (46, 6, 0),
        (26, 26, 0),
    ] {
        assert_eq!(alpha(&mut nested, x, y), expected, "{x} {y}");
    }
}

#[test]
fn resolves_user_space_gradient_percentages() {
    let mut image = render(
        br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 50">
          <defs><linearGradient id="g" gradientUnits="userSpaceOnUse" x2="100%">
            <stop stop-color="red"/><stop offset="1" stop-color="blue"/>
          </linearGradient></defs>
          <rect width="100" height="50" fill="url(#g)"/>
        </svg>"##,
    );
    let middle = pixel(&mut image, 50, 25);
    assert!((100..156).contains(&middle[0]) && (100..156).contains(&middle[2]));
}

#[test]
fn disables_invalid_and_zero_sized_shapes() {
    for element in [
        "<circle r='-1'/>",
        "<ellipse rx='-1' ry='-1'/>",
        "<ellipse rx='1' ry='0'/>",
        "<rect width='-1' height='1'/>",
        "<rect width='1' height='0'/>",
    ] {
        let source = format!("<svg xmlns='http://www.w3.org/2000/svg'>{element}</svg>");
        assert!(is_blank(&mut render(source.as_bytes())), "{element}");
    }
    assert_eq!(
        error(b"<svg><path d='M0 0L1 1' stroke='black' stroke-width='-1'/></svg>"),
        Some(VectorDecodeError::InvalidData)
    );
    // A negative radius computes to auto, which takes the other radius.
    let mut ellipse = render(b"<svg><ellipse cx='10' cy='10' rx='-5' ry='5'/></svg>");
    assert_eq!(alpha(&mut ellipse, 13, 10), 255);
}

#[test]
fn validates_numeric_and_color_list_syntax() {
    let mut image = render(
        br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1" fill="RGB(100% 0% 0% / 50%)"/></svg>"#,
    );
    let color = pixel(&mut image, 0, 0);
    assert!(
        color[0] > 250 && (120..=136).contains(&color[3]),
        "{color:?}"
    );

    for source in [
        "<svg viewBox='0,,0 1 1'/>",
        "<svg viewBox='0-1 1 1'/>",
        "<svg><rect width='1' height='1' transform='translate(1,)'/></svg>",
        "<svg><rect width='1' height='1' transform='translate(1),'/></svg>",
        "<svg><rect width='1' height='1' transform='translate(1)scale(2)'/></svg>",
        "<svg><path d='M0,,0'/></svg>",
        "<svg><rect width='1' height='1' fill='rgb(1,,2,3)'/></svg>",
        "<svg><rect width='1' height='1' fill='rgb(1,2 3)'/></svg>",
    ] {
        assert_eq!(
            error(source.as_bytes()),
            Some(VectorDecodeError::InvalidData),
            "{source}"
        );
    }
}

#[test]
fn resolves_luminance_and_alpha_masks() {
    let mut image = render(
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80">
          <rect x="10" y="10" width="40" height="20" mask="url(#fade)"/>
          <defs>
            <mask id="fade" mask-type="alpha" maskContentUnits="objectBoundingBox">
              <rect width=".5" height="1" fill="white"/>
            </mask>
          </defs>
        </svg>"##,
    );
    assert_eq!(alpha(&mut image, 15, 20), 255);
    assert_eq!(alpha(&mut image, 40, 20), 0);
}

#[test]
fn rotated_masks_use_the_untransformed_object_bounding_box() {
    let mut image = render(
        br##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
          <defs><mask id="mask"><rect width="100" height="100" fill="white"/></mask></defs>
          <rect x="10" y="20" width="40" height="10" transform="rotate(45)" mask="url(#mask)"/>
        </svg>"##,
    );
    // The local points (30, 25) and (45, 25) of the rotated rectangle.
    assert_eq!(alpha(&mut image, 3, 38), 255);
    assert_eq!(alpha(&mut image, 14, 49), 255);
}

#[test]
fn resolves_forward_nested_masks_and_rejects_cycles() {
    let mut image = render(br##"<svg xmlns="http://www.w3.org/2000/svg">
          <rect width="20" height="20" mask="url(#outer)"/>
          <defs>
            <mask id="inner"><rect width="20" height="20" fill="white"/></mask>
            <mask id="outer"><g mask="url(#inner)"><circle cx="10" cy="10" r="10" fill="white"/></g></mask>
          </defs>
        </svg>"##);
    assert_eq!(alpha(&mut image, 10, 10), 255);
    assert_eq!(alpha(&mut image, 1, 1), 0);
    assert_eq!(
        error(
            br##"<svg xmlns="http://www.w3.org/2000/svg">
              <defs><mask id="cycle"><rect width="1" height="1" mask="url(#cycle)"/></mask></defs>
              <rect width="1" height="1" mask="url(#cycle)"/>
            </svg>"##
        ),
        Some(VectorDecodeError::InvalidData)
    );
}

#[test]
fn resolves_gradient_stops_and_forward_references() {
    let mut image = render(br##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10" viewBox="0 0 20 10">
          <rect width="20" height="10" fill="url(#derived)"/>
          <defs>
            <linearGradient id="base"><stop offset="0" stop-color="#f00"/><stop offset="100%" stop-color="#00f"/></linearGradient>
            <linearGradient id="derived" href="#base" x2="50%" spreadMethod="reflect"/>
          </defs>
        </svg>"##);
    // The gradient runs to blue at the middle and is reflected back to red.
    assert!(pixel(&mut image, 10, 5)[2] > 200);
    assert!(pixel(&mut image, 19, 5)[0] > 200);
}

#[test]
fn css_keeps_valid_fallback_when_later_value_is_invalid() {
    let mut image = render(br#"<svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1" style="fill:red;fill:context-fill"/></svg>"#);
    assert_eq!(pixel(&mut image, 0, 0), [255, 0, 0, 255]);
}

#[test]
fn recognized_malformed_svg_never_becomes_unknown_input() {
    let bytes = b"<!-- leading comment --><svg><path d='M0 nan'/></svg>";
    assert!(is_svg(bytes));
    assert_eq!(error(bytes), Some(VectorDecodeError::InvalidData));
}

#[test]
fn uses_css_absolute_units_and_normalized_percentages() {
    let mut stroke = render(
        br#"<svg width="300" height="100"><path d="M0 50H300" stroke="black" stroke-width="1in" stroke-dasharray="1pc 12pt"/></svg>"#,
    );
    for (x, y, expected) in [(8, 3, 255), (8, 1, 0), (24, 50, 0), (40, 50, 255)] {
        assert_eq!(alpha(&mut stroke, x, y), expected, "{x} {y}");
    }
    // Ten percent of the normalized diagonal of 300 by 400 is 35.36.
    let mut circle =
        render(br#"<svg width="300" height="400"><circle cx="150" cy="200" r="10%"/></svg>"#);
    assert_eq!(alpha(&mut circle, 184, 200), 255);
    assert_eq!(alpha(&mut circle, 187, 200), 0);
}

#[test]
fn resolves_paint_fallbacks_and_gradient_templates() {
    let mut image = render(br##"<svg width="100" height="10"><defs>
      <linearGradient id="base" gradientUnits="userSpaceOnUse" x1="20" x2="30" spreadMethod="reflect">
        <stop stop-color="red"/><stop offset="1" stop-color="blue"/>
      </linearGradient>
      <linearGradient id="derived" href="#base"/></defs>
      <rect width="10" height="10" fill="url(#missing) currentColor" color="blue"/>
      <rect x="20" width="80" height="10" fill="url(#derived)"/></svg>"##);
    assert_eq!(pixel(&mut image, 5, 5), [0, 0, 255, 255]);
    assert!(pixel(&mut image, 21, 5)[0] > 200);
    assert!(pixel(&mut image, 39, 5)[0] > 200);
}

#[test]
fn uses_fallback_for_singular_gradient_transforms() {
    let mut image = render(br##"<svg width="20" height="10"><defs>
      <linearGradient id="linear" gradientTransform="scale(0)"><stop stop-color="red"/></linearGradient>
      <radialGradient id="radial" gradientTransform="matrix(0 0 0 0 0 0)"><stop stop-color="red"/></radialGradient>
    </defs><rect width="10" height="10" fill="url(#linear) green"/>
    <rect x="10" width="10" height="10" fill="url(#radial) green"/></svg>"##);
    assert_eq!(pixel(&mut image, 5, 5), [0, 128, 0, 255]);
    assert_eq!(pixel(&mut image, 15, 5), [0, 128, 0, 255]);
}

#[test]
fn instantiates_symbol_viewports_without_rendering_definitions() {
    let mut image = render(br##"<svg width="100" height="100"><symbol id="s" viewBox="0 0 10 10"><rect width="10" height="10"/></symbol><use href="#s" x="20" y="30" width="40" height="20"/></svg>"##);
    for (x, y, expected) in [
        (31, 31, 255),
        (49, 49, 255),
        (29, 31, 0),
        (51, 31, 0),
        (5, 5, 0),
    ] {
        assert_eq!(alpha(&mut image, x, y), expected, "{x} {y}");
    }
}

#[test]
fn applies_bounded_embedded_css_cascade() {
    let mut image = render(br##"<svg width="10" height="10"><style><![CDATA[
      rect { fill: red; } .shape { fill: green; } #target.shape { fill: blue !important; }
      g rect { fill: white; } rect:hover { fill: white; }
    ]]></style><rect id="target" class="shape extra" width="10" height="10" style="fill: yellow"/></svg>"##);
    assert_eq!(pixel(&mut image, 5, 5), [0, 0, 255, 255]);
}

#[test]
fn applies_css_geometry_to_shapes() {
    let mut image = render(
        br#"<svg width="200" height="100"><style>
      circle { cx: 25%; cy: 50%; r: 10%; }
      ellipse { rx: auto; ry: 20px; }
    </style>
    <circle cx="1" cy="1" r="1"/>
    <ellipse cx="100" cy="50"/>
    <rect x="120" y="10" style="width: 40px; height: 30px"/>
    </svg>"#,
    );
    // Ten percent of the normalized diagonal of 200 by 100 is 15.81.
    for (x, y, expected) in [
        (64, 50, 255),
        (67, 50, 0),
        (2, 2, 0),
        (118, 50, 255),
        (100, 31, 255),
        (100, 29, 0),
        (121, 11, 255),
        (159, 39, 255),
        (161, 11, 0),
        (121, 41, 0),
    ] {
        assert_eq!(alpha(&mut image, x, y), expected, "{x} {y}");
    }

    let mut cascade = render(
        br#"<svg width="100" height="100"><style>
      rect { width: 10px; width: invalid; height: 20px !important; }
      #target { height: 30px; }
    </style><rect id="target" width="5" height="5" style="width: 40px"/></svg>"#,
    );
    for (x, y, expected) in [(39, 19, 255), (41, 1, 0), (1, 21, 0)] {
        assert_eq!(alpha(&mut cascade, x, y), expected, "{x} {y}");
    }
}

#[test]
fn uses_default_viewport_and_disables_zero_viewboxes() {
    let default = render(b"<svg><rect width='1' height='1'/></svg>");
    assert_eq!(
        (default.logical_width(), default.logical_height()),
        (300.0, 150.0)
    );

    let mut root = render(
        b"<svg width='200' height='100' viewBox='0 0 0 10'><rect width='10' height='10'/></svg>",
    );
    assert!(is_blank(&mut root));

    let mut symbol = render(
        br##"<svg width="200" height="100">
      <symbol id="empty" viewBox="0 0 10 0"><rect width="10" height="10"/></symbol>
      <use href="#empty" width="100" height="100"/>
    </svg>"##,
    );
    assert!(is_blank(&mut symbol));
    assert_eq!(
        error(b"<svg viewBox='0 0 -1 10'/></svg>"),
        Some(VectorDecodeError::InvalidData)
    );
}
