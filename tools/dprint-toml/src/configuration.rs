/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use dprint_core::configuration::{
    ConfigKeyMap, ConfigurationDiagnostic, GlobalConfiguration, NewLineKind,
    ParseConfigurationError, RECOMMENDED_GLOBAL_CONFIGURATION, get_unknown_property_diagnostics,
    get_value,
};
use dprint_core::generate_str_to_from;

/// Which quote a single-line string is written with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(
    target_arch = "wasm32",
    derive(serde::Serialize),
    serde(rename_all = "camelCase")
)]
pub(crate) enum QuoteStyle {
    /// Keeps the quote the string was written with.
    Maintain,
    /// Uses a double quote when doing so doesn't require adding an escape.
    PreferDouble,
    /// Uses a single quote when doing so doesn't require adding an escape.
    PreferSingle,
}

generate_str_to_from![
    QuoteStyle,
    [Maintain, "maintain"],
    [PreferDouble, "preferDouble"],
    [PreferSingle, "preferSingle"]
];

/// Whether something is indented.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(
    target_arch = "wasm32",
    derive(serde::Serialize),
    serde(rename_all = "camelCase")
)]
pub(crate) enum IndentKind {
    /// Indents where the file already did.
    Maintain,
    /// Always indents.
    Always,
    /// Never indents.
    Never,
}

generate_str_to_from![
    IndentKind,
    [Maintain, "maintain"],
    [Always, "always"],
    [Never, "never"]
];

/// When a trailing comma is written after the last value of an array or inline table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(
    target_arch = "wasm32",
    derive(serde::Serialize),
    serde(rename_all = "camelCase")
)]
pub(crate) enum TrailingCommaKind {
    /// Writes one only when the array or inline table is written over multiple lines.
    OnlyMultiLine,
    /// Never writes one.
    Never,
}

generate_str_to_from![
    TrailingCommaKind,
    [OnlyMultiLine, "onlyMultiLine"],
    [Never, "never"]
];

#[derive(Clone)]
#[cfg_attr(
    target_arch = "wasm32",
    derive(serde::Serialize),
    serde(rename_all = "camelCase")
)]
pub(crate) struct Configuration {
    pub(crate) line_width: u32,
    pub(crate) use_tabs: bool,
    pub(crate) indent_width: u8,
    pub(crate) new_line_kind: NewLineKind,
    pub(crate) quote_style: QuoteStyle,
    pub(crate) indent_tables: IndentKind,
    pub(crate) indent_entries: IndentKind,
    pub(crate) trailing_commas: TrailingCommaKind,
    pub(crate) space_surrounding_equals: bool,
    pub(crate) sort_keys: bool,
    pub(crate) sort_arrays: bool,
    pub(crate) sort_inline_tables: bool,
    pub(crate) array_prefer_single_line: bool,
    pub(crate) array_space_surrounding_brackets: bool,
    pub(crate) inline_table_prefer_single_line: bool,
    pub(crate) inline_table_space_surrounding_braces: bool,
    pub(crate) comment_force_leading_space: bool,
    pub(crate) cargo_apply_conventions: bool,
}

/// Resolves the configuration from the plugin's key value pairs and the global configuration.
pub(crate) fn resolve_config(
    mut config: ConfigKeyMap,
    global_config: &GlobalConfiguration,
) -> (Configuration, Vec<ConfigurationDiagnostic>) {
    let mut diagnostics = Vec::new();
    // General option that the more specific ones below fall back to
    let prefer_single_line = get_value(&mut config, "preferSingleLine", false, &mut diagnostics);
    let resolved = Configuration {
        line_width: get_value(
            &mut config,
            "lineWidth",
            global_config
                .line_width
                .unwrap_or(RECOMMENDED_GLOBAL_CONFIGURATION.line_width),
            &mut diagnostics,
        ),
        use_tabs: get_value(
            &mut config,
            "useTabs",
            global_config
                .use_tabs
                .unwrap_or(RECOMMENDED_GLOBAL_CONFIGURATION.use_tabs),
            &mut diagnostics,
        ),
        indent_width: get_value(
            &mut config,
            "indentWidth",
            global_config.indent_width.unwrap_or(2),
            &mut diagnostics,
        ),
        new_line_kind: get_value(
            &mut config,
            "newLineKind",
            global_config
                .new_line_kind
                .unwrap_or(RECOMMENDED_GLOBAL_CONFIGURATION.new_line_kind),
            &mut diagnostics,
        ),
        quote_style: get_value(
            &mut config,
            "quoteStyle",
            QuoteStyle::PreferDouble,
            &mut diagnostics,
        ),
        indent_tables: get_value(
            &mut config,
            "indentTables",
            IndentKind::Maintain,
            &mut diagnostics,
        ),
        indent_entries: get_value(
            &mut config,
            "indentEntries",
            IndentKind::Maintain,
            &mut diagnostics,
        ),
        trailing_commas: get_value(
            &mut config,
            "trailingCommas",
            TrailingCommaKind::OnlyMultiLine,
            &mut diagnostics,
        ),
        space_surrounding_equals: get_value(
            &mut config,
            "spaceSurroundingEquals",
            true,
            &mut diagnostics,
        ),
        sort_keys: get_value(&mut config, "sortKeys", false, &mut diagnostics),
        sort_arrays: get_value(&mut config, "sortArrays", false, &mut diagnostics),
        sort_inline_tables: get_value(&mut config, "sortInlineTables", false, &mut diagnostics),
        array_prefer_single_line: get_value(
            &mut config,
            "array.preferSingleLine",
            prefer_single_line,
            &mut diagnostics,
        ),
        array_space_surrounding_brackets: get_value(
            &mut config,
            "array.spaceSurroundingBrackets",
            false,
            &mut diagnostics,
        ),
        inline_table_prefer_single_line: get_value(
            &mut config,
            "inlineTable.preferSingleLine",
            prefer_single_line,
            &mut diagnostics,
        ),
        inline_table_space_surrounding_braces: get_value(
            &mut config,
            "inlineTable.spaceSurroundingBraces",
            true,
            &mut diagnostics,
        ),
        comment_force_leading_space: get_value(
            &mut config,
            "comment.forceLeadingSpace",
            true,
            &mut diagnostics,
        ),
        cargo_apply_conventions: get_value(
            &mut config,
            "cargo.applyConventions",
            true,
            &mut diagnostics,
        ),
    };
    diagnostics.extend(get_unknown_property_diagnostics(config));
    (resolved, diagnostics)
}
