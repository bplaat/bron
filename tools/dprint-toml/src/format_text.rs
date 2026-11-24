/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt;
use std::path::Path;

use dprint_core::configuration::resolve_new_line_kind;
use dprint_core::formatting::{Arena, PrintOptions};

use crate::configuration::Configuration;
use crate::generation::generate;
use crate::{cargo, parser, sorting};

/// The input could not be parsed as TOML.
///
/// The message is a pre-rendered diagnostic that includes the offending line and a caret pointing
/// at the position of the error.
#[derive(Debug)]
pub(crate) struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

/// Formats a file in the given arena, returning `None` when it is already formatted.
pub(crate) fn format_text(
    arena: &Arena,
    file_path: &Path,
    text: &str,
    config: &Configuration,
) -> Result<Option<String>, ParseError> {
    let stripped = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let root = parse(arena, stripped)?;
    sorting::apply_sorting(root, config);
    // After the general sorting, so that a Cargo.toml keeps its conventional order
    if config.cargo_apply_conventions && cargo::is_cargo_toml_file(file_path) {
        cargo::apply_cargo_toml_conventions(root);
    }

    let print_options = PrintOptions {
        indent_width: config.indent_width,
        max_width: config.line_width,
        use_tabs: config.use_tabs,
        new_line_text: resolve_new_line_kind(stripped, config.new_line_kind),
    };
    let result = dprint_core::formatting::print(generate(arena, root, config), print_options);
    Ok((result != text).then_some(result))
}

fn parse<'a>(arena: &'a Arena, text: &'a str) -> Result<&'a mut crate::ast::Root<'a>, ParseError> {
    parser::parse(arena.bump(), text).map_err(|err| {
        ParseError(
            dprint_core::formatting::utils::string_utils::format_diagnostic(
                Some(highlight_range(&err, text)),
                &err.message,
                text,
            ),
        )
    })
}

/// The range to underline beneath the offending line.
///
/// The diagnostic only underlines within a single line, and draws nothing for an empty range or
/// one that runs past the end of its line. Keeping the range on its own line, and at least one
/// character wide, means every error gets a caret.
fn highlight_range(err: &parser::SyntaxError, text: &str) -> (usize, usize) {
    let line_end = text[err.span.start..]
        .find('\n')
        .map_or(text.len(), |i| err.span.start + i);
    let end = err.span.end.min(line_end);
    if end > err.span.start {
        return (err.span.start, end);
    }
    // Point at the character the empty range sits before, or after at the end of the input
    let start = err.span.start.min(text.len());
    match text[start..].chars().next() {
        Some(c) => (start, start + c.len_utf8()),
        None => (
            text[..start]
                .chars()
                .next_back()
                .map_or(start, |c| start - c.len_utf8()),
            start,
        ),
    }
}
