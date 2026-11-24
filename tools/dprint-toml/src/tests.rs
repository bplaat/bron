/*
 * Copyright (c) 2021-2023 David Sherret
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use dprint_core::configuration::{ConfigKeyMap, ConfigKeyValue, GlobalConfiguration};
use dprint_core::formatting::ArenaPool;

use crate::configuration::resolve_config;
use crate::format_range::format_text_range;
use crate::format_text::{ParseError, format_text};

fn format(
    file_name: &str,
    text: &str,
    range: Option<Range<usize>>,
    config: ConfigKeyMap,
) -> Result<Option<String>, ParseError> {
    let (config, diagnostics) = resolve_config(config, &GlobalConfiguration::default());
    assert!(
        diagnostics.is_empty(),
        "config diagnostics: {diagnostics:?}"
    );
    let path = Path::new(file_name);
    let arena_pool = ArenaPool::new();
    match range {
        Some(range) => format_text_range(&arena_pool, path, text, range, &config),
        None => format_text(&arena_pool.create_arena(), path, text, &config),
    }
}

fn format_default(text: &str) -> Result<Option<String>, ParseError> {
    format("file.toml", text, None, ConfigKeyMap::new())
}

fn parse_spec_range(text: &str) -> (String, Option<Range<usize>>) {
    let Some((prefix, rest)) = text.split_once("[|") else {
        return (text.to_owned(), None);
    };
    let (selected, suffix) = rest.split_once("|]").expect("spec range without |]");
    let start = prefix.len();
    (
        format!("{prefix}{selected}{suffix}"),
        Some(start..start + selected.len()),
    )
}

/// Parses the `~~ key: value, ... ~~` config line of a spec file.
fn parse_spec_config(text: &str) -> ConfigKeyMap {
    text.split(',')
        .map(|item| {
            let (key, value) = item.split_once(':').expect("config option without colon");
            let value = value.trim();
            let value = if let Ok(value) = value.parse::<bool>() {
                ConfigKeyValue::from(value)
            } else if let Ok(value) = value.parse::<i32>() {
                ConfigKeyValue::from(value)
            } else {
                ConfigKeyValue::from(value)
            };
            (key.trim().to_owned(), value)
        })
        .collect()
}

fn spec_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("can't read specs directory") {
        let path = entry.expect("can't read spec entry").path();
        if path.is_dir() {
            spec_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

/// Runs the specs in the `specs` directory, which use the dprint-development spec format: an
/// optional `-- file name --` line, an optional `~~ config ~~` line, then `== message ==` sections
/// holding the input and the expected output separated by an `[expect]` line.
#[test]
fn specs() {
    let mut files = Vec::new();
    spec_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("specs"),
        &mut files,
    );
    files.sort();
    let mut failures = Vec::new();
    for file in files {
        let contents = fs::read_to_string(&file)
            .expect("can't read spec")
            .replace("\r\n", "\n");
        let mut rest = contents.as_str();
        let mut file_name = "file.toml";
        if let Some(header) = rest.strip_prefix("--") {
            let (name, after) = header.split_once("--\n").expect("unterminated file name");
            file_name = name.trim();
            rest = after;
        }
        let mut config_text = "";
        if let Some(header) = rest.strip_prefix("~~") {
            let (config, after) = header.split_once("~~\n").expect("unterminated config");
            config_text = config.trim();
            rest = after;
        }

        for section in rest
            .split("\n== ")
            .map(|section| section.trim_start_matches("== "))
        {
            let Some((message, body)) = section.split_once('\n') else {
                continue;
            };
            let (input, expected) = body
                .split_once("[expect]\n")
                .expect("spec without [expect]");
            let (input, input_range) = parse_spec_range(input.strip_suffix('\n').unwrap_or(input));
            let (expected, expected_range) = parse_spec_range(expected);
            let config = if config_text.is_empty() {
                ConfigKeyMap::new()
            } else {
                parse_spec_config(config_text)
            };
            let format_once =
                |text: &str, range| match format(file_name, text, range, config.clone()) {
                    Ok(formatted) => formatted.unwrap_or_else(|| text.to_owned()),
                    Err(err) => format!("error: {err}"),
                };
            let formatted = format_once(&input, input_range);
            let formatted_twice = format_once(&formatted, expected_range);
            if formatted != expected || formatted_twice != expected {
                failures.push(format!(
                    "{}: {}\n--- expected ---\n{expected}--- got ---\n{formatted}--- got twice ---\n{formatted_twice}",
                    file.display(),
                    message.trim_end_matches(" =="),
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn range_keeps_bom_crlf_and_untouched_lines() -> Result<(), ParseError> {
    let text = "\u{FEFF}a   =   1\r\nb   =   [\r\n1]  \r\nc   =   3\r\n";
    let start = text.find('b').unwrap();
    assert_eq!(
        format(
            "file.toml",
            text,
            Some(start..start + 1),
            ConfigKeyMap::new()
        )?
        .as_deref(),
        Some("\u{FEFF}a   =   1\r\nb = [\r\n  1,\r\n]\r\nc   =   3\r\n")
    );
    Ok(())
}

#[test]
fn strips_bom() -> Result<(), ParseError> {
    assert_eq!(
        format_default("\u{FEFF}# 1\n# 2\n")?.as_deref(),
        Some("# 1\n# 2\n")
    );
    Ok(())
}

#[test]
fn handles_windows_newlines() -> Result<(), ParseError> {
    assert_eq!(
        format_default("# 1\r\n# 2\r\n")?.as_deref(),
        Some("# 1\n# 2\n")
    );
    Ok(())
}

/// Nesting deeply enough to run out of stack aborts the process rather than raising an error, so
/// the parser refuses well before that.
#[test]
fn rejects_deeply_nested_collections() {
    for text in [
        format!("a = {}", "[".repeat(200)),
        format!("a = {}", "{ b = ".repeat(200)),
    ] {
        let error = format_default(&text).expect_err("should fail").to_string();
        assert!(error.contains("nested too deeply"), "got {error}");
    }
    // A depth a real document might plausibly reach is still accepted
    let nested = format!("a = {}{}", "[".repeat(16), "]".repeat(16));
    assert!(format_default(&nested).is_ok());
}

/// A comment may legitimately end in whitespace that isn't a space or a tab.
#[test]
fn keeps_unicode_whitespace_in_a_comment() -> Result<(), ParseError> {
    assert_eq!(format_default("a = 1 # hi\u{a0}\n")?, None);
    Ok(())
}

#[test]
fn reports_parse_errors() {
    let cases = [
        (
            "a = 1\rb = 2\n",
            "expected a line feed after the carriage return",
        ),
        (
            "a = [1,\r2]\n",
            "expected a line feed after the carriage return",
        ),
        (
            "a = { b = 1,\rc = 2 }\n",
            "expected a line feed after the carriage return",
        ),
        ("b = BAR\n", "expected a value"),
        ("a = \"unterminated\n", "unterminated string"),
        // A line-ending backslash belongs to multi-line strings only
        ("a = \"x\\\ny\"\n", "unterminated string"),
        ("a = [1, 2\n", "unterminated array"),
        ("a = { b = 1\n", "unterminated inline table"),
        ("a = [1 2]\n", "expected ',' between array values"),
        (
            "a = { b = 1 c = 2 }\n",
            "expected ',' between inline table entries",
        ),
        ("a\n", "expected '=' after the key"),
        ("[table\n", "expected ']' to close the table header"),
        (
            "\"\"\"key\"\"\" = 1\n",
            "a key cannot be a multi-line string",
        ),
        (
            "a = \"\"\"\"\"\"\"\"\"\"\n",
            "too many '\"' in a row to end a multi-line string",
        ),
        ("a = 1 b = 2\n", "expected a newline"),
        ("[a] x\n", "expected a newline"),
    ];
    for (input, expected) in cases {
        let error = format_default(input).expect_err("should fail").to_string();
        assert!(
            error.contains(expected),
            "for {input:?}\n  expected message containing {expected:?}\n  got {error}"
        );
    }
}
