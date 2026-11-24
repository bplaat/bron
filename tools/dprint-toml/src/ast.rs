/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// AST types for the TOML parser.
//
// The tree is lossless for everything the formatter cares about: values keep their source text
// verbatim, and comments and blank lines are attached to the construct they belong to rather than
// being recovered by walking siblings. Anything not represented here is insignificant whitespace.
//
// Text is borrowed from the source. Every piece of text the tree holds is a slice of the input,
// which outlives the tree, so none of it is copied. Nothing rewrites a node's text -- the Cargo
// conventions reorder nodes and adjust their blank-line flags, but the text itself is only ever
// read -- so a plain `&str` is enough and a `Cow` would only make each node larger.
//
// The tree itself is allocated in the same arena as the print items, so building it costs no heap
// allocations. The arena never runs destructors, so nothing in the tree may own heap memory: the
// lists the sorting rearranges are vectors in the arena and the rest are slices of it.

use crate::configuration::Configuration;
use crate::generation::BumpVec;

// the arena never drops these, so they mustn't own anything that would need to be freed
const _: () = assert!(!std::mem::needs_drop::<Comment<'static>>());
const _: () = assert!(!std::mem::needs_drop::<Key<'static>>());
const _: () = assert!(!std::mem::needs_drop::<TableHeader<'static>>());

/// A comment, from the `#` up to (but not including) the end of the line.
#[derive(Debug, Clone)]
pub(crate) struct Comment<'a> {
    /// The comment's source text, including the leading `#`.
    pub(crate) text: &'a str,
    /// Whether a blank line separates this comment from whatever precedes it.
    pub(crate) blank_line_before: bool,
    /// How many whitespace characters preceded the comment on its line, which is what the
    /// `maintain` indent settings go by. Zero for a comment that isn't on a line of its own at the
    /// top level, which is never indented independently.
    pub(crate) indent_in_source: usize,
}

/// A parsed TOML document.
#[derive(Debug, Clone)]
pub(crate) struct Root<'a> {
    pub(crate) items: BumpVec<'a, RootItem<'a>>,
}

/// A top level item. Comments on their own line are items in their own right rather than trivia
/// attached to the following item, which is what lets blank lines around them be preserved.
#[derive(Debug, Clone)]
pub(crate) enum RootItem<'a> {
    Comment(Comment<'a>),
    Entry(Entry<'a>),
    TableHeader(TableHeader<'a>),
}

impl RootItem<'_> {
    pub(crate) const fn blank_line_before(&self) -> bool {
        match self {
            RootItem::Comment(c) => c.blank_line_before,
            RootItem::Entry(e) => e.blank_line_before,
            RootItem::TableHeader(h) => h.blank_line_before,
        }
    }

    pub(crate) const fn set_blank_line_before(&mut self, blank_line_before: bool) {
        match self {
            RootItem::Comment(c) => c.blank_line_before = blank_line_before,
            RootItem::Entry(e) => e.blank_line_before = blank_line_before,
            RootItem::TableHeader(h) => h.blank_line_before = blank_line_before,
        }
    }

    pub(crate) const fn is_table_header(&self) -> bool {
        matches!(self, RootItem::TableHeader(_))
    }
}

/// A table header, either `[key]` or `[[key]]`.
#[derive(Debug, Clone)]
pub(crate) struct TableHeader<'a> {
    pub(crate) key: Key<'a>,
    /// Whether this is an array of tables header (`[[key]]`).
    pub(crate) is_array_of_tables: bool,
    pub(crate) blank_line_before: bool,
    pub(crate) trailing_comment: Option<Comment<'a>>,
    /// How many whitespace characters preceded the header on its line, which is what the
    /// `maintain` indent settings go by.
    pub(crate) indent_in_source: usize,
}

/// A key/value pair.
#[derive(Debug, Clone)]
pub(crate) struct Entry<'a> {
    pub(crate) key: Key<'a>,
    pub(crate) value: Value<'a>,
    pub(crate) blank_line_before: bool,
    pub(crate) trailing_comment: Option<Comment<'a>>,
    /// Comments on the lines immediately above this entry. Only populated for entries inside an
    /// inline table; at the top level such comments are separate [`RootItem::Comment`]s.
    pub(crate) leading_comments: BumpVec<'a, Comment<'a>>,
    /// How many whitespace characters preceded the entry on its line, which is what the `maintain`
    /// indent settings go by. Always zero for an entry inside an inline table, which is never
    /// indented on its own.
    pub(crate) indent_in_source: usize,
}

/// A key, which may be dotted (`a.b.c`).
///
/// The first segment is held inline because a dotted key is the exception: keeping the rest in a
/// slice means the overwhelmingly common single segment key allocates nothing at all.
#[derive(Debug, Clone)]
pub(crate) struct Key<'a> {
    pub(crate) first: KeyPart<'a>,
    pub(crate) rest: &'a [KeyPart<'a>],
}

impl<'a> Key<'a> {
    /// The key's dot separated segments, of which there is always at least one.
    pub(crate) fn parts(&self) -> impl Iterator<Item = &KeyPart<'a>> {
        std::iter::once(&self.first).chain(self.rest.iter())
    }
}

impl Key<'_> {
    /// Whether this key names a strict ancestor of `other`, as `[a]` does of `[a.b]`. Quoting is
    /// ignored, since `[a."b"]` names the same table as `[a.b]`.
    pub(crate) fn is_strict_prefix_of(&self, other: &Key<'_>) -> bool {
        let mut other_parts = other.parts();
        for part in self.parts() {
            match other_parts.next() {
                Some(other_part) if other_part.unquoted_text() == part.unquoted_text() => {}
                _ => return false,
            }
        }
        other_parts.next().is_some()
    }
}

/// One dot separated segment of a key. Bare, quoted and literal keys are all kept verbatim, since
/// the formatter reproduces the segment as written.
#[derive(Debug, Clone)]
pub(crate) struct KeyPart<'a> {
    pub(crate) text: &'a str,
}

impl KeyPart<'_> {
    /// The segment with its surrounding quotes removed, naming the key rather than spelling it. Any
    /// escape within a basic string is left as written, which is enough to compare against a plain
    /// name like `package`.
    pub(crate) fn unquoted_text(&self) -> &str {
        strip_quotes(self.text, &["\"", "'"])
    }
}

/// The text with the first of `quotes` that surrounds it removed, or the text itself when none does.
pub(crate) fn strip_quotes<'a>(text: &'a str, quotes: &[&str]) -> &'a str {
    quotes
        .iter()
        .find_map(|quote| text.strip_prefix(quote)?.strip_suffix(quote))
        .unwrap_or(text)
}

/// Where on its line a value is being written, which decides what may break around it.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LineContext {
    /// An enclosing inline table is written on a single line, so a table within it is written on
    /// that line too -- a newline between *its* braces would not be within a value.
    pub(crate) within_single_line_table: bool,
    /// An enclosing inline table holds a multi-line string, so the arrays within it are written on
    /// its line rather than broken up. A raw string leaves the printer's idea of where the line
    /// begins out of step with the text, so a width based decision taken after one on the same line
    /// cannot be trusted; the arrays there are laid out without one.
    pub(crate) arrays_collapsed: bool,
}

/// A value.
#[derive(Debug, Clone)]
pub(crate) struct Value<'a> {
    pub(crate) kind: ValueKind<'a>,
}

impl Value<'_> {
    /// Whether this value is written over more than one line however it is formatted.
    ///
    /// Telling a group this in advance saves it printing its values on the assumption that they fit
    /// on one line only to find out otherwise, which for values nested inside one another it would
    /// otherwise repeat at every level. It has to be certain, so an inline table the author wrote on
    /// one line counts for nothing within it except a string, whose own newlines are kept wherever it
    /// appears.
    pub(crate) fn is_known_multi_line(&self, config: &Configuration, line: LineContext) -> bool {
        match &self.kind {
            // a triple quoted string is only written over several lines if its contents are
            ValueKind::MultiLineString(text) => text.contains('\n'),
            ValueKind::Scalar(_) => false,
            // an array may be broken up wherever it sits, since its newlines are within a value --
            // unless it is one of the arrays collapsed onto its table's line, where only a string that
            // spans lines is left to break it
            ValueKind::Array(array) => {
                (!line.arrays_collapsed && array.force_use_new_lines(config))
                    || array
                        .values
                        .iter()
                        .any(|value| value.value.is_known_multi_line(config, line))
            }
            ValueKind::InlineTable(table) => {
                let broken_up = !line.within_single_line_table && table.force_use_new_lines(config);
                let inner = LineContext {
                    within_single_line_table: !broken_up,
                    arrays_collapsed: line.arrays_collapsed
                        || (!broken_up && table.contains_multi_line_string()),
                };
                broken_up
                    || table
                        .entries
                        .iter()
                        .any(|entry| entry.value.is_known_multi_line(config, inner))
            }
        }
    }

    /// Whether a multi-line string appears anywhere within this value.
    fn contains_multi_line_string(&self) -> bool {
        match &self.kind {
            ValueKind::MultiLineString(_) => true,
            ValueKind::Scalar(_) => false,
            ValueKind::Array(array) => array
                .values
                .iter()
                .any(|value| value.value.contains_multi_line_string()),
            ValueKind::InlineTable(table) => table.contains_multi_line_string(),
        }
    }

    /// Whether an inline table anywhere within this value holds a comment of its own.
    ///
    /// Such a comment is only written when its table is written over several lines, so a value
    /// containing one can never be collapsed onto a single line without losing it. An array's own
    /// comments don't count: an array keeps them however the table around it is written, since the
    /// newlines they bring sit within a value, which TOML 1.0 already allows between braces.
    fn contains_inline_table_comment(&self) -> bool {
        match &self.kind {
            ValueKind::Scalar(_) | ValueKind::MultiLineString(_) => false,
            ValueKind::Array(array) => array
                .values
                .iter()
                .any(|value| value.value.contains_inline_table_comment()),
            ValueKind::InlineTable(table) => table.contains_own_comment(),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ValueKind<'a> {
    /// A single-line value kept verbatim: a string, number, boolean or date-time.
    Scalar(&'a str),
    /// A multi-line basic or literal string, kept verbatim including its newlines.
    MultiLineString(&'a str),
    Array(Array<'a>),
    InlineTable(InlineTable<'a>),
}

/// An array (`[1, 2, 3]`).
#[derive(Debug, Clone)]
pub(crate) struct Array<'a> {
    pub(crate) values: BumpVec<'a, ArrayValue<'a>>,
    /// A comment on the same line as the opening bracket (`[ # here`).
    pub(crate) comment_after_open: Option<Comment<'a>>,
    /// Comments on their own lines between the last value and the closing bracket.
    pub(crate) comments_before_close: &'a [Comment<'a>],
    /// Whether the opening bracket is followed by a newline or a comment, meaning the author wrote
    /// the array over multiple lines.
    pub(crate) multi_line_in_source: bool,
}

impl Array<'_> {
    /// Whether the array should be printed over multiple lines. An array the author broke up is kept
    /// broken up, but one that holds nothing at all collapses.
    pub(crate) fn force_use_new_lines(&self, config: &Configuration) -> bool {
        // A comment written on its own line before the closing bracket is only given a line of its own
        // when the array is broken up. Left on one line it is printed against the last value, turning
        // it into that value's trailing comment and formatting differently the second time around.
        if !self.comments_before_close.is_empty() {
            return true;
        }
        if config.array_prefer_single_line {
            // The author's layout no longer decides this, so only a comment does. Any comment runs to the
            // end of its line, so an array holding one can't be written on a single line.
            return self.has_own_comment();
        }
        self.multi_line_in_source && !(self.values.is_empty() && self.comment_after_open.is_none())
    }

    /// Whether a comment sits directly within this array's brackets rather than inside one of its
    /// values.
    fn has_own_comment(&self) -> bool {
        self.comment_after_open.is_some()
            || !self.comments_before_close.is_empty()
            || self
                .values
                .iter()
                .any(|value| value.trailing_comment.is_some() || !value.leading_comments.is_empty())
    }
}

/// A value within an array, along with the comments attached to it.
#[derive(Debug, Clone)]
pub(crate) struct ArrayValue<'a> {
    pub(crate) value: Value<'a>,
    /// Comments on the lines immediately above the value.
    pub(crate) leading_comments: BumpVec<'a, Comment<'a>>,
    /// A comment on the same line as the value, before or after its comma.
    pub(crate) trailing_comment: Option<Comment<'a>>,
    pub(crate) blank_line_before: bool,
}

/// An item of an array or inline table, carrying the comments written around it.
pub(crate) trait ListItem<'a> {
    fn value(&self) -> &Value<'a>;
    fn leading_comments(&self) -> &BumpVec<'a, Comment<'a>>;
    fn leading_comments_mut(&mut self) -> &mut BumpVec<'a, Comment<'a>>;
    fn trailing_comment(&self) -> Option<&Comment<'a>>;
    fn trailing_comment_mut(&mut self) -> &mut Option<Comment<'a>>;
    fn blank_line_before(&self) -> bool;
    fn set_blank_line_before(&mut self, value: bool);
}

macro_rules! impl_list_item {
    ($($item:ident),*) => {$(
        impl<'a> ListItem<'a> for $item<'a> {
            fn value(&self) -> &Value<'a> {
                &self.value
            }
            fn leading_comments(&self) -> &BumpVec<'a, Comment<'a>> {
                &self.leading_comments
            }
            fn leading_comments_mut(&mut self) -> &mut BumpVec<'a, Comment<'a>> {
                &mut self.leading_comments
            }
            fn trailing_comment(&self) -> Option<&Comment<'a>> {
                self.trailing_comment.as_ref()
            }
            fn trailing_comment_mut(&mut self) -> &mut Option<Comment<'a>> {
                &mut self.trailing_comment
            }
            fn blank_line_before(&self) -> bool {
                self.blank_line_before
            }
            fn set_blank_line_before(&mut self, value: bool) {
                self.blank_line_before = value;
            }
        }
    )*};
}

impl_list_item!(ArrayValue, Entry);

/// An inline table (`{ a = 1, b = 2 }`).
#[derive(Debug, Clone)]
pub(crate) struct InlineTable<'a> {
    pub(crate) entries: BumpVec<'a, Entry<'a>>,
    /// A comment on the same line as the opening brace.
    pub(crate) comment_after_open: Option<Comment<'a>>,
    /// Comments on their own lines between the last entry and the closing brace.
    pub(crate) comments_before_close: &'a [Comment<'a>],
    /// Whether the author wrote the table over multiple lines, which TOML 1.1 permits.
    pub(crate) multi_line_in_source: bool,
}

impl InlineTable<'_> {
    /// Whether a comment sits directly within this table's braces rather than inside one of its
    /// values.
    ///
    /// Only such a comment forces the table itself onto several lines. A comment inside a value --
    /// within a nested array, say -- sits among that value's own lines, which TOML 1.0 already permits
    /// between the braces: "No newlines are allowed between the curly braces unless they are valid
    /// within a value."
    pub(crate) fn has_own_comment(&self) -> bool {
        self.comment_after_open.is_some()
            || !self.comments_before_close.is_empty()
            || self
                .entries
                .iter()
                .any(|entry| entry.trailing_comment.is_some() || !entry.leading_comments.is_empty())
    }

    /// Whether a multi-line string appears anywhere within this table.
    pub(crate) fn contains_multi_line_string(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.value.contains_multi_line_string())
    }

    /// Whether this table, or one nested within it, holds a comment of its own.
    ///
    /// A table nested inside one written on a single line is written on a single line too, and a
    /// single line has nowhere to put a comment, so the table around it has to be broken up as well.
    /// Doing so is always safe: a comment between braces runs to the end of its line, so a table
    /// holding one was already written over several lines and the document is already TOML 1.1.
    pub(crate) fn contains_own_comment(&self) -> bool {
        self.has_own_comment()
            || self
                .entries
                .iter()
                .any(|entry| entry.value.contains_inline_table_comment())
    }

    /// Whether the table should be printed over multiple lines, which only a TOML 1.1 parser
    /// accepts. A table the author wrote that way is kept that way unless it is asked to collapse,
    /// but one holding a comment anywhere within it has no choice.
    pub(crate) fn force_use_new_lines(&self, config: &Configuration) -> bool {
        self.contains_own_comment()
            || (!config.inline_table_prefer_single_line && self.multi_line_in_source)
    }
}
