/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::borrow::Cow;
use std::ops::{Range, RangeInclusive};
use std::path::Path;

use dprint_core::configuration::{NewLineKind, resolve_new_line_kind};
use dprint_core::formatting::ArenaPool;

use crate::ast::{Root, RootItem};
use crate::configuration::Configuration;
use crate::format_text::{ParseError, Reorder, format_text, parse, print, process_node, strip_bom};
use crate::sorting::section_end;

/// Formats only the part of the text within the provided byte range.
///
/// The range is widened to the lines of the entries, table headers and comments it touches at the
/// top level of the file, and the text outside of those is left as it was, including the blank
/// lines above and below them, the line endings and any byte order mark. An empty range is a
/// cursor, which touches whatever is on its line.
///
/// Entries are only reordered (ex. by the `Cargo.toml` conventions or the `sortKeys` option) when
/// all of the lines that would move are touched. Cargo tables move during whole-file formatting.
/// What's within the value of a touched entry is sorted either way.
///
/// When formatting the rest of the file afterwards wouldn't then give what formatting all of it at
/// once does (ex. a table header and the first entry beneath it decide between them whether the
/// table's entries are indented, so changing the indentation of only one changes the answer), the
/// tables they are in are formatted instead. A range that touches both the first and last items
/// formats the whole file, which like whole-file formatting normalises the line endings and drops a
/// byte order mark. A range that only touches blank lines formats nothing.
pub(crate) fn format_text_range(
    arena_pool: &ArenaPool,
    file_path: &Path,
    text: &str,
    range: Range<usize>,
    config: &Configuration,
) -> Result<Option<String>, ParseError> {
    let body = strip_bom(text);
    let bom_len = text.len() - body.len();
    let end = range.end.saturating_sub(bom_len).min(body.len());
    let range = range.start.saturating_sub(bom_len).min(end)..end;
    // also what formats a file that only has blank lines, which has nothing to touch
    if range.start == 0 && range.end == body.len() {
        return format_text(&arena_pool.create_arena(), file_path, text, config);
    }

    let arena = arena_pool.create_arena();
    let root = parse(&arena, body)?;
    let starts = item_starts(root);
    let Some(touched) = find_touched(body, &starts, &range) else {
        return Ok(None);
    };
    // the blank lines at the start of the file and the newline at its end belong to no item, so
    // they are only formatted along with all of it
    if touched == (0..=starts.len() - 1) {
        return format_text(&arena_pool.create_arena(), file_path, text, config);
    }

    process_node(file_path, root, config, Reorder::Entries);
    let reorders = item_starts(root) != starts;
    let tables = widen_to_tables(root, touched.clone());
    let mut attempts = Vec::with_capacity(3);
    if holds_same_items(root, &starts, &touched) {
        attempts.push((true, touched.clone()));
    }
    if reorders {
        // lines outside of the touched ones would move, so the entries are left in their order
        attempts.push((false, touched));
    }
    attempts.push((!reorders, tables));

    let reordered = Formatted::new(print(&arena, root, body, config), arena_pool)?;
    let mut kept = None;
    for (reorder_entries, items) in attempts {
        let formatted = if reorder_entries {
            &reordered
        } else if let Some(kept) = &kept {
            kept
        } else {
            kept.insert(Formatted::new(
                format_body(file_path, body, config, false, arena_pool)?,
                arena_pool,
            )?)
        };
        if formatted.starts.len() != starts.len() {
            continue;
        }
        let original = lines_range(body, item_range(body, &starts, items.clone()));
        let replacement = lines_range(
            &formatted.text,
            item_range(&formatted.text, &formatted.starts, items),
        );
        let spliced = format!(
            "{}{}{}",
            &body[..original.start],
            with_file_new_lines(&formatted.text[replacement], body),
            &body[original.end..]
        );
        if format_body(file_path, &spliced, config, reorder_entries, arena_pool)
            .is_ok_and(|candidate| candidate == formatted.text)
        {
            let result = format!("{}{}", &text[..bom_len], spliced);
            return Ok(if result == text { None } else { Some(result) });
        }
    }
    Ok(None)
}

/// The file once formatted along with where each of its items starts.
struct Formatted {
    text: String,
    starts: Vec<usize>,
}

impl Formatted {
    fn new(text: String, arena_pool: &ArenaPool) -> Result<Self, ParseError> {
        let arena = arena_pool.create_arena();
        let starts = item_starts(parse(&arena, &text)?);
        Ok(Formatted { text, starts })
    }
}

fn format_body(
    file_path: &Path,
    body: &str,
    config: &Configuration,
    reorder_entries: bool,
    arena_pool: &ArenaPool,
) -> Result<String, ParseError> {
    let arena = arena_pool.create_arena();
    let root = parse(&arena, body)?;
    let reorder = if reorder_entries {
        Reorder::Entries
    } else {
        Reorder::Values
    };
    process_node(file_path, root, config, reorder);
    Ok(print(&arena, root, body, config))
}

/// Where each of the root's items starts in the text it was parsed from.
fn item_starts(root: &Root) -> Vec<usize> {
    root.items.iter().map(RootItem::start_in_source).collect()
}

/// The indexes of the items the range touches, if any.
///
/// A cursor touches an item from anywhere on its lines, but a selection needs to hold some of the
/// item itself, so that one ending in the indentation of the next line doesn't format that line.
fn find_touched(
    text: &str,
    starts: &[usize],
    range: &Range<usize>,
) -> Option<RangeInclusive<usize>> {
    let mut touched = (0..starts.len()).filter(|index| {
        let item = item_range(text, starts, *index..=*index);
        if range.is_empty() {
            let lines = lines_range(text, item);
            lines.start <= range.start && range.start <= lines.end
        } else {
            range.start < item.end && range.end > item.start
        }
    });
    let first = touched.next()?;
    let last = touched.next_back().unwrap_or(first);
    Some(first..=last)
}

/// Whether the same items are within `items` once sorted, whatever order they end up in, which
/// means sorting them moves nothing outside of them.
///
/// `root` is the file once sorted and `starts` is where each item started before it was.
fn holds_same_items(root: &Root, starts: &[usize], items: &RangeInclusive<usize>) -> bool {
    items.clone().all(|index| {
        let original_index = starts.binary_search(&root.items[index].start_in_source());
        original_index.is_ok_and(|index| items.contains(&index))
    })
}

/// Widens the items to all of the tables they are in, from the header of the first to the last
/// entry of the last.
fn widen_to_tables(root: &Root, items: RangeInclusive<usize>) -> RangeInclusive<usize> {
    let (first, last) = items.into_inner();
    let first = root.items[..=first]
        .iter()
        .rposition(RootItem::is_table_header)
        .unwrap_or(0);
    first..=section_end(&root.items, last + 1) - 1
}

/// The text from the start of the first item to the end of the last one.
///
/// Only whitespace separates one item from the next since a comment is an item too.
fn item_range(text: &str, starts: &[usize], items: RangeInclusive<usize>) -> Range<usize> {
    let start = starts[*items.start()];
    let next_start = starts.get(items.end() + 1).copied().unwrap_or(text.len());
    start
        ..start
            + text[start..next_start]
                .trim_end_matches([' ', '\t', '\r', '\n'])
                .len()
}

/// Widens the range to the start of its first line, so that the indentation there is formatted,
/// and to the end of its last line without the line break.
fn lines_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = text[..range.start]
        .rfind('\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let rest = &text[range.end..];
    let line = rest.find('\n').map(|index| &rest[..index]).unwrap_or(rest);
    start..range.end + line.trim_end_matches('\r').len()
}

/// Keeps the line endings of the rest of the file since changing those is up to formatting the
/// whole file.
fn with_file_new_lines<'a>(formatted: &'a str, file_text: &str) -> Cow<'a, str> {
    if !file_text.contains('\n') {
        return Cow::Borrowed(formatted);
    }
    match resolve_new_line_kind(file_text, NewLineKind::Auto) {
        "\r\n" if formatted.contains('\n') && !formatted.contains("\r\n") => {
            Cow::Owned(formatted.replace('\n', "\r\n"))
        }
        "\n" if formatted.contains("\r\n") => Cow::Owned(formatted.replace("\r\n", "\n")),
        _ => Cow::Borrowed(formatted),
    }
}
