/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, c_char};

/// Preprocess a user-supplied search string into a safe FTS5 query expression.
/// FTS5 keyword operators (AND, OR, NOT) are preserved so callers can use them
/// intentionally. Structural characters that cause FTS5 parse errors (unmatched
/// quotes and parentheses) are stripped from individual tokens.
pub fn preprocess_fts_query(q: &str) -> String {
    const FTS5_KEYWORDS: &[&str] = &["AND", "OR", "NOT"];

    let mut parts: Vec<String> = Vec::new();

    for token in q.split_whitespace() {
        if FTS5_KEYWORDS.contains(&token) {
            parts.push(token.to_string());
        } else {
            let clean: String = token
                .chars()
                .filter(|c| !matches!(c, '"' | '(' | ')'))
                .collect();
            if !clean.is_empty() {
                if clean.ends_with('*') {
                    parts.push(clean);
                } else {
                    parts.push(format!("{clean}*"));
                }
            }
        }
    }

    // Remove dangling/consecutive operators
    let mut cleaned: Vec<String> = Vec::with_capacity(parts.len());
    for part in parts {
        if FTS5_KEYWORDS.contains(&part.as_str()) {
            if matches!(cleaned.last(), Some(prev) if !FTS5_KEYWORDS.contains(&prev.as_str())) {
                cleaned.push(part);
            }
        } else {
            cleaned.push(part);
        }
    }
    if matches!(cleaned.last(), Some(p) if FTS5_KEYWORDS.contains(&p.as_str())) {
        cleaned.pop();
    }

    cleaned.join(" ")
}

pub(crate) fn optional_string(value: *const c_char) -> Option<String> {
    if value.is_null() {
        None
    } else {
        Some(
            // SAFETY: value is non-null and points to a SQLite-owned NUL-terminated string.
            unsafe { CStr::from_ptr(value) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}
