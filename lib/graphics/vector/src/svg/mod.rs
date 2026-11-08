/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bcanvas::OffscreenCanvas;
use xmlparser::{Token, Tokenizer};

use crate::VectorDecodeError;
use crate::svg::render::Decoder;

mod geometry;
mod path;
mod render;
mod style;
mod values;
mod xml;

const SVG_NS: &str = "http://www.w3.org/2000/svg";
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ELEMENTS: usize = 1_000_000;
const MAX_DEPTH: usize = 256;
const MAX_ITEMS: usize = 1_000_000;
const MAX_ATTRIBUTES: usize = 1_000_000;
const MAX_STYLESHEET_BYTES: usize = 4 * 1024 * 1024;
const MAX_CSS_RULES: usize = 65_536;
const MAX_CSS_SELECTORS: usize = 65_536;
const MAX_CSS_DECLARATIONS: usize = 262_144;

pub(crate) fn is_svg(data: &[u8]) -> bool {
    let Ok(source) = std::str::from_utf8(data) else {
        return data.windows(4).take(1024).any(|window| window == b"<svg");
    };
    let source = source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .trim_start();
    for token in Tokenizer::from(source) {
        match token {
            Ok(Token::ElementStart { local, .. }) => return local.as_str() == "svg",
            Err(_) => break,
            _ => {}
        }
    }
    source.find("<svg").is_some_and(|index| index < 1024)
}

pub(crate) fn decode(data: &[u8]) -> Result<OffscreenCanvas, VectorDecodeError> {
    Decoder::new(data)?.decode()
}

#[cfg(test)]
mod tests;
