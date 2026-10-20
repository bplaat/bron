/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::Os;
use crate::utils::{normalize_path, relative_slash};

pub(crate) struct InstallableApp {
    pub(crate) package: String,
    pub(crate) directory: PathBuf,
    pub(crate) name: String,
    pub(crate) identifier: String,
    /// Has a [package.metadata.bundle.ios] table
    pub(crate) ios: bool,
}

pub(crate) fn packages(metadata: &Value) -> Result<&[Value]> {
    metadata
        .get("packages")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .context("cargo metadata has no packages")
}

pub(crate) fn platform_excludes(metadata: &Value, os: Os) -> BTreeSet<String> {
    packages(metadata)
        .unwrap_or_default()
        .iter()
        .filter(|package| !supports_os(package, os))
        .filter_map(|package| {
            package
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

pub(crate) fn platform_exclude_paths(
    metadata: &Value,
    os: Os,
    root: &Path,
) -> Result<BTreeSet<String>> {
    packages(metadata)?
        .iter()
        .filter(|package| !supports_os(package, os))
        .map(|package| {
            let manifest = package
                .get("manifest_path")
                .and_then(Value::as_str)
                .context("Cargo package has no manifest path")?;
            let directory = normalize_path(Path::new(manifest), root)
                .parent()
                .context("Cargo package manifest has no parent directory")?
                .to_owned();
            Ok(relative_slash(root, &directory))
        })
        .collect()
}

pub(crate) fn supports_os(package: &Value, os: Os) -> bool {
    package
        .pointer("/metadata/platforms")
        .and_then(Value::as_array)
        .is_none_or(|platforms| {
            platforms
                .iter()
                .any(|platform| platform.as_str() == Some(os.name()))
        })
}

pub(crate) fn add_excludes(command: &mut Command, excludes: &BTreeSet<String>) {
    for package in excludes {
        command.args(["--exclude", package]);
    }
}

pub(crate) fn features_without_swap(
    metadata: &Value,
    package_name: &str,
    swap_feature: &str,
) -> Result<Vec<String>> {
    let package = packages(metadata)?
        .iter()
        .find(|package| package.get("name").and_then(Value::as_str) == Some(package_name))
        .with_context(|| format!("package not found in cargo metadata: {package_name}"))?;
    let features = package
        .get("features")
        .and_then(Value::as_object)
        .context("Cargo package has no features object")?;
    let remaining = features
        .keys()
        .filter(|feature| feature.as_str() != "default" && feature.as_str() != swap_feature)
        .cloned()
        .collect::<Vec<_>>();
    if remaining.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec!["--features".to_owned(), remaining.join(",")])
    }
}

pub(crate) fn has_target_kind(package: &Value, kinds: &[&str]) -> bool {
    package
        .get("targets")
        .and_then(Value::as_array)
        .is_some_and(|targets| {
            targets.iter().any(|target| {
                target
                    .get("kind")
                    .and_then(Value::as_array)
                    .is_some_and(|target_kinds| {
                        target_kinds
                            .iter()
                            .filter_map(Value::as_str)
                            .any(|kind| kinds.contains(&kind))
                    })
            })
        })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::env;

    use serde_json::json;

    use super::*;

    #[test]
    fn platform_support_defaults_to_every_os() {
        let package = json!({ "metadata": {} });
        assert!(supports_os(&package, Os::Linux));
        assert!(supports_os(&package, Os::Macos));
        assert!(supports_os(&package, Os::Windows));
    }

    #[test]
    fn platform_support_uses_cargo_metadata() {
        let package = json!({ "metadata": { "platforms": ["linux", "windows"] } });
        assert!(supports_os(&package, Os::Linux));
        assert!(!supports_os(&package, Os::Macos));
        assert!(supports_os(&package, Os::Windows));
    }

    #[test]
    fn backend_swap_feature_is_excluded() -> Result<()> {
        let metadata = json!({
            "packages": [{
                "name": "example",
                "features": {
                    "default": [],
                    "bundled": [],
                    "chrono": [],
                    "uuid": []
                }
            }]
        });
        assert_eq!(
            features_without_swap(&metadata, "example", "bundled")?,
            ["--features", "chrono,uuid"]
        );
        Ok(())
    }

    #[test]
    fn backend_swap_can_leave_no_features() -> Result<()> {
        let metadata = json!({
            "packages": [{
                "name": "example",
                "features": { "default": [], "vendored": [] }
            }]
        });
        assert!(features_without_swap(&metadata, "example", "vendored")?.is_empty());
        Ok(())
    }

    #[test]
    fn platform_excludes_lists_unsupported_packages() {
        let metadata = json!({
            "packages": [
                { "name": "everywhere", "metadata": {} },
                { "name": "unix-only", "metadata": { "platforms": ["linux", "macos"] } },
                { "name": "win-only", "metadata": { "platforms": ["windows"] } }
            ]
        });
        let excludes = platform_excludes(&metadata, Os::Windows);
        assert!(excludes.contains("unix-only"));
        assert!(!excludes.contains("everywhere"));
        assert!(!excludes.contains("win-only"));
    }

    #[test]
    fn platform_exclude_paths_lists_unsupported_package_directories() -> Result<()> {
        let root = env::temp_dir().join("workspace");
        let metadata = json!({
            "packages": [
                {
                    "manifest_path": root.join("bin/macos-app/Cargo.toml"),
                    "metadata": { "platforms": ["macos"] }
                },
                {
                    "manifest_path": "bin/windows-app/Cargo.toml",
                    "metadata": { "platforms": ["windows"] }
                }
            ]
        });
        assert_eq!(
            platform_exclude_paths(&metadata, Os::Linux, &root)?,
            ["bin/macos-app".to_owned(), "bin/windows-app".to_owned()]
                .into_iter()
                .collect()
        );
        Ok(())
    }

    #[test]
    fn packages_errors_when_absent() {
        assert!(packages(&json!({})).is_err());
    }

    #[test]
    fn add_excludes_appends_flags_in_order() {
        let mut command = Command::new("cargo");
        let excludes = ["alpha".to_owned(), "beta".to_owned()]
            .into_iter()
            .collect::<BTreeSet<_>>();
        add_excludes(&mut command, &excludes);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(args, ["--exclude", "alpha", "--exclude", "beta"]);
    }

    #[test]
    fn has_target_kind_matches_any_requested_kind() {
        let package = json!({ "targets": [{ "kind": ["bin"] }, { "kind": ["proc-macro"] }] });
        assert!(has_target_kind(&package, &["lib", "proc-macro"]));
        assert!(has_target_kind(&package, &["bin"]));
        assert!(!has_target_kind(&package, &["lib"]));
        assert!(!has_target_kind(&json!({}), &["bin"]));
    }
}
