/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [mime_guess](https://crates.io/crates/mime_guess) crate
//!
//! Disable default features to use extension lookup with `no_std` and `alloc`. Path helpers require `std`.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

use alloc::string::{String, ToString};
#[cfg(feature = "std")]
use std::path::Path;

use mime::Mime;

/// Create a new `MimeGuess` from a file path
#[cfg(feature = "std")]
pub fn from_path(path: impl AsRef<Path>) -> MimeGuess {
    MimeGuess::from_path(path.as_ref())
}

/// Create a new `MimeGuess` from a file extension without a leading dot.
pub fn from_ext(extension: &str) -> MimeGuess {
    MimeGuess::from_ext(extension)
}

/// MimeGuess
pub struct MimeGuess {
    extension: String,
}

impl MimeGuess {
    /// Create a new `MimeGuess` from a file extension without a leading dot.
    pub fn from_ext(extension: &str) -> Self {
        Self {
            extension: extension.to_string(),
        }
    }

    /// Create a new `MimeGuess` from a file path
    #[cfg(feature = "std")]
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let extension = path
            .as_ref()
            .extension()
            .and_then(|ext| ext.to_str())
            .map_or_else(String::new, |ext| ext.to_string());
        Self { extension }
    }

    /// Guess MIME type or return `application/octet-stream` if unknown
    pub fn first_or_octet_stream(&self) -> Mime {
        match self.extension.as_str() {
            // Text
            "html" | "htm" => mime::TEXT_HTML,
            "css" => mime::TEXT_CSS,
            "js" | "mjs" => mime::APPLICATION_JAVASCRIPT,
            "json" => mime::APPLICATION_JSON,
            "xml" => mime::TEXT_XML,
            "txt" => mime::TEXT_PLAIN,
            "csv" => mime::TEXT_CSV,
            "md" | "markdown" => mime::TEXT_MARKDOWN,
            "yaml" | "yml" => mime::APPLICATION_YAML,
            // Web
            "wasm" => mime::APPLICATION_WASM,
            "webmanifest" => mime::APPLICATION_MANIFEST_JSON,
            // Images
            "png" => mime::IMAGE_PNG,
            "jpg" | "jpeg" => mime::IMAGE_JPEG,
            "gif" => mime::IMAGE_GIF,
            "svg" => mime::IMAGE_SVG,
            "webp" => mime::IMAGE_WEBP,
            "ico" => mime::IMAGE_X_ICON,
            "avif" => mime::IMAGE_AVIF,
            "bmp" => mime::IMAGE_BMP,
            "tiff" | "tif" => mime::IMAGE_TIFF,
            // Fonts
            "woff" => mime::FONT_WOFF,
            "woff2" => mime::FONT_WOFF2,
            "ttf" => mime::FONT_TTF,
            "otf" => mime::FONT_OTF,
            // Audio
            "mp3" => mime::AUDIO_MPEG,
            "wav" => mime::AUDIO_WAV,
            "ogg" => mime::AUDIO_OGG,
            "opus" => mime::AUDIO_OPUS,
            "flac" => mime::AUDIO_FLAC,
            "m4a" | "aac" => mime::AUDIO_AAC,
            // Video
            "mp4" => mime::VIDEO_MP4,
            "webm" => mime::VIDEO_WEBM,
            "ogv" => mime::VIDEO_OGG,
            // Documents & archives
            "pdf" => mime::APPLICATION_PDF,
            "zip" => mime::APPLICATION_ZIP,
            "gz" => mime::APPLICATION_GZIP,
            "tar" => mime::APPLICATION_X_TAR,
            _ => mime::APPLICATION_OCTET_STREAM,
        }
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_from_ext() {
        assert_eq!(from_ext("png").first_or_octet_stream(), mime::IMAGE_PNG);
        assert_eq!(
            MimeGuess::from_ext("unknown").first_or_octet_stream(),
            mime::APPLICATION_OCTET_STREAM
        );
    }

    #[test]
    #[rustfmt::skip]
    #[cfg(feature = "std")]
    fn test_guess() {
        // Text / markup
        assert_eq!(MimeGuess::from_path("index.html").first_or_octet_stream(), mime::TEXT_HTML);
        assert_eq!(MimeGuess::from_path("index.htm").first_or_octet_stream(), mime::TEXT_HTML);
        assert_eq!(MimeGuess::from_path("style.css").first_or_octet_stream(), mime::TEXT_CSS);
        assert_eq!(MimeGuess::from_path("script.js").first_or_octet_stream(), mime::APPLICATION_JAVASCRIPT);
        assert_eq!(MimeGuess::from_path("module.mjs").first_or_octet_stream(), mime::APPLICATION_JAVASCRIPT);
        assert_eq!(MimeGuess::from_path("data.json").first_or_octet_stream(), mime::APPLICATION_JSON);
        assert_eq!(MimeGuess::from_path("feed.xml").first_or_octet_stream(), mime::TEXT_XML);
        assert_eq!(MimeGuess::from_path("readme.txt").first_or_octet_stream(), mime::TEXT_PLAIN);
        assert_eq!(MimeGuess::from_path("data.csv").first_or_octet_stream(), mime::TEXT_CSV);
        assert_eq!(MimeGuess::from_path("docs.md").first_or_octet_stream(), mime::TEXT_MARKDOWN);
        assert_eq!(MimeGuess::from_path("docs.markdown").first_or_octet_stream(), mime::TEXT_MARKDOWN);
        assert_eq!(MimeGuess::from_path("config.yaml").first_or_octet_stream(), mime::APPLICATION_YAML);
        assert_eq!(MimeGuess::from_path("config.yml").first_or_octet_stream(), mime::APPLICATION_YAML);

        // Web
        assert_eq!(MimeGuess::from_path("app.wasm").first_or_octet_stream(), mime::APPLICATION_WASM);
        assert_eq!(MimeGuess::from_path("app.webmanifest").first_or_octet_stream(), mime::APPLICATION_MANIFEST_JSON);

        // Images
        assert_eq!(MimeGuess::from_path("image.png").first_or_octet_stream(), mime::IMAGE_PNG);
        assert_eq!(MimeGuess::from_path("photo.jpg").first_or_octet_stream(), mime::IMAGE_JPEG);
        assert_eq!(MimeGuess::from_path("photo.jpeg").first_or_octet_stream(), mime::IMAGE_JPEG);
        assert_eq!(MimeGuess::from_path("anim.gif").first_or_octet_stream(), mime::IMAGE_GIF);
        assert_eq!(MimeGuess::from_path("icon.svg").first_or_octet_stream(), mime::IMAGE_SVG);
        assert_eq!(MimeGuess::from_path("image.webp").first_or_octet_stream(), mime::IMAGE_WEBP);
        assert_eq!(MimeGuess::from_path("favicon.ico").first_or_octet_stream(), mime::IMAGE_X_ICON);
        assert_eq!(MimeGuess::from_path("image.avif").first_or_octet_stream(), mime::IMAGE_AVIF);
        assert_eq!(MimeGuess::from_path("image.bmp").first_or_octet_stream(), mime::IMAGE_BMP);
        assert_eq!(MimeGuess::from_path("image.tiff").first_or_octet_stream(), mime::IMAGE_TIFF);
        assert_eq!(MimeGuess::from_path("image.tif").first_or_octet_stream(), mime::IMAGE_TIFF);

        // Fonts
        assert_eq!(MimeGuess::from_path("font.woff").first_or_octet_stream(), mime::FONT_WOFF);
        assert_eq!(MimeGuess::from_path("font.woff2").first_or_octet_stream(), mime::FONT_WOFF2);
        assert_eq!(MimeGuess::from_path("font.ttf").first_or_octet_stream(), mime::FONT_TTF);
        assert_eq!(MimeGuess::from_path("font.otf").first_or_octet_stream(), mime::FONT_OTF);

        // Audio
        assert_eq!(MimeGuess::from_path("audio.mp3").first_or_octet_stream(), mime::AUDIO_MPEG);
        assert_eq!(MimeGuess::from_path("audio.wav").first_or_octet_stream(), mime::AUDIO_WAV);
        assert_eq!(MimeGuess::from_path("audio.ogg").first_or_octet_stream(), mime::AUDIO_OGG);
        assert_eq!(MimeGuess::from_path("audio.opus").first_or_octet_stream(), mime::AUDIO_OPUS);
        assert_eq!(MimeGuess::from_path("audio.flac").first_or_octet_stream(), mime::AUDIO_FLAC);
        assert_eq!(MimeGuess::from_path("audio.aac").first_or_octet_stream(), mime::AUDIO_AAC);

        // Video
        assert_eq!(MimeGuess::from_path("video.mp4").first_or_octet_stream(), mime::VIDEO_MP4);
        assert_eq!(MimeGuess::from_path("video.webm").first_or_octet_stream(), mime::VIDEO_WEBM);
        assert_eq!(MimeGuess::from_path("video.ogv").first_or_octet_stream(), mime::VIDEO_OGG);

        // Documents & archives
        assert_eq!(MimeGuess::from_path("doc.pdf").first_or_octet_stream(), mime::APPLICATION_PDF);
        assert_eq!(MimeGuess::from_path("archive.zip").first_or_octet_stream(), mime::APPLICATION_ZIP);
        assert_eq!(MimeGuess::from_path("archive.gz").first_or_octet_stream(), mime::APPLICATION_GZIP);
        assert_eq!(MimeGuess::from_path("archive.tar").first_or_octet_stream(), mime::APPLICATION_X_TAR);

        // Fallback
        assert_eq!(MimeGuess::from_path("unknown.xyz").first_or_octet_stream(), mime::APPLICATION_OCTET_STREAM);
    }
}
