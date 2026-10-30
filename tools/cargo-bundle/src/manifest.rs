/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(from = "RawManifest")]
pub(crate) struct Manifest {
    pub package: Package,
}

#[derive(Deserialize)]
struct RawManifest {
    package: Package,
}

impl From<RawManifest> for Manifest {
    fn from(raw: RawManifest) -> Self {
        let mut package = raw.package;
        if package.metadata.bundle.name.is_empty() {
            package.metadata.bundle.name.clone_from(&package.name);
        }
        if package.metadata.bundle.identifier.is_empty() {
            package.metadata.bundle.identifier = format!("nl.bplaat.{}", package.name);
        }
        Self { package }
    }
}

#[derive(Deserialize)]
pub(crate) struct Package {
    pub name: String,
    pub version: String,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub authors: Option<Authors>,
    #[serde(default)]
    pub metadata: PackageMetadata,
}

#[derive(Deserialize)]
#[serde(untagged)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Authors {
    List(Vec<String>),
    Workspace { workspace: bool },
}

#[derive(Default, Deserialize)]
pub(crate) struct PackageMetadata {
    #[serde(default)]
    pub bundle: BundleMetadata,
}

#[derive(Clone, Default, Deserialize)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct BundleMetadata {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub identifier: String,
    pub copyright: Option<String>,
    pub resources_dir: Option<String>,
    pub icon: Option<String>,
    pub info_plist: Option<String>,
    #[serde(default)]
    pub macos: MacosMetadata,
    #[serde(default)]
    pub ios: IosMetadata,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub windows: Option<WindowsMetadata>,
    pub android: Option<AndroidMetadata>,
}

#[derive(Clone, Default, Deserialize)]
pub(crate) struct AndroidMetadata {
    pub min_sdk_version: Option<u32>,
    pub target_sdk_version: Option<u32>,
    pub manifest: Option<String>,
    pub res_dir: Option<String>,
}

#[derive(Clone, Deserialize)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct AppexMetadata {
    pub name: String,
    pub identifier: String,
    pub package: String,
    pub binary: String,
    pub minimal_os_version: Option<String>,
    pub info_plist: String,
    pub entitlements: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct MacosMetadata {
    pub minimal_os_version: Option<String>,
    pub lipo: Option<bool>,
    pub iconset: Option<String>,
    pub icns: Option<String>,
    pub info_plist: Option<String>,
    pub entitlements: Option<String>,
    pub hardened_runtime: Option<bool>,
    pub appex: Option<Vec<AppexMetadata>>,
}

#[derive(Clone, Default, Deserialize)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct IosMetadata {
    pub info_plist: Option<String>,
    pub entitlements: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct WindowsMetadata {
    pub assets: Option<String>,
    pub sandbox: Option<bool>,
    pub virtualization: Option<bool>,
    pub capabilities: Option<Vec<String>>,
    pub file_types: Option<Vec<FileTypeMetadata>>,
    pub shell_extension: Option<ShellExtensionMetadata>,
}

#[derive(Clone, Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct ShellExtensionMetadata {
    pub package: String,
    pub library: String,
    pub thumbnail_handler: Option<String>,
    pub preview_handler: Option<String>,
}

#[derive(Clone, Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct FileTypeMetadata {
    pub name: String,
    pub extensions: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::Manifest;

    #[test]
    fn bundle_metadata_is_optional() {
        let source = "[package]\nname = \"example-app\"\nversion = \"0.1.0\"\n";
        for metadata in ["", "[package.metadata]\n", "[package.metadata.bundle]\n"] {
            let manifest: Manifest = basic_toml::from_str(&format!("{source}{metadata}"))
                .expect("valid manifest without bundle options");
            assert_eq!(manifest.package.metadata.bundle.name, "example-app");
            assert_eq!(
                manifest.package.metadata.bundle.identifier,
                "nl.bplaat.example-app"
            );
        }
    }

    #[test]
    fn bundle_name_defaults_to_package_name() {
        let source = r#"
[package]
name = "example-app"
version = "0.1.0"
[package.metadata.bundle]
"#;
        let manifest: Manifest = basic_toml::from_str(source).expect("valid manifest");
        assert_eq!(manifest.package.metadata.bundle.name, "example-app");
        assert_eq!(
            manifest.package.metadata.bundle.identifier,
            "nl.bplaat.example-app"
        );
        let manifest: Manifest = basic_toml::from_str(&format!("{source}name = \"Example App\"\n"))
            .expect("valid manifest with override");
        assert_eq!(manifest.package.metadata.bundle.name, "Example App");
        assert_eq!(
            manifest.package.metadata.bundle.identifier,
            "nl.bplaat.example-app"
        );
        let manifest: Manifest =
            basic_toml::from_str(&format!("{source}identifier = \"com.example.App\"\n"))
                .expect("valid manifest with identifier override");
        assert_eq!(
            manifest.package.metadata.bundle.identifier,
            "com.example.App"
        );
    }
}
