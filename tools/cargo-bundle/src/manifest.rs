/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct Manifest {
    pub package: Package,
}

#[derive(Deserialize)]
pub(crate) struct Package {
    pub name: String,
    pub version: String,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub authors: Option<Authors>,
    pub metadata: PackageMetadata,
}

#[derive(Deserialize)]
#[serde(untagged)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) enum Authors {
    List(Vec<String>),
    Workspace { workspace: bool },
}

#[derive(Deserialize)]
pub(crate) struct PackageMetadata {
    pub bundle: BundleMetadata,
}

#[derive(Clone, Deserialize)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct BundleMetadata {
    pub name: String,
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
}

#[derive(Clone, Deserialize)]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) struct FileTypeMetadata {
    pub name: String,
    pub extensions: Vec<String>,
}
