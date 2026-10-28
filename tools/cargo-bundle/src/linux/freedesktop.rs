/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Generates the desktop entry, thumbnailer and AppStream metainfo of an application.

use std::fmt::Write as _;

use crate::manifest::Package;
use crate::support::xml_escape;

const DESKTOP_ENTRY: &str = "[Desktop Entry]";

fn list(values: &[String]) -> String {
    values.iter().map(|value| format!("{value};")).collect()
}

/// Generate the desktop entry, a custom entry overrides its keys and adds its other groups.
pub(crate) fn desktop_entry(
    package: &Package,
    executable: &str,
    has_icon: bool,
    custom: Option<&str>,
) -> String {
    let bundle = &package.metadata.bundle;
    let linux = &bundle.linux;
    let mime_types = linux.mime_types.as_deref().unwrap_or_default();
    let mut entries = vec![
        ("Type", "Application".to_owned()),
        ("Version", "1.5".to_owned()),
        ("Name", bundle.name.clone()),
    ];
    if let Some(description) = &package.description {
        entries.push(("Comment", description.clone()));
    }
    if mime_types.is_empty() {
        entries.push(("Exec", executable.to_owned()));
    } else {
        entries.push(("Exec", format!("{executable} %F")));
    }
    if has_icon {
        entries.push(("Icon", bundle.identifier.clone()));
    }
    entries.push(("Terminal", "false".to_owned()));
    if let Some(categories) = &linux.categories {
        entries.push(("Categories", list(categories)));
    }
    if !mime_types.is_empty() {
        entries.push(("MimeType", list(mime_types)));
    }

    let mut groups = String::new();
    let mut group = None;
    for line in custom.unwrap_or_default().lines().map(str::trim) {
        if line.starts_with('[') {
            group = Some(line);
            if line != DESKTOP_ENTRY {
                _ = write!(groups, "\n{line}\n");
            }
        } else if group == Some(DESKTOP_ENTRY) {
            if let Some((key, value)) = line.split_once('=') {
                let (key, value) = (key.trim(), value.trim().to_owned());
                match entries.iter_mut().find(|(name, _)| *name == key) {
                    Some(entry) => entry.1 = value,
                    None => entries.push((key, value)),
                }
            }
        } else if group.is_some() && !line.is_empty() {
            groups.push_str(line);
            groups.push('\n');
        }
    }

    let mut output = format!("{DESKTOP_ENTRY}\n");
    for (key, value) in entries {
        _ = writeln!(output, "{key}={value}");
    }
    output + &groups
}

/// Generate a thumbnailer that runs the executable with the thumbnailer arguments.
pub(crate) fn thumbnailer(package: &Package, executable: &str, arguments: &str) -> String {
    let mime_types = package
        .metadata
        .bundle
        .linux
        .mime_types
        .as_deref()
        .unwrap_or_default();
    assert!(
        !mime_types.is_empty(),
        "A thumbnailer needs the mime_types it creates thumbnails for"
    );
    format!(
        "[Thumbnailer Entry]\nTryExec={executable}\nExec={executable} {arguments}\nMimeType={}\n",
        list(mime_types)
    )
}

/// Generate AppStream metainfo, so software centers show the installed application.
pub(crate) fn metainfo(package: &Package, summary: &str) -> String {
    let bundle = &package.metadata.bundle;
    let identifier = xml_escape(&bundle.identifier);
    let mut output = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
    <id>{identifier}</id>
    <metadata_license>CC0-1.0</metadata_license>
"#
    );
    if let Some(license) = &package.license {
        _ = writeln!(
            output,
            "    <project_license>{}</project_license>",
            xml_escape(license)
        );
    }
    let summary = xml_escape(summary);
    _ = write!(
        output,
        "    <name>{}</name>\n    <summary>{summary}</summary>\n    <description>\n        \
         <p>{summary}.</p>\n    </description>\n",
        xml_escape(&bundle.name)
    );
    if let Some(author) = package.authors.first() {
        let name = author.split('<').next().unwrap_or(author).trim();
        let id = bundle
            .identifier
            .rsplit_once('.')
            .map_or(bundle.identifier.as_str(), |(prefix, _)| prefix);
        _ = writeln!(
            output,
            "    <developer id=\"{}\">\n        <name>{}</name>\n    </developer>",
            xml_escape(id),
            xml_escape(name)
        );
    }
    if let Some(url) = package.homepage.as_ref().or(package.repository.as_ref()) {
        _ = writeln!(
            output,
            "    <url type=\"homepage\">{}</url>",
            xml_escape(url)
        );
    }
    _ = writeln!(
        output,
        "    <launchable type=\"desktop-id\">{identifier}.desktop</launchable>"
    );
    if let Some(mime_types) = &bundle.linux.mime_types {
        output.push_str("    <provides>\n");
        for mime_type in mime_types {
            _ = writeln!(
                output,
                "        <mediatype>{}</mediatype>",
                xml_escape(mime_type)
            );
        }
        output.push_str("    </provides>\n");
    }
    output.push_str("    <content_rating type=\"oars-1.1\" />\n</component>\n");
    output
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;

    fn package() -> Package {
        Manifest::parse(
            r#"
[package]
name = "example"
version = "0.1.0"
description = "View examples"
license = "MIT"
authors = ["Example Author <author@example.com>"]
repository = "https://example.com/example"

[package.metadata.bundle]
name = "Example"
identifier = "com.example.Example"

[package.metadata.bundle.linux]
categories = ["Graphics", "Viewer"]
mime_types = ["image/png", "image/x-example"]
"#,
            None,
        )
        .expect("valid manifest")
        .package
    }

    #[test]
    fn desktop_entry_merges_custom_keys_and_groups() {
        let custom = "# Extra keys\n[Desktop Entry]\nTerminal=true\nKeywords=view;\n\n\
                      [Desktop Action new]\nName=New Window\nExec=example --new\n";
        assert_eq!(
            desktop_entry(&package(), "/usr/bin/example", true, Some(custom)),
            "[Desktop Entry]\nType=Application\nVersion=1.5\nName=Example\nComment=View examples\n\
             Exec=/usr/bin/example %F\nIcon=com.example.Example\nTerminal=true\n\
             Categories=Graphics;Viewer;\nMimeType=image/png;image/x-example;\nKeywords=view;\n\
             \n[Desktop Action new]\nName=New Window\nExec=example --new\n"
        );
    }

    #[test]
    fn thumbnailer_runs_the_executable() {
        assert_eq!(
            thumbnailer(&package(), "/usr/bin/example", "--thumbnail %s %u %o"),
            "[Thumbnailer Entry]\nTryExec=/usr/bin/example\n\
             Exec=/usr/bin/example --thumbnail %s %u %o\nMimeType=image/png;image/x-example;\n"
        );
    }

    #[test]
    fn metainfo_describes_the_application() {
        let metainfo = metainfo(&package(), "View examples");
        for expected in [
            "<id>com.example.Example</id>",
            "<project_license>MIT</project_license>",
            "<summary>View examples</summary>",
            "<developer id=\"com.example\">\n        <name>Example Author</name>",
            "<url type=\"homepage\">https://example.com/example</url>",
            "<launchable type=\"desktop-id\">com.example.Example.desktop</launchable>",
            "<mediatype>image/x-example</mediatype>",
        ] {
            assert!(metainfo.contains(expected), "{expected} in {metainfo}");
        }
    }
}
