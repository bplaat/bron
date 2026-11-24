/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;

use anyhow::{Context, Result};
use regex::{Captures, regex};
use serde::Serialize;
use serde_json::ser::PrettyFormatter;
use serde_json::{Serializer, Value};

use crate::Xtask;
use crate::metadata::platform_exclude_paths;

impl Xtask {
    pub(crate) fn configure_vscode(&self) -> Result<()> {
        let metadata = self.cargo_metadata()?;
        let excludes = platform_exclude_paths(&metadata, self.os, &self.root)?;
        let settings_path = self.root.join(".vscode/settings.json");
        let contents = fs::read_to_string(&settings_path)
            .with_context(|| format!("failed to read {}", settings_path.display()))?;
        let mut settings = parse_json_with_comments(&contents)
            .with_context(|| format!("failed to parse {}", settings_path.display()))?;
        set_dprint_format_on_save(&mut settings)?;
        set_rust_analyzer_excludes(&mut settings, excludes)?;

        let mut output = Vec::new();
        let formatter = PrettyFormatter::with_indent(b"    ");
        let mut serializer = Serializer::with_formatter(&mut output, formatter);
        settings.serialize(&mut serializer)?;
        output.push(b'\n');

        if output != contents.as_bytes() {
            fs::write(&settings_path, output)
                .with_context(|| format!("failed to write {}", settings_path.display()))?;
            println!("Configured VS Code for {}", self.os.name());
        }
        self.build_dprint_plugins()
    }
}

fn parse_json_with_comments(contents: &str) -> Result<Value, serde_json::Error> {
    let comments = regex!(r#"(?s)("(?:\\.|[^"\\])*")|//[^\r\n]*|/\*.*?\*/"#);
    let json = comments.replace_all(contents, |captures: &Captures<'_>| {
        captures
            .get(1)
            .map_or_else(String::new, |string| string.as_str().to_owned())
    });
    serde_json::from_str(&json)
}

const DPRINT_LANGUAGES: [&str; 12] = [
    "css",
    "dockerfile",
    "html",
    "javascript",
    "javascriptreact",
    "json",
    "jsonc",
    "markdown",
    "typescript",
    "typescriptreact",
    "xml",
    "yaml",
];

const XML_FILE_PATTERNS: [&str; 5] = ["*.entitlements", "*.manifest", "*.plist", "*.svg", "*.xaml"];

fn set_dprint_format_on_save(settings: &mut Value) -> Result<()> {
    let settings = settings
        .as_object_mut()
        .context("VS Code settings must be a JSON object")?;
    let associations = settings
        .entry("files.associations")
        .or_insert_with(|| Value::Object(Default::default()))
        .as_object_mut()
        .context("VS Code files.associations must be a JSON object")?;
    for pattern in XML_FILE_PATTERNS {
        associations.insert(pattern.to_owned(), Value::String("xml".to_owned()));
    }

    for language in DPRINT_LANGUAGES {
        let key = format!("[{language}]");
        let language_settings = settings
            .entry(&key)
            .or_insert_with(|| Value::Object(Default::default()))
            .as_object_mut()
            .with_context(|| format!("VS Code {key} settings must be a JSON object"))?;
        language_settings.insert("editor.defaultFormatter".to_owned(), "dprint.dprint".into());
        language_settings.insert("editor.formatOnSave".to_owned(), Value::Bool(true));
    }
    Ok(())
}

fn set_rust_analyzer_excludes(
    settings: &mut Value,
    excludes: impl IntoIterator<Item = String>,
) -> Result<()> {
    let settings = settings
        .as_object_mut()
        .context("VS Code settings must be a JSON object")?;
    settings.insert(
        "rust-analyzer.files.exclude".to_owned(),
        Value::Array(excludes.into_iter().map(Value::String).collect()),
    );
    Ok(())
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn json_with_comments_is_parsed_without_changing_strings() -> Result<()> {
        let settings = parse_json_with_comments(
            r#"{
                // Line comment
                "url": "https://example.com/path/*-literal",
                /* Block comment */
                "enabled": true
            }"#,
        )?;
        assert_eq!(
            settings,
            json!({
                "url": "https://example.com/path/*-literal",
                "enabled": true
            })
        );
        Ok(())
    }

    #[test]
    fn rust_analyzer_excludes_are_added_without_changing_other_settings() -> Result<()> {
        let mut settings = json!({ "editor.formatOnSave": true });
        set_rust_analyzer_excludes(
            &mut settings,
            ["bin/macos-app".to_owned(), "bin/windows-app".to_owned()],
        )?;
        assert_eq!(
            settings,
            json!({
                "editor.formatOnSave": true,
                "rust-analyzer.files.exclude": ["bin/macos-app", "bin/windows-app"]
            })
        );
        Ok(())
    }

    #[test]
    fn dprint_formats_supported_languages_on_save() -> Result<()> {
        let mut settings = json!({
            "[typescript]": { "editor.defaultFormatter": "esbenp.prettier-vscode", "editor.tabSize": 4 },
            "[rust]": { "editor.defaultFormatter": "rust-lang.rust-analyzer" }
        });
        set_dprint_format_on_save(&mut settings)?;
        for language in DPRINT_LANGUAGES {
            let language_settings = &settings[format!("[{language}]")];
            assert_eq!(
                language_settings["editor.defaultFormatter"],
                "dprint.dprint"
            );
            assert_eq!(language_settings["editor.formatOnSave"], true);
        }
        assert_eq!(settings["[typescript]"]["editor.tabSize"], 4);
        assert_eq!(
            settings["[rust]"]["editor.defaultFormatter"],
            "rust-lang.rust-analyzer"
        );
        Ok(())
    }

    #[test]
    fn xml_file_associations_keep_other_associations() -> Result<()> {
        let mut settings = json!({ "files.associations": { "*.inc": "nasm" } });
        set_dprint_format_on_save(&mut settings)?;
        assert_eq!(
            settings["files.associations"],
            json!({
                "*.entitlements": "xml",
                "*.inc": "nasm",
                "*.manifest": "xml",
                "*.plist": "xml",
                "*.svg": "xml",
                "*.xaml": "xml"
            })
        );
        Ok(())
    }

    #[test]
    fn rust_analyzer_excludes_replace_stale_platform_paths() -> Result<()> {
        let mut settings = json!({ "rust-analyzer.files.exclude": ["bin/old"] });
        set_rust_analyzer_excludes(&mut settings, ["bin/current".to_owned()])?;
        assert_eq!(
            settings["rust-analyzer.files.exclude"],
            json!(["bin/current"])
        );
        Ok(())
    }
}
