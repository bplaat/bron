/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use dprint_core::formatting::Arena;

use crate::configuration::Configuration;

/// A vector allocated in the arena, which is freed along with the print items.
pub(crate) type BumpVec<'a, T> = dprint_core::formatting::bumpalo::collections::Vec<'a, T>;

pub(crate) struct Context<'a> {
    pub(crate) arena: &'a Arena,
    pub(crate) config: &'a Configuration,
    /// How many single-line inline tables enclose whatever is being generated. A table within one is
    /// kept on a single line too, since a newline between its braces would not be within a value.
    single_line_table_depth: usize,
    /// How many of those tables hold a multi-line string, whose raw text leaves the printer's idea
    /// of where the line begins out of step with what was written. A width based decision taken
    /// after one on the same line cannot be trusted, so the arrays there are collapsed onto the
    /// table's line instead of being laid out against the line width.
    collapsed_arrays_depth: usize,
}

impl<'a> Context<'a> {
    pub(crate) const fn new(arena: &'a Arena, config: &'a Configuration) -> Self {
        Self {
            arena,
            config,
            single_line_table_depth: 0,
            collapsed_arrays_depth: 0,
        }
    }

    pub(crate) const fn is_in_single_line_table(&self) -> bool {
        self.single_line_table_depth > 0
    }

    pub(crate) const fn are_arrays_collapsed(&self) -> bool {
        self.collapsed_arrays_depth > 0
    }

    pub(crate) const fn line_context(&self) -> crate::ast::LineContext {
        crate::ast::LineContext {
            within_single_line_table: self.is_in_single_line_table(),
            arrays_collapsed: self.are_arrays_collapsed(),
        }
    }

    pub(crate) fn with_single_line_table<T>(
        &mut self,
        collapse_arrays: bool,
        action: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.single_line_table_depth += 1;
        if collapse_arrays {
            self.collapsed_arrays_depth += 1;
        }
        let result = action(self);
        self.single_line_table_depth -= 1;
        if collapse_arrays {
            self.collapsed_arrays_depth -= 1;
        }
        result
    }
}
