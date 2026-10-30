/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [mime](https://crates.io/crates/mime) crate
//!
//! Only the essence (`type/subtype+suffix`) is supported, parsing a MIME type with parameters returns an error.

use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::str::FromStr;

// MARK: Mime
/// A parsed mime or media type.
#[derive(Clone, PartialEq, Eq)]
pub struct Mime {
    source: Cow<'static, str>,
}

impl PartialEq<&str> for Mime {
    fn eq(&self, other: &&str) -> bool {
        self.source.eq_ignore_ascii_case(other)
    }
}

impl PartialEq<Mime> for &str {
    fn eq(&self, other: &Mime) -> bool {
        other == self
    }
}

impl AsRef<str> for Mime {
    fn as_ref(&self) -> &str {
        &self.source
    }
}

impl fmt::Debug for Mime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.source, f)
    }
}

impl fmt::Display for Mime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.source, f)
    }
}

impl FromStr for Mime {
    type Err = FromStrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid_token = |pos: usize, byte: u8| FromStrError {
            inner: ParseError::InvalidToken { pos, byte },
        };
        let mut bytes = s.bytes().enumerate();

        // Top level type
        loop {
            match bytes.next() {
                Some((_, byte)) if is_token(byte) => (),
                Some((pos, b'/')) if pos > 0 => break,
                Some((pos, byte)) => return Err(invalid_token(pos, byte)),
                None => {
                    return Err(FromStrError {
                        inner: ParseError::MissingSlash,
                    });
                }
            }
        }

        // Subtype with optional suffix
        for (pos, byte) in bytes {
            if !is_token(byte) {
                return Err(invalid_token(pos, byte));
            }
        }
        Ok(Self {
            source: Cow::Owned(s.to_ascii_lowercase()),
        })
    }
}

const fn is_token(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

// MARK: FromStrError
/// An error when parsing a `Mime` from a string.
#[derive(Debug)]
pub struct FromStrError {
    inner: ParseError,
}

impl fmt::Display for FromStrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mime parse error: {}", self.inner)
    }
}

impl Error for FromStrError {}

#[derive(Debug)]
enum ParseError {
    MissingSlash,
    InvalidToken { pos: usize, byte: u8 },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSlash => {
                f.write_str("a slash (/) was missing between the type and subtype")
            }
            Self::InvalidToken { pos, byte } => {
                write!(
                    f,
                    "an invalid token was encountered, {byte:X} at position {pos}"
                )
            }
        }
    }
}

// MARK: Constants
macro_rules! mimes {
    ($($id:ident, $source:expr;)*) => {
        $(
            #[doc = concat!("`", $source, "`")]
            pub const $id: Mime = Mime {
                source: Cow::Borrowed($source),
            };
        )*
    };
}

mimes! {
    TEXT_PLAIN, "text/plain";
    TEXT_HTML, "text/html";
    TEXT_CSS, "text/css";
    TEXT_XML, "text/xml";
    TEXT_CSV, "text/csv";

    IMAGE_JPEG, "image/jpeg";
    IMAGE_GIF, "image/gif";
    IMAGE_PNG, "image/png";
    IMAGE_BMP, "image/bmp";
    IMAGE_SVG, "image/svg+xml";

    FONT_WOFF, "font/woff";
    FONT_WOFF2, "font/woff2";

    APPLICATION_JSON, "application/json";
    APPLICATION_JAVASCRIPT, "application/javascript";
    APPLICATION_OCTET_STREAM, "application/octet-stream";
    APPLICATION_PDF, "application/pdf";
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_to_string() {
        assert_eq!(TEXT_HTML.to_string(), "text/html");
        assert_eq!(IMAGE_SVG.to_string(), "image/svg+xml");
        assert_eq!(
            APPLICATION_OCTET_STREAM.to_string(),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_parse() {
        let mime = "Application/Vnd.Api+JSON"
            .parse::<Mime>()
            .expect("valid mime");
        assert_eq!(mime.to_string(), "application/vnd.api+json");
        assert_eq!(mime, "application/vnd.api+json");
        assert_eq!("text/html".parse::<Mime>().ok(), Some(TEXT_HTML));
        assert_eq!(TEXT_HTML, "TEXT/HTML");

        assert!("text".parse::<Mime>().is_err());
        assert!("/html".parse::<Mime>().is_err());
        assert!("text/ html".parse::<Mime>().is_err());
        assert!("text/html; charset=utf-8".parse::<Mime>().is_err());
    }
}
