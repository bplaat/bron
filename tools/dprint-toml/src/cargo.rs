/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;

use crate::ast::*;
use crate::sorting::{section_end, sort_root_entries, sort_with_comments, value_sort_key};

pub(crate) fn is_cargo_toml_file(file_path: &Path) -> bool {
    // Cargo.toml always has this casing, see https://github.com/rust-lang/cargo/issues/45
    file_path
        .file_name()
        .is_some_and(|name| name == "Cargo.toml")
}

/// A table whose contents the conventions rearrange.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Dependencies,
    Workspace,
    Other,
}

const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "build-dependencies", "dev-dependencies"];
const TARGET_TABLES: [&str; 4] = ["bin", "example", "test", "bench"];

fn key_parts<'a>(header: &'a TableHeader<'_>) -> Vec<&'a str> {
    header.key.parts().map(KeyPart::unquoted_text).collect()
}

fn section_of(header: &TableHeader<'_>) -> Section {
    // An array of tables holds repeated elements rather than one table's keys
    if header.is_array_of_tables {
        return Section::Other;
    }
    match key_parts(header).as_slice() {
        [name] | ["target", _, name] if DEPENDENCY_TABLES.contains(name) => Section::Dependencies,
        ["workspace", "dependencies"] => Section::Dependencies,
        ["workspace"] => Section::Workspace,
        _ => Section::Other,
    }
}

/// Where a table goes in the conventional table order, tables of the same rank keep their order.
fn table_rank(header: &TableHeader<'_>) -> usize {
    let dependency = |name: &str| DEPENDENCY_TABLES.iter().position(|table| *table == name);
    match key_parts(header).as_slice() {
        ["package", ..] => 0,
        ["workspace", ..] => 1,
        ["lib", ..] => 2,
        ["lints", ..] => 3,
        ["features", ..] => 4,
        [name, ..] if let Some(index) = dependency(name) => 5 + 2 * index,
        ["target", _, name, ..] if let Some(index) = dependency(name) => 6 + 2 * index,
        [name, ..] if let Some(index) = TARGET_TABLES.iter().position(|table| table == name) => {
            11 + index
        }
        ["patch", ..] => 15,
        ["profile", ..] => 16,
        _ => 17,
    }
}

/// Puts the tables in the conventional order: package, workspace, lib, lints, features, the
/// dependencies each followed by their target specific ones, the build targets, patch and profile.
/// Comments directly above a table header move with it, and a moved table gets a blank line above it.
fn sort_tables(root: &mut Root<'_>) {
    let mut starts = Vec::new();
    let mut previous_header = 0;
    for (index, item) in root.items.iter().enumerate() {
        if item.is_table_header() {
            let mut start = index;
            while start > previous_header
                && matches!(root.items[start - 1], RootItem::Comment(_))
                && !root.items[start].blank_line_before()
            {
                start -= 1;
            }
            starts.push(start);
            previous_header = index + 1;
        }
    }
    let Some(&first) = starts.first() else {
        return;
    };

    let mut tables = Vec::with_capacity(starts.len());
    let mut items = root.items.drain(first..).collect::<Vec<_>>();
    for start in starts.iter().skip(1).rev() {
        tables.push(items.split_off(start - first));
    }
    tables.push(items);
    tables.reverse();

    let mut tables = tables.into_iter().enumerate().collect::<Vec<_>>();
    tables.sort_by_key(|(_, table)| {
        table
            .iter()
            .find_map(|item| match item {
                RootItem::TableHeader(header) => Some(table_rank(header)),
                _ => None,
            })
            .unwrap_or_default()
    });
    for (position, (original_position, table)) in tables.iter_mut().enumerate() {
        // A moved table is separated from whatever now comes before it
        if position != *original_position {
            table[0].set_blank_line_before(true);
        }
    }
    root.items
        .extend(tables.into_iter().flat_map(|(_, table)| table));
}

/// Sorts the tables, the entries of every dependencies table, including the target specific ones,
/// and the `[workspace]` `members` array. The entries of `[package]` keep the order they were written in.
pub(crate) fn apply_cargo_toml_conventions(root: &mut Root<'_>) {
    sort_tables(root);
    let mut last_header = Section::Other;
    for index in 0..root.items.len() {
        match &mut root.items[index] {
            RootItem::TableHeader(header) => {
                last_header = section_of(header);
                if last_header == Section::Dependencies {
                    let end = section_end(&root.items, index + 1);
                    sort_root_entries(&mut root.items, index + 1, end, &|left, right| {
                        entry_sort_key(left).cmp(entry_sort_key(right))
                    });
                }
            }
            RootItem::Entry(entry)
                if last_header == Section::Workspace && entry_sort_key(entry) == "members" =>
            {
                sort_workspace_members(entry);
            }
            _ => {}
        }
    }
}

/// The name an entry sorts under. Only the first segment of a dotted key is used, so that
/// `serde.workspace` sorts beside `serde`, and the quotes around a quoted segment are ignored.
fn entry_sort_key<'a>(entry: &'a Entry<'_>) -> &'a str {
    entry.key.first.unquoted_text()
}

/// Sorts the string members of a `[workspace]` `members` array by their contents.
fn sort_workspace_members(entry: &mut Entry<'_>) {
    let ValueKind::Array(array) = &mut entry.value.kind else {
        return;
    };
    let all_strings = array.values.iter().all(|value| match &value.value.kind {
        ValueKind::Scalar(text) => text.starts_with('"') || text.starts_with('\''),
        _ => false,
    });
    if all_strings {
        sort_with_comments(&mut array.values, |left, right| {
            value_sort_key(&left.value).cmp(&value_sort_key(&right.value))
        });
    }
}
