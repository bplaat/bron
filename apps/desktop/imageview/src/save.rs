/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Save As dialog of a window: the cross-platform file dialog, with the native controls of
//! the platform for the format, the JPEG quality and the size vectors are drawn at as bitmaps.

use std::path::{Path, PathBuf};

use bwindow::{FileDialog, Window};
use imageview_core::edit::{EXPORT_SCALES, is_jpeg};

use crate::platforms::SaveOptions;

/// What a save dialog asks for.
pub(crate) struct SaveRequest<'a> {
    /// The folder it starts in.
    pub(crate) directory: Option<&'a Path>,
    /// The file name it starts with.
    pub(crate) name: &'a str,
    /// The JPEG quality it starts with.
    pub(crate) quality: u8,
    /// The formats to choose from, as a name and file name extension.
    pub(crate) formats: &'a [(&'static str, &'static str)],
    /// The natural size of a vector, which is drawn at a chosen scale.
    pub(crate) size: Option<(f64, f64)>,
}

/// What a save dialog chose: the path, the JPEG quality and the scale of a vector.
pub(crate) struct SaveChoice {
    pub(crate) path: PathBuf,
    pub(crate) quality: u8,
    pub(crate) scale: f64,
}

/// Asks where to save an image in a dialog of a window, in which format and with which JPEG
/// quality, and at which size for a vector.
pub(crate) fn ask_save(window: &Window, request: &SaveRequest<'_>) -> Option<SaveChoice> {
    let mut options = None;
    let mut dialog = FileDialog::new()
        .parent(window)
        .title("Save As")
        .file_name(request.name)
        .customize(|native| options = SaveOptions::new(native, request));
    if let Some(directory) = request
        .directory
        .filter(|directory| !directory.as_os_str().is_empty())
    {
        dialog = dialog.directory(directory);
    }
    let path = dialog.save_file()?;
    let options = options?;
    Some(SaveChoice {
        path,
        quality: options.quality(),
        scale: options.scale(),
    })
}

/// Whether files with an extension can have a JPEG quality.
pub(crate) fn has_quality(extension: &str) -> bool {
    is_jpeg(Path::new(&format!("a.{extension}")))
}

/// The index of the format of a file name, which formats with the same files share.
pub(crate) fn format_of(name: &str, formats: &[(&str, &str)]) -> usize {
    let extension = Path::new(name)
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase());
    formats
        .iter()
        .position(|(_, candidate)| {
            extension.as_deref() == Some(candidate)
                || (is_jpeg(Path::new(name)) && has_quality(candidate))
        })
        .unwrap_or(0)
}

/// A file name with the extension of a format instead of its own.
#[cfg_attr(
    windows,
    allow(dead_code, reason = "Windows dialogs change the extension themselves")
)]
pub(crate) fn with_extension(name: &str, extension: &str) -> String {
    let stem = Path::new(name).file_stem().map_or_else(
        || name.to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    format!("{stem}.{extension}")
}

/// The names of the scales vectors are drawn at, with their pixel sizes.
pub(crate) fn scale_titles((width, height): (f64, f64)) -> Vec<String> {
    EXPORT_SCALES
        .iter()
        .map(|scale| {
            format!(
                "{}% ({} \u{d7} {})",
                (scale * 100.0).round(),
                (width * scale).round().max(1.0),
                (height * scale).round().max(1.0)
            )
        })
        .collect()
}

/// The index of the scale vectors are drawn at by default, their natural size.
pub(crate) fn natural_scale() -> usize {
    EXPORT_SCALES
        .iter()
        .position(|scale| *scale == 1.0)
        .unwrap_or(0)
}

/// The scale at an index of the names, or the natural size.
pub(crate) fn scale_at(index: usize) -> f64 {
    EXPORT_SCALES.get(index).copied().unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMATS: &[(&str, &str)] = &[("PNG", "png"), ("JPEG", "jpg"), ("BMP", "bmp")];

    #[test]
    fn formats_follow_file_names() {
        assert_eq!(format_of("photo.bmp", FORMATS), 2);
        assert_eq!(format_of("photo.JPG", FORMATS), 1);
        assert_eq!(format_of("photo.jpeg", FORMATS), 1);
        assert_eq!(format_of("photo.webp", FORMATS), 0);
        assert_eq!(format_of("photo", FORMATS), 0);
    }

    #[test]
    fn extensions_change_only_the_end_of_a_name() {
        assert_eq!(with_extension("a.b.png", "jpg"), "a.b.jpg");
        assert_eq!(with_extension("photo", "png"), "photo.png");
        assert!(has_quality("jpg") && !has_quality("png"));
    }

    #[test]
    fn scales_show_pixel_sizes() {
        let titles = scale_titles((100.0, 50.0));
        let natural = natural_scale();
        assert_eq!(titles[natural], "100% (100 \u{d7} 50)");
        assert_eq!(scale_at(natural), 1.0);
        assert_eq!(scale_at(usize::MAX), 1.0);
    }
}
