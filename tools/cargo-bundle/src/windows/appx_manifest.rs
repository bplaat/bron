/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write as _;

use crate::manifest::{Manifest, WindowsMetadata};

// Marks the package as unsigned so it can only be installed with `Add-AppxPackage -AllowUnsigned`
// https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package
const UNSIGNED_PUBLISHER_OID: &str = "OID.2.25.311729368913984317654407730594956997722=1";

// Unsigned packages are only supported on Windows 11
const MIN_OS_VERSION: &str = "10.0.22000.0";
const MAX_OS_VERSION_TESTED: &str = "10.0.26100.0";

pub(crate) fn appx_manifest(
    manifest: &Manifest,
    windows: &WindowsMetadata,
    publisher: &str,
    executable: &str,
    arch: &str,
    revision: u16,
) -> String {
    let bundle = &manifest.package.metadata.bundle;
    let name = xml_escape(&bundle.name);
    let sandbox = windows.sandbox.unwrap_or(false);
    let virtualization = windows.virtualization.unwrap_or(true);
    assert!(
        !(sandbox && !virtualization),
        "virtualization can only be disabled for apps that are not sandboxed"
    );

    let mut capabilities = windows.capabilities.clone().unwrap_or_default();
    if !sandbox {
        capabilities.push("runFullTrust".to_owned());
    }
    if !virtualization {
        capabilities.push("unvirtualizedResources".to_owned());
    }

    let mut xml = String::new();
    xml.push_str(r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
    xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
    xmlns:uap10="http://schemas.microsoft.com/appx/manifest/uap/windows10/10"
    xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
    xmlns:desktop6="http://schemas.microsoft.com/appx/manifest/desktop/windows10/6"
    IgnorableNamespaces="uap uap10 rescap desktop6">
"#);
    _ = writeln!(
        xml,
        r#"    <Identity Name="{}" Publisher="CN={}, {UNSIGNED_PUBLISHER_OID}" Version="{}" ProcessorArchitecture="{arch}"/>"#,
        xml_escape(&bundle.identifier),
        xml_escape(publisher),
        package_version(&manifest.package.version, revision),
    );
    _ = writeln!(xml, "    <Properties>");
    _ = writeln!(xml, "        <DisplayName>{name}</DisplayName>");
    _ = writeln!(
        xml,
        "        <PublisherDisplayName>{}</PublisherDisplayName>",
        xml_escape(publisher)
    );
    _ = writeln!(xml, r"        <Logo>Assets\StoreLogo.png</Logo>");
    if !virtualization {
        _ = writeln!(
            xml,
            "        <desktop6:FileSystemWriteVirtualization>disabled</desktop6:FileSystemWriteVirtualization>"
        );
        _ = writeln!(
            xml,
            "        <desktop6:RegistryWriteVirtualization>disabled</desktop6:RegistryWriteVirtualization>"
        );
    }
    _ = writeln!(xml, "    </Properties>");
    _ = writeln!(xml, "    <Dependencies>");
    _ = writeln!(
        xml,
        r#"        <TargetDeviceFamily Name="Windows.Desktop" MinVersion="{MIN_OS_VERSION}" MaxVersionTested="{MAX_OS_VERSION_TESTED}"/>"#
    );
    _ = writeln!(xml, "    </Dependencies>");
    _ = writeln!(xml, "    <Resources>");
    _ = writeln!(xml, r#"        <Resource Language="en-us"/>"#);
    _ = writeln!(xml, "    </Resources>");
    _ = writeln!(xml, "    <Applications>");
    _ = writeln!(
        xml,
        r#"        <Application Id="App" Executable="{}" uap10:RuntimeBehavior="packagedClassicApp" uap10:TrustLevel="{}">"#,
        xml_escape(executable),
        if sandbox { "appContainer" } else { "mediumIL" }
    );
    _ = writeln!(
        xml,
        r#"            <uap:VisualElements DisplayName="{name}" Description="{name}" BackgroundColor="transparent" Square150x150Logo="Assets\Square150x150Logo.png" Square44x44Logo="Assets\Square44x44Logo.png"/>"#
    );
    if let Some(file_types) = windows
        .file_types
        .as_ref()
        .filter(|types| !types.is_empty())
    {
        _ = writeln!(xml, "            <Extensions>");
        for file_type in file_types {
            _ = writeln!(
                xml,
                r#"                <uap:Extension Category="windows.fileTypeAssociation">"#
            );
            _ = writeln!(
                xml,
                r#"                    <uap:FileTypeAssociation Name="{}">"#,
                file_type_id(&file_type.name)
            );
            _ = writeln!(
                xml,
                "                        <uap:DisplayName>{}</uap:DisplayName>",
                xml_escape(&file_type.name)
            );
            _ = writeln!(xml, "                        <uap:SupportedFileTypes>");
            for extension in &file_type.extensions {
                _ = writeln!(
                    xml,
                    "                            <uap:FileType>.{}</uap:FileType>",
                    xml_escape(extension.trim_start_matches('.'))
                );
            }
            _ = writeln!(xml, "                        </uap:SupportedFileTypes>");
            _ = writeln!(xml, "                    </uap:FileTypeAssociation>");
            _ = writeln!(xml, "                </uap:Extension>");
        }
        _ = writeln!(xml, "            </Extensions>");
    }
    _ = writeln!(xml, "        </Application>");
    _ = writeln!(xml, "    </Applications>");
    if capabilities.is_empty() {
        _ = writeln!(xml, "    <Capabilities/>");
    } else {
        _ = writeln!(xml, "    <Capabilities>");
        // The schema requires capabilities ordered by namespace
        for namespace in ["", "uap:", "rescap:"] {
            for capability in capabilities
                .iter()
                .filter(|capability| capability_namespace(capability) == namespace)
            {
                _ = writeln!(
                    xml,
                    r#"        <{namespace}Capability Name="{}"/>"#,
                    xml_escape(capability)
                );
            }
        }
        _ = writeln!(xml, "    </Capabilities>");
    }
    _ = writeln!(xml, "</Package>");
    xml
}

// MakeAppx /r derives the neutral resource manifest from this main-package template.
pub(crate) fn asset_manifest_template(
    manifest: &Manifest,
    windows: &WindowsMetadata,
    publisher: &str,
    executable: &str,
    revision: u16,
) -> String {
    appx_manifest(manifest, windows, publisher, executable, "neutral", revision)
        .replace("    xmlns:uap=", "    xmlns:uap6=\"http://schemas.microsoft.com/appx/manifest/uap/windows10/6\"\n    xmlns:uap=")
        .replace("IgnorableNamespaces=\"uap ", "IgnorableNamespaces=\"uap6 uap ")
        .replace("    <Properties>", "    <Properties>\n        <uap6:AllowExecution>false</uap6:AllowExecution>")
}

fn capability_namespace(capability: &str) -> &'static str {
    match capability {
        "runFullTrust" | "allowElevation" | "unvirtualizedResources" | "broadFileSystemAccess" => {
            "rescap:"
        }
        "musicLibrary"
        | "picturesLibrary"
        | "videosLibrary"
        | "documentsLibrary"
        | "removableStorage"
        | "sharedUserCertificates"
        | "enterpriseAuthentication" => "uap:",
        _ => "",
    }
}

fn file_type_id(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|char| char.to_ascii_lowercase())
        .collect()
}

pub(crate) fn package_version(version: &str, revision: u16) -> String {
    let release = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts: Vec<_> = release.split('.').collect();
    assert!(parts.len() <= 3, "Package version has too many parts");
    for part in &parts {
        assert!(
            part.parse::<u16>().is_ok(),
            "Package version parts must be numbers from 0 to 65535"
        );
    }
    parts.resize(3, "0");
    format!("{}.{revision}", parts.join("."))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_template_preserves_family_and_version_without_enabling_execution() {
        let manifest: Manifest = toml::from_str(
            r#"
[package]
name = "example-game"
version = "1.2.3"
"#,
        )
        .expect("Manifest");
        let windows = WindowsMetadata::default();
        let main = appx_manifest(
            &manifest,
            &windows,
            "Example & Publisher",
            "game.exe",
            "x64",
            7,
        );
        let asset =
            asset_manifest_template(&manifest, &windows, "Example & Publisher", "game.exe", 7);
        for expected in [
            "Name=\"nl.bplaat.example-game\"",
            "Version=\"1.2.3.7\"",
            "Publisher=\"CN=Example &amp; Publisher,",
        ] {
            assert!(main.contains(expected));
            assert!(asset.contains(expected));
        }
        assert!(asset.contains("ProcessorArchitecture=\"neutral\""));
        assert!(asset.contains("<uap6:AllowExecution>false</uap6:AllowExecution>"));
        assert!(
            asset.contains(
                "xmlns:uap6=\"http://schemas.microsoft.com/appx/manifest/uap/windows10/6\""
            )
        );
        assert!(!main.contains("AllowExecution"));
    }

    #[test]
    fn package_version_has_four_parts() {
        assert_eq!(package_version("0.1.0", 0), "0.1.0.0");
        assert_eq!(package_version("1.2.3-beta.1", 7), "1.2.3.7");
        assert_eq!(package_version("4", 65535), "4.0.0.65535");
    }

    #[test]
    fn file_type_id_is_lowercase_alphanumeric() {
        assert_eq!(file_type_id("Pixel Font"), "pixelfont");
        assert_eq!(file_type_id("SQLite-3 Database"), "sqlite3database");
    }

    #[test]
    fn xml_escape_escapes_markup() {
        assert_eq!(
            xml_escape(r#"A & "B" <C>"#),
            "A &amp; &quot;B&quot; &lt;C&gt;"
        );
    }

    #[test]
    fn capabilities_use_their_namespace() {
        assert_eq!(capability_namespace("internetClient"), "");
        assert_eq!(capability_namespace("musicLibrary"), "uap:");
        assert_eq!(capability_namespace("runFullTrust"), "rescap:");
    }
}
