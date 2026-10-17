/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [mime](https://crates.io/crates/mime) crate

#![allow(missing_docs)]
#![cfg_attr(not(test), no_std)]

use core::fmt::{self, Display, Formatter};

// MARK: Mime
/// A MIME type
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mime {
    type_: &'static str,
    subtype: &'static str,
    suffix: Option<&'static str>,
}

impl Mime {
    /// Create a new `Mime` instance
    pub const fn new(
        type_: &'static str,
        subtype: &'static str,
        suffix: Option<&'static str>,
    ) -> Self {
        Self {
            type_,
            subtype,
            suffix,
        }
    }

    /// Type
    pub const fn type_(&self) -> &str {
        self.type_
    }

    /// Subtype
    pub const fn subtype(&self) -> &str {
        self.subtype
    }

    /// Suffix
    pub const fn suffix(&self) -> Option<&str> {
        self.suffix
    }
}

impl Display for Mime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.type_, self.subtype)?;
        if let Some(suffix) = self.suffix {
            write!(f, "+{suffix}")?;
        }
        Ok(())
    }
}

// MARK: Common MIME types
pub const APPLICATION_GZIP: Mime = Mime::new("application", "gzip", None);
pub const APPLICATION_JAVASCRIPT: Mime = Mime::new("application", "javascript", None);
pub const APPLICATION_JSON: Mime = Mime::new("application", "json", None);
pub const APPLICATION_MANIFEST_JSON: Mime = Mime::new("application", "manifest+json", None);
pub const APPLICATION_OCTET_STREAM: Mime = Mime::new("application", "octet-stream", None);
pub const APPLICATION_PDF: Mime = Mime::new("application", "pdf", None);
pub const APPLICATION_WASM: Mime = Mime::new("application", "wasm", None);
pub const APPLICATION_X_TAR: Mime = Mime::new("application", "x-tar", None);
pub const APPLICATION_YAML: Mime = Mime::new("application", "yaml", None);
pub const APPLICATION_ZIP: Mime = Mime::new("application", "zip", None);

pub const AUDIO_AAC: Mime = Mime::new("audio", "aac", None);
pub const AUDIO_FLAC: Mime = Mime::new("audio", "flac", None);
pub const AUDIO_MPEG: Mime = Mime::new("audio", "mpeg", None);
pub const AUDIO_OGG: Mime = Mime::new("audio", "ogg", None);
pub const AUDIO_OPUS: Mime = Mime::new("audio", "opus", None);
pub const AUDIO_WAV: Mime = Mime::new("audio", "wav", None);

pub const FONT_OTF: Mime = Mime::new("font", "otf", None);
pub const FONT_TTF: Mime = Mime::new("font", "ttf", None);
pub const FONT_WOFF: Mime = Mime::new("font", "woff", None);
pub const FONT_WOFF2: Mime = Mime::new("font", "woff2", None);

pub const IMAGE_AVIF: Mime = Mime::new("image", "avif", None);
pub const IMAGE_BMP: Mime = Mime::new("image", "bmp", None);
pub const IMAGE_GIF: Mime = Mime::new("image", "gif", None);
pub const IMAGE_JPEG: Mime = Mime::new("image", "jpeg", None);
pub const IMAGE_PNG: Mime = Mime::new("image", "png", None);
pub const IMAGE_SVG: Mime = Mime::new("image", "svg", Some("xml"));
pub const IMAGE_TIFF: Mime = Mime::new("image", "tiff", None);
pub const IMAGE_WEBP: Mime = Mime::new("image", "webp", None);
pub const IMAGE_X_ICON: Mime = Mime::new("image", "x-icon", None);

pub const TEXT_CSV: Mime = Mime::new("text", "csv", None);
pub const TEXT_CSS: Mime = Mime::new("text", "css", None);
pub const TEXT_HTML: Mime = Mime::new("text", "html", None);
pub const TEXT_MARKDOWN: Mime = Mime::new("text", "markdown", None);
pub const TEXT_PLAIN: Mime = Mime::new("text", "plain", None);
pub const TEXT_XML: Mime = Mime::new("text", "xml", None);

pub const VIDEO_MP4: Mime = Mime::new("video", "mp4", None);
pub const VIDEO_OGG: Mime = Mime::new("video", "ogg", None);
pub const VIDEO_WEBM: Mime = Mime::new("video", "webm", None);

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_to_string() {
        let mime = Mime::new("text", "html", None);
        assert_eq!(mime.to_string(), "text/html");

        let mime = Mime::new("application", "vnd.api", Some("json"));
        assert_eq!(mime.to_string(), "application/vnd.api+json");

        assert_eq!(
            APPLICATION_OCTET_STREAM.to_string(),
            "application/octet-stream"
        );
        assert_eq!(IMAGE_SVG.to_string(), "image/svg+xml");
        assert_eq!(TEXT_XML.to_string(), "text/xml");
    }
}
