/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;

use serde::Deserialize;

pub(crate) struct Manifest {
    pub package: Package,
}

impl Manifest {
    /// Read the crate manifest, resolving fields inherited from the workspace.
    pub(crate) fn read(path: &str) -> Result<Self, String> {
        let manifest_path = format!("{path}/Cargo.toml");
        let source = fs::read_to_string(&manifest_path)
            .map_err(|error| format!("Can't read {manifest_path} file: {error}"))?;
        let workspace = fs::canonicalize(path)
            .map_err(|error| format!("Can't resolve {path}: {error}"))?
            .ancestors()
            .filter_map(|directory| fs::read_to_string(directory.join("Cargo.toml")).ok())
            .find(|source| {
                toml::from_str::<WorkspaceManifest>(source)
                    .is_ok_and(|manifest| manifest.workspace.is_some())
            });
        Self::parse(&source, workspace.as_deref())
            .map_err(|error| format!("Can't parse {manifest_path} file: {error}"))
    }

    pub(crate) fn parse(source: &str, workspace: Option<&str>) -> Result<Self, toml::de::Error> {
        let raw: RawManifest = toml::from_str(source)?;
        let inherited = match workspace {
            Some(workspace) => toml::from_str::<WorkspaceManifest>(workspace)?
                .workspace
                .and_then(|workspace| workspace.package)
                .unwrap_or_default(),
            None => WorkspacePackage::default(),
        };
        let raw = raw.package;
        let mut metadata = raw.metadata;
        if metadata.bundle.name.is_empty() {
            metadata.bundle.name.clone_from(&raw.name);
        }
        if metadata.bundle.identifier.is_empty() {
            metadata.bundle.identifier = format!("nl.bplaat.{}", raw.name);
        }
        Ok(Self {
            package: Package {
                name: raw.name,
                version: raw.version,
                authors: Inheritable::resolve(raw.authors, inherited.authors).unwrap_or_default(),
                description: Inheritable::resolve(raw.description, inherited.description),
                license: Inheritable::resolve(raw.license, inherited.license),
                homepage: Inheritable::resolve(raw.homepage, inherited.homepage),
                repository: Inheritable::resolve(raw.repository, inherited.repository),
                metadata,
            },
        })
    }
}

#[derive(Deserialize)]
struct RawManifest {
    package: RawPackage,
}

#[derive(Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    authors: Option<Inheritable<Vec<String>>>,
    description: Option<Inheritable<String>>,
    license: Option<Inheritable<String>>,
    homepage: Option<Inheritable<String>>,
    repository: Option<Inheritable<String>>,
    #[serde(default)]
    metadata: PackageMetadata,
}

/// A package field that is set directly or with `field.workspace = true`.
#[derive(Deserialize)]
#[serde(untagged)]
enum Inheritable<T> {
    Value(T),
    Workspace { workspace: bool },
}

impl<T> Inheritable<T> {
    fn resolve(field: Option<Self>, inherited: Option<T>) -> Option<T> {
        match field? {
            Self::Value(value) => Some(value),
            Self::Workspace { workspace } => inherited.filter(|_| workspace),
        }
    }
}

#[derive(Deserialize)]
struct WorkspaceManifest {
    workspace: Option<Workspace>,
}

#[derive(Deserialize)]
struct Workspace {
    package: Option<WorkspacePackage>,
}

#[derive(Default, Deserialize)]
struct WorkspacePackage {
    authors: Option<Vec<String>>,
    description: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
}

#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub(crate) struct Package {
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub metadata: PackageMetadata,
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
    #[serde(default)]
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub linux: LinuxMetadata,
    pub android: Option<AndroidMetadata>,
}

#[derive(Clone, Default, Deserialize)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct LinuxMetadata {
    pub icons: Option<String>,
    pub categories: Option<Vec<String>>,
    pub mime_types: Option<Vec<String>>,
    pub mime_info: Option<String>,
    pub thumbnailer: Option<String>,
    pub desktop_entry: Option<String>,
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
            let manifest = Manifest::parse(&format!("{source}{metadata}"), None)
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
        let manifest = Manifest::parse(source, None).expect("valid manifest");
        assert_eq!(manifest.package.metadata.bundle.name, "example-app");
        assert_eq!(
            manifest.package.metadata.bundle.identifier,
            "nl.bplaat.example-app"
        );
        let manifest = Manifest::parse(&format!("{source}name = \"Example App\"\n"), None)
            .expect("valid manifest with override");
        assert_eq!(manifest.package.metadata.bundle.name, "Example App");
        assert_eq!(
            manifest.package.metadata.bundle.identifier,
            "nl.bplaat.example-app"
        );
        let manifest =
            Manifest::parse(&format!("{source}identifier = \"com.example.App\"\n"), None)
                .expect("valid manifest with identifier override");
        assert_eq!(
            manifest.package.metadata.bundle.identifier,
            "com.example.App"
        );
    }

    #[test]
    fn package_fields_inherit_from_workspace() {
        let source = r#"
[package]
name = "example-app"
version = "0.1.0"
description = "An example"
license.workspace = true
authors.workspace = true
homepage.workspace = false
"#;
        let workspace = r#"
[workspace.package]
license = "MIT"
authors = ["Example Author <author@example.com>"]
homepage = "https://example.com"
"#;
        let package = Manifest::parse(source, Some(workspace))
            .expect("valid manifest")
            .package;
        assert_eq!(package.description.as_deref(), Some("An example"));
        assert_eq!(package.license.as_deref(), Some("MIT"));
        assert_eq!(package.authors, ["Example Author <author@example.com>"]);
        assert_eq!(package.homepage, None);
        assert_eq!(package.repository, None);
    }
}
