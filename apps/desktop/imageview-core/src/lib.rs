/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Media decoding, sizing, zooming and folder browsing for ImageView and its file manager
//! extensions, with a native renderer for each platform.

use std::time::SystemTime;

pub use image::LoopCount;

// The `ns_string!` macro refers to these through the crate root.
#[cfg(target_os = "macos")]
#[doc(hidden)]
pub use crate::appkit::{__CFConstantStringClassReference, CFConstString};
pub use crate::browse::{compare_names, is_supported, neighbour_path, sibling_index};
pub use crate::media::{Frame, Media, Raster, decode_media};
pub use crate::render::{MediaRenderer, draw_checkerboard, render_thumbnail};

/// AppKit images and rendering for macOS and its Quick Look extensions.
#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
pub mod appkit;
mod browse;
mod media;
mod render;

/// The file name extensions of the formats the local decoders read.
pub const EXTENSIONS: &[&str] = &[
    "bmp", "dib", "gif", "jpg", "jpeg", "jpe", "jfif", "png", "apng", "ico", "qoi", "svg", "tvg",
];

/// The magnification the media may be shrunk and enlarged to.
pub const MINIMUM_MAGNIFICATION: f64 = 0.02;
/// The largest magnification of the media.
pub const MAXIMUM_MAGNIFICATION: f64 = 64.0;
/// The factor a single zoom step magnifies or shrinks the media by.
pub const ZOOM_STEP: f64 = 1.5;
/// The zoom exponent per scroll wheel line.
pub const WHEEL_ZOOM_RATE: f64 = 0.15;
/// The zoom exponent per precise scroll point, like a trackpad.
pub const PRECISE_ZOOM_RATE: f64 = 0.005;

// The size media is shown at, which is the media itself when it lies between these bounds.
const MINIMUM_CONTENT_SIZE: (f64, f64) = (320.0, 240.0);
const MAXIMUM_CONTENT_SIZE: (f64, f64) = (1200.0, 800.0);

/// Returns a bounded window or preview size that preserves the media's shape.
pub fn preferred_content_size(width: f64, height: f64) -> (f64, f64) {
    let scale = (MAXIMUM_CONTENT_SIZE.0 / width)
        .min(MAXIMUM_CONTENT_SIZE.1 / height)
        .min(1.0);
    (
        (width * scale).max(MINIMUM_CONTENT_SIZE.0),
        (height * scale).max(MINIMUM_CONTENT_SIZE.1),
    )
}

/// Returns the magnification that shows all of the media inside the visible area.
pub fn fit_magnification(visible: (f64, f64), media: (f64, f64)) -> f64 {
    if media.0 <= 0.0 || media.1 <= 0.0 {
        return 1.0;
    }
    (visible.0.max(1.0) / media.0)
        .min(visible.1.max(1.0) / media.1)
        .clamp(MINIMUM_MAGNIFICATION, MAXIMUM_MAGNIFICATION)
}

/// Clamps a magnification to the supported range.
pub const fn clamp_magnification(magnification: f64) -> f64 {
    magnification.clamp(MINIMUM_MAGNIFICATION, MAXIMUM_MAGNIFICATION)
}

/// The window title of media, its file name and stored size.
pub fn window_title(name: &str, size: (f64, f64)) -> String {
    format!(
        "{name} ({}x{})",
        size.0.round() as i64,
        size.1.round() as i64
    )
}

/// The length and modification time of a file, which change when it is saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileVersion {
    length: u64,
    modified: Option<SystemTime>,
}

impl FileVersion {
    /// Reads the current version of the file, or `None` when it cannot be read.
    pub fn of(path: &std::path::Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_size_stays_within_bounds_and_keeps_the_shape() {
        assert_eq!(preferred_content_size(2400.0, 800.0), (1200.0, 400.0));
        assert_eq!(preferred_content_size(100.0, 50.0), (320.0, 240.0));
        assert_eq!(preferred_content_size(640.0, 480.0), (640.0, 480.0));
    }

    #[test]
    fn fitting_shows_all_of_the_media() {
        assert_eq!(fit_magnification((400.0, 300.0), (800.0, 300.0)), 0.5);
        assert_eq!(fit_magnification((400.0, 300.0), (0.0, 0.0)), 1.0);
        assert_eq!(
            fit_magnification((1e9, 1e9), (1.0, 1.0)),
            MAXIMUM_MAGNIFICATION
        );
    }

    #[test]
    fn titles_show_the_rounded_size() {
        assert_eq!(window_title("a.png", (10.4, 20.6)), "a.png (10x21)");
    }
}
