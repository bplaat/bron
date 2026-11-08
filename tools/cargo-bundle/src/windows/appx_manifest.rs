/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write as _;

use crate::manifest::{FileTypeIcon, Manifest, ShellExtensionMetadata, WindowsMetadata};

// Marks the package as unsigned so it can only be installed with `Add-AppxPackage -AllowUnsigned`
// https://learn.microsoft.com/en-us/windows/msix/package/unsigned-package
const UNSIGNED_PUBLISHER_OID: &str = "OID.2.25.311729368913984317654407730594956997722=1";

// Unsigned packages are only supported on Windows 11
const MIN_OS_VERSION: &str = "10.0.22000.0";
const MAX_OS_VERSION_TESTED: &str = "10.0.26100.0";

// The publisher of the Windows App Runtime framework packages
const MICROSOFT_PUBLISHER: &str =
    "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US";

// The framework package and minimum package version of a Windows App SDK release, like
// Microsoft.WindowsAppRuntime.2 2.0.0.0 for "2.0". Since 2.0 the framework is named after the
// major version and its package version is the release version.
fn windows_app_runtime(version: &str) -> (String, String) {
    let parts = version
        .split('.')
        .map(|part| part.parse::<u16>().ok())
        .collect::<Option<Vec<_>>>()
        .filter(|parts| (1..=4).contains(&parts.len()))
        .unwrap_or_else(|| panic!("Invalid Windows App SDK version: {version}"));
    assert!(
        parts[0] >= 2,
        "Windows App SDK {version} is not supported, use 2.0 or newer"
    );
    let min_version = (0..4)
        .map(|index| parts.get(index).copied().unwrap_or(0).to_string())
        .collect::<Vec<_>>()
        .join(".");
    (
        format!("Microsoft.WindowsAppRuntime.{}", parts[0]),
        min_version,
    )
}

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
    xmlns:desktop2="http://schemas.microsoft.com/appx/manifest/desktop/windows10/2"
    xmlns:desktop6="http://schemas.microsoft.com/appx/manifest/desktop/windows10/6"
    xmlns:desktop7="http://schemas.microsoft.com/appx/manifest/desktop/windows10/7"
    xmlns:com="http://schemas.microsoft.com/appx/manifest/com/windows10"
    IgnorableNamespaces="uap uap10 rescap desktop2 desktop6 desktop7 com">
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
    if let Some(version) = &windows.windows_app_sdk {
        let (name, min_version) = windows_app_runtime(version);
        _ = writeln!(
            xml,
            r#"        <PackageDependency Name="{name}" MinVersion="{min_version}" Publisher="{MICROSOFT_PUBLISHER}"/>"#
        );
    }
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
    let file_types = windows.file_types.as_deref().unwrap_or_default();
    let shell_extension = windows.shell_extension.as_ref();
    if !file_types.is_empty() || shell_extension.is_some() {
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
            match &file_type.icon {
                Some(FileTypeIcon::Resource(id)) => {
                    // Windows copies this value into DefaultIcon: positive values are
                    // zero-based icon indexes, negative values select resource IDs.
                    _ = writeln!(
                        xml,
                        r#"                        <desktop7:Logo ResourceId="-{id}">{}</desktop7:Logo>"#,
                        xml_escape(executable)
                    );
                }
                Some(FileTypeIcon::File(icon)) => {
                    let element = if icon.ends_with(".ico") {
                        "desktop7:Logo"
                    } else {
                        "uap:Logo"
                    };
                    _ = writeln!(
                        xml,
                        r#"                        <{element}>Assets\{}</{element}>"#,
                        xml_escape(&icon.replace('/', "\\"))
                    );
                }
                None => {}
            }
            _ = writeln!(xml, "                        <uap:SupportedFileTypes>");
            for extension in &file_type.extensions {
                let attributes = match &file_type.perceived_type {
                    Some(perceived_type) => {
                        format!(r#" uap10:PerceivedType="{}""#, xml_escape(perceived_type))
                    }
                    None => String::new(),
                };
                _ = writeln!(
                    xml,
                    "                            <uap:FileType{attributes}>.{}</uap:FileType>",
                    xml_escape(extension.trim_start_matches('.'))
                );
            }
            _ = writeln!(xml, "                        </uap:SupportedFileTypes>");
            if let Some(clsid) =
                shell_extension.and_then(|shell| shell.thumbnail_handler.as_deref())
            {
                _ = writeln!(
                    xml,
                    r#"                        <desktop2:ThumbnailHandler Clsid="{}"/>"#,
                    clsid_attribute(clsid)
                );
            }
            if let Some(clsid) = shell_extension.and_then(|shell| shell.preview_handler.as_deref())
            {
                _ = writeln!(
                    xml,
                    r#"                        <desktop2:DesktopPreviewHandler Clsid="{}"/>"#,
                    clsid_attribute(clsid)
                );
            }
            _ = writeln!(xml, "                    </uap:FileTypeAssociation>");
            _ = writeln!(xml, "                </uap:Extension>");
        }
        if let Some(shell) = shell_extension {
            // The handlers run out of process in a COM surrogate, like Explorer requires
            _ = writeln!(
                xml,
                r#"                <com:Extension Category="windows.comServer">"#
            );
            _ = writeln!(xml, "                    <com:ComServer>");
            _ = writeln!(
                xml,
                r#"                        <com:SurrogateServer DisplayName="{name}">"#
            );
            for clsid in [&shell.thumbnail_handler, &shell.preview_handler]
                .into_iter()
                .flatten()
            {
                _ = writeln!(
                    xml,
                    r#"                            <com:Class Id="{}" Path="{}" ThreadingModel="STA"/>"#,
                    clsid_attribute(clsid),
                    xml_escape(&shell_extension_library(shell))
                );
            }
            _ = writeln!(xml, "                        </com:SurrogateServer>");
            _ = writeln!(xml, "                    </com:ComServer>");
            _ = writeln!(xml, "                </com:Extension>");
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

/// The file name of the shell extension DLL in the package.
pub(crate) fn shell_extension_library(shell: &ShellExtensionMetadata) -> String {
    format!("{}.dll", shell.library)
}

// Validates a CLSID and formats it without braces, like the manifest schema expects.
fn clsid_attribute(clsid: &str) -> String {
    let clsid = clsid.trim_start_matches('{').trim_end_matches('}');
    let groups: Vec<_> = clsid.split('-').collect();
    assert!(
        groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && groups
                .iter()
                .all(|group| group.chars().all(|char| char.is_ascii_hexdigit())),
        "Invalid shell extension CLSID {clsid}"
    );
    clsid.to_ascii_uppercase()
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
        let manifest: Manifest = basic_toml::from_str(
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
    fn embedded_file_icons_reference_executable_resources() {
        let manifest: Manifest = basic_toml::from_str(
            r#"
[package]
name = "viewer"
version = "1.0.0"
[package.metadata.bundle.windows]
file_types = [
    { name = "PNG Image", extensions = ["png", "apng"], icon = 2 },
    { name = "JPEG Image", extensions = ["jpg", "jpeg"], icon = 3 },
    { name = "Other Image", extensions = ["other"] },
]
"#,
        )
        .expect("Manifest");
        let windows = manifest
            .package
            .metadata
            .bundle
            .windows
            .as_ref()
            .expect("Windows metadata");
        let xml = appx_manifest(
            &manifest,
            windows,
            "Publisher",
            "viewer&app.exe",
            "arm64",
            0,
        );
        for id in [2, 3] {
            assert!(xml.contains(&format!(
                r#"<desktop7:Logo ResourceId="-{id}">viewer&amp;app.exe</desktop7:Logo>"#
            )));
            assert!(!xml.contains(&format!(r#"ResourceId="{id}""#)));
        }
        assert_eq!(xml.matches("<desktop7:Logo").count(), 2);
        assert!(!xml.contains("Assets\\file-types"));
        assert!(xml.contains("<uap:FileType>.apng</uap:FileType>"));
        assert!(xml.contains("<uap:FileType>.jpeg</uap:FileType>"));
    }

    #[test]
    fn shell_extension_registers_its_handlers_for_every_file_type() {
        let manifest: Manifest = basic_toml::from_str(
            r#"
[package]
name = "viewer"
version = "1.0.0"
[package.metadata.bundle.windows]
file_types = [{ name = "PNG Image", extensions = ["png"], icon = "png.png" }, { name = "QOI Image", extensions = ["qoi"], perceived_type = "image", icon = "file-types/qoi.ico" }]
[package.metadata.bundle.windows.shell_extension]
package = "../viewer-shell"
library = "viewer-shell"
thumbnail_handler = "{a1b2c3d4-0000-4000-8000-000000000001}"
preview_handler = "A1B2C3D4-0000-4000-8000-000000000002"
"#,
        )
        .expect("Manifest");
        let windows = manifest
            .package
            .metadata
            .bundle
            .windows
            .clone()
            .expect("Windows metadata");
        let xml = appx_manifest(&manifest, &windows, "Publisher", "viewer.exe", "x64", 0);
        assert!(xml.contains("<uap:FileType>.png</uap:FileType>"));
        assert!(xml.contains(r#"<uap:FileType uap10:PerceivedType="image">.qoi</uap:FileType>"#));
        assert!(xml.contains(r#"<desktop7:Logo>Assets\file-types\qoi.ico</desktop7:Logo>"#));
        assert!(xml.contains(r#"<uap:Logo>Assets\png.png</uap:Logo>"#));
        assert_eq!(
            xml.matches(
                r#"<desktop2:ThumbnailHandler Clsid="A1B2C3D4-0000-4000-8000-000000000001"/>"#
            )
            .count(),
            2
        );
        assert_eq!(
            xml.matches(
                r#"<desktop2:DesktopPreviewHandler Clsid="A1B2C3D4-0000-4000-8000-000000000002"/>"#
            )
            .count(),
            2
        );
        assert!(xml.contains(r#"<com:SurrogateServer DisplayName="viewer">"#));
        assert!(xml.contains(
            r#"<com:Class Id="A1B2C3D4-0000-4000-8000-000000000001" Path="viewer-shell.dll" ThreadingModel="STA"/>"#
        ));
        assert!(xml.contains(
            r#"<com:Class Id="A1B2C3D4-0000-4000-8000-000000000002" Path="viewer-shell.dll" ThreadingModel="STA"/>"#
        ));
        // The handlers follow the file types inside each association.
        assert!(xml.find("</uap:SupportedFileTypes>") < xml.find("<desktop2:ThumbnailHandler"));
    }

    #[test]
    fn windows_app_sdk_adds_its_framework_dependency() {
        let manifest: Manifest = basic_toml::from_str(
            r#"
[package]
name = "viewer"
version = "1.0.0"
[package.metadata.bundle.windows]
windows_app_sdk = "2.0"
"#,
        )
        .expect("Manifest");
        let windows = manifest
            .package
            .metadata
            .bundle
            .windows
            .clone()
            .expect("Windows metadata");
        let xml = appx_manifest(&manifest, &windows, "Publisher", "viewer.exe", "x64", 0);
        assert!(xml.contains(
            r#"<PackageDependency Name="Microsoft.WindowsAppRuntime.2" MinVersion="2.0.0.0" Publisher="CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US"/>"#
        ));
        assert!(xml.find("<TargetDeviceFamily") < xml.find("<PackageDependency"));
    }

    #[test]
    fn windows_app_runtime_versions_have_four_parts() {
        assert_eq!(
            windows_app_runtime("2.4.1"),
            (
                "Microsoft.WindowsAppRuntime.2".to_owned(),
                "2.4.1.0".to_owned()
            )
        );
    }

    #[test]
    #[should_panic(expected = "use 2.0 or newer")]
    fn windows_app_sdk_1_is_rejected() {
        windows_app_runtime("1.8");
    }

    #[test]
    #[should_panic(expected = "Invalid shell extension CLSID")]
    fn malformed_clsids_are_rejected() {
        clsid_attribute("not-a-clsid");
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
