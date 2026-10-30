/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [mime_guess](https://crates.io/crates/mime_guess) crate
//!
//! Only a subset of the upstream extension table is included, the `rev-mappings` feature exists for compatibility only.

use std::ffi::OsStr;
use std::path::Path;

use mime::Mime;

/// Guess the MIME type of a file (real or otherwise) with the given extension.
pub fn from_ext(ext: &str) -> MimeGuess {
    MimeGuess::from_ext(ext)
}

/// Guess the MIME type of `path` by its extension.
pub fn from_path<P: AsRef<Path>>(path: P) -> MimeGuess {
    MimeGuess::from_path(path)
}

// MARK: MimeGuess
/// A "guess" of the MIME/Media Type(s) of an extension or path as one or more `Mime` instances.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MimeGuess(&'static [&'static str]);

impl MimeGuess {
    /// Guess the MIME type of a file (real or otherwise) with the given extension.
    ///
    /// The search is case-insensitive.
    pub fn from_ext(ext: &str) -> MimeGuess {
        MimeGuess(
            MIME_TYPES
                .iter()
                .find(|(known, _)| known.eq_ignore_ascii_case(ext))
                .map_or(&[], |(_, mimes)| *mimes),
        )
    }

    /// Guess the MIME type of `path` by its extension. **No disk access is performed.**
    ///
    /// The search is case-insensitive.
    pub fn from_path<P: AsRef<Path>>(path: P) -> MimeGuess {
        path.as_ref()
            .extension()
            .and_then(OsStr::to_str)
            .map_or(MimeGuess(&[]), Self::from_ext)
    }

    /// Get the first guessed `Mime`, if applicable.
    pub fn first(&self) -> Option<Mime> {
        self.first_raw()
            .map(|mime| mime.parse().expect("Static MIME types should be valid"))
    }

    /// Get the first guessed Media Type as a string, if applicable.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn first_raw(&self) -> Option<&'static str> {
        self.0.first().copied()
    }

    /// Get the first guessed `Mime`, or if the guess is empty, return `application/octet-stream` instead.
    pub fn first_or_octet_stream(&self) -> Mime {
        self.first().unwrap_or(mime::APPLICATION_OCTET_STREAM)
    }
}

// MARK: Mime types
/// Subset of the upstream `mime_guess` 2.0.5 extension table.
static MIME_TYPES: &[(&str, &[&str])] = &[
    ("aac", &["audio/aac"]),
    ("avif", &["image/avif"]),
    ("bmp", &["image/bmp"]),
    ("css", &["text/css"]),
    ("csv", &["text/csv"]),
    ("flac", &["audio/flac"]),
    ("gif", &["image/gif"]),
    ("gz", &["application/gzip", "application/x-gzip"]),
    ("htm", &["text/html"]),
    ("html", &["text/html"]),
    ("ico", &["image/x-icon"]),
    ("jpeg", &["image/jpeg"]),
    ("jpg", &["image/jpeg"]),
    ("js", &["text/javascript"]),
    ("json", &["application/json"]),
    ("m4a", &["audio/m4a"]),
    ("markdown", &["text/markdown"]),
    ("md", &["text/markdown", "text/x-markdown"]),
    ("mjs", &["application/javascript"]),
    ("mp3", &["audio/mpeg"]),
    ("mp4", &["video/mp4"]),
    ("ogg", &["audio/ogg"]),
    ("ogv", &["video/ogg"]),
    ("opus", &["audio/ogg"]),
    ("otf", &["application/font-sfnt"]),
    ("pdf", &["application/pdf"]),
    ("png", &["image/png"]),
    ("svg", &["image/svg+xml"]),
    ("tar", &["application/x-tar"]),
    ("tif", &["image/tiff"]),
    ("tiff", &["image/tiff"]),
    (
        "ttf",
        &[
            "font/ttf",
            "application/x-font-ttf",
            "application/font-sfnt",
        ],
    ),
    ("txt", &["text/plain"]),
    ("wasm", &["application/wasm"]),
    ("wav", &["audio/wav"]),
    ("webm", &["video/webm"]),
    ("webmanifest", &["application/manifest+json"]),
    ("webp", &["image/webp"]),
    ("woff", &["application/font-woff"]),
    ("woff2", &["font/woff2"]),
    ("xml", &["text/xml"]),
    ("yaml", &["text/x-yaml"]),
    ("yml", &["text/x-yaml"]),
    ("zip", &["application/zip"]),
];

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_from_ext() {
        assert_eq!(from_ext("png").first_or_octet_stream(), mime::IMAGE_PNG);
        assert_eq!(from_ext("PNG").first_or_octet_stream(), mime::IMAGE_PNG);
        assert_eq!(from_ext("md").first_raw(), Some("text/markdown"));
        assert_eq!(from_ext("").first(), None);
        assert_eq!(
            MimeGuess::from_ext("unknown").first_or_octet_stream(),
            mime::APPLICATION_OCTET_STREAM
        );
    }

    #[test]
    fn test_table_valid() {
        for (ext, mimes) in MIME_TYPES {
            assert_eq!(*ext, ext.to_ascii_lowercase());
            for mime in *mimes {
                assert!(mime.parse::<Mime>().is_ok(), "{ext}: {mime}");
            }
        }
    }

    #[test]
    #[rustfmt::skip]
    fn test_guess() {
        // Text / markup
        assert_eq!(from_path("index.html").first_or_octet_stream(), "text/html");
        assert_eq!(from_path("INDEX.HTM").first_or_octet_stream(), "text/html");
        assert_eq!(from_path("style.css").first_or_octet_stream(), "text/css");
        assert_eq!(from_path("script.js").first_or_octet_stream(), "text/javascript");
        assert_eq!(from_path("module.mjs").first_or_octet_stream(), "application/javascript");
        assert_eq!(from_path("data.json").first_or_octet_stream(), "application/json");
        assert_eq!(from_path("feed.xml").first_or_octet_stream(), "text/xml");
        assert_eq!(from_path("readme.txt").first_or_octet_stream(), "text/plain");
        assert_eq!(from_path("data.csv").first_or_octet_stream(), "text/csv");
        assert_eq!(from_path("docs.md").first_or_octet_stream(), "text/markdown");
        assert_eq!(from_path("docs.markdown").first_or_octet_stream(), "text/markdown");
        assert_eq!(from_path("config.yaml").first_or_octet_stream(), "text/x-yaml");
        assert_eq!(from_path("config.yml").first_or_octet_stream(), "text/x-yaml");

        // Web
        assert_eq!(from_path("app.wasm").first_or_octet_stream(), "application/wasm");
        assert_eq!(from_path("app.webmanifest").first_or_octet_stream(), "application/manifest+json");

        // Images
        assert_eq!(from_path("image.png").first_or_octet_stream(), "image/png");
        assert_eq!(from_path("photo.jpg").first_or_octet_stream(), "image/jpeg");
        assert_eq!(from_path("photo.JPEG").first_or_octet_stream(), "image/jpeg");
        assert_eq!(from_path("anim.gif").first_or_octet_stream(), "image/gif");
        assert_eq!(from_path("icon.svg").first_or_octet_stream(), mime::IMAGE_SVG);
        assert_eq!(from_path("image.webp").first_or_octet_stream(), "image/webp");
        assert_eq!(from_path("favicon.ico").first_or_octet_stream(), "image/x-icon");
        assert_eq!(from_path("image.avif").first_or_octet_stream(), "image/avif");
        assert_eq!(from_path("image.bmp").first_or_octet_stream(), "image/bmp");
        assert_eq!(from_path("image.tiff").first_or_octet_stream(), "image/tiff");
        assert_eq!(from_path("image.tif").first_or_octet_stream(), "image/tiff");

        // Fonts
        assert_eq!(from_path("font.woff").first_or_octet_stream(), "application/font-woff");
        assert_eq!(from_path("font.woff2").first_or_octet_stream(), "font/woff2");
        assert_eq!(from_path("font.ttf").first_or_octet_stream(), "font/ttf");
        assert_eq!(from_path("font.otf").first_or_octet_stream(), "application/font-sfnt");

        // Audio
        assert_eq!(from_path("audio.mp3").first_or_octet_stream(), "audio/mpeg");
        assert_eq!(from_path("audio.wav").first_or_octet_stream(), "audio/wav");
        assert_eq!(from_path("audio.ogg").first_or_octet_stream(), "audio/ogg");
        assert_eq!(from_path("audio.opus").first_or_octet_stream(), "audio/ogg");
        assert_eq!(from_path("audio.flac").first_or_octet_stream(), "audio/flac");
        assert_eq!(from_path("audio.m4a").first_or_octet_stream(), "audio/m4a");
        assert_eq!(from_path("audio.aac").first_or_octet_stream(), "audio/aac");

        // Video
        assert_eq!(from_path("video.mp4").first_or_octet_stream(), "video/mp4");
        assert_eq!(from_path("video.webm").first_or_octet_stream(), "video/webm");
        assert_eq!(from_path("video.ogv").first_or_octet_stream(), "video/ogg");

        // Documents & archives
        assert_eq!(from_path("doc.pdf").first_or_octet_stream(), mime::APPLICATION_PDF);
        assert_eq!(from_path("archive.zip").first_or_octet_stream(), "application/zip");
        assert_eq!(from_path("archive.gz").first_or_octet_stream(), "application/gzip");
        assert_eq!(from_path("archive.tar").first_or_octet_stream(), "application/x-tar");

        // Fallback
        assert_eq!(from_path("unknown.xyz").first_or_octet_stream(), mime::APPLICATION_OCTET_STREAM);
        assert_eq!(from_path("no-extension").first(), None);
    }
}
