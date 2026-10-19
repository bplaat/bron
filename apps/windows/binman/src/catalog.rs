/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use crate::pattern;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Catalog {
    pub(crate) version: u32,
    pub(crate) groups: Vec<CleanupGroup>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupGroup {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) rules: Vec<CleanupDefinition>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupDefinition {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) impact: CleanupImpact,
    #[serde(default)]
    pub(crate) recovery: CleanupRecovery,
    #[serde(default)]
    pub(crate) side_effect: Option<String>,
    #[serde(default)]
    pub(crate) requires_administrator: bool,
    #[serde(default)]
    pub(crate) process_names: Vec<String>,
    #[serde(flatten)]
    pub(crate) kind: CleanupKind,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CleanupImpact {
    #[default]
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum CleanupRecovery {
    #[default]
    Regenerate,
    Redownload,
    Permanent,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum CleanupKind {
    Paths { rules: Vec<PathRule> },
    RecycleBin,
    WindowsUpdate,
    DeliveryOptimization,
    DismComponentStore,
    DockerBuildCache,
    RustToolchains,
    UvCache,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PathRule {
    pub(crate) root: PathRoot,
    pub(crate) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) minimum_age_days: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum PathRoot {
    UserProfile,
    LocalAppData,
    RoamingAppData,
    ProgramData,
    Windows,
    SystemDrive,
    ProgramFiles,
    ProgramFilesX86,
}

impl PathRoot {
    pub(crate) fn resolve(self) -> Option<PathBuf> {
        let name = match self {
            Self::UserProfile => return absolute(BaseDirs::new()?.home_dir()),
            Self::LocalAppData => return absolute(BaseDirs::new()?.data_local_dir()),
            Self::RoamingAppData => return absolute(BaseDirs::new()?.data_dir()),
            Self::ProgramData => "ProgramData",
            Self::Windows => "WINDIR",
            Self::SystemDrive => "SystemDrive",
            Self::ProgramFiles => "ProgramFiles",
            Self::ProgramFilesX86 => "ProgramFiles(x86)",
        };
        let mut value = std::env::var_os(name)?;
        if matches!(self, Self::SystemDrive) {
            value.push("\\");
        }
        absolute(PathBuf::from(value))
    }
}

fn absolute(path: PathBuf) -> Option<PathBuf> {
    path.is_absolute().then_some(path)
}

impl Catalog {
    pub(crate) fn load() -> Result<Self> {
        let catalog: Self = serde_json::from_str(include_str!("../rules.json"))?;
        catalog.validate()?;
        Ok(catalog)
    }

    fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported cleanup catalog version {}", self.version);
        }
        let mut ids = HashSet::new();
        for group in &self.groups {
            if group.id.is_empty() || group.name.is_empty() {
                bail!("Cleanup groups must have an id and name");
            }
            for cleanup in &group.rules {
                if !ids.insert(&cleanup.id) {
                    bail!("Duplicate cleanup rule id: {}", cleanup.id);
                }
                if cleanup.id.is_empty() || cleanup.name.is_empty() {
                    bail!("Cleanup rules must have an id and name");
                }
                if (cleanup.impact != CleanupImpact::Low
                    || cleanup.recovery == CleanupRecovery::Permanent)
                    && cleanup.side_effect.as_deref().is_none_or(str::is_empty)
                {
                    bail!("Cleanup rule {} must explain its side effect", cleanup.id);
                }
                if matches!(
                    cleanup.kind,
                    CleanupKind::WindowsUpdate
                        | CleanupKind::DeliveryOptimization
                        | CleanupKind::DismComponentStore
                ) && !cleanup.requires_administrator
                {
                    bail!(
                        "System cleanup rule {} must require administrator access",
                        cleanup.id
                    );
                }
                if let CleanupKind::Paths { rules } = &cleanup.kind {
                    if rules.is_empty() {
                        bail!("Path cleanup {} has no rules", cleanup.id);
                    }
                    for rule in rules {
                        validate_relative_pattern(&rule.path)
                            .map_err(|error| anyhow!("{}: {error}", cleanup.id))?;
                        if rule
                            .minimum_age_days
                            .is_some_and(|days| days == 0 || days > 3650)
                        {
                            bail!(
                                "Path cleanup {} must use a minimum age between 1 and 3650 days",
                                cleanup.id
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn rules(&self) -> impl Iterator<Item = &CleanupDefinition> {
        self.groups.iter().flat_map(|group| group.rules.iter())
    }
}

fn validate_relative_pattern(value: &str) -> Result<()> {
    if value.is_empty() || Path::new(value).is_absolute() || value.starts_with(['\\', '/']) {
        bail!("cleanup patterns must be non-empty relative paths");
    }
    if Path::new(value)
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        bail!("cleanup patterns may not escape their root");
    }
    pattern::validate(value).map_err(|error| anyhow!("invalid cleanup pattern: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_is_valid() {
        let catalog = Catalog::load().expect("catalog should load");
        assert!(!catalog.groups.is_empty());
        assert!(catalog.rules().count() >= 65);

        let ids: HashSet<_> = catalog.rules().map(|rule| rule.id.as_str()).collect();
        for id in [
            "android-cache",
            "brave-cache",
            "cargo-cache",
            "chrome-cache",
            "docker-build-cache",
            "deno-cache",
            "edge-cache",
            "firefox-cache",
            "rust-toolchains",
            "rustup-cache",
            "uv-cache",
            "windows-recycle-bin",
            "windows-update-downloads",
        ] {
            assert!(ids.contains(id), "missing release cleanup rule {id}");
        }

        let recycle_bin = catalog
            .rules()
            .find(|rule| rule.id == "windows-recycle-bin")
            .expect("Recycle Bin rule should exist");
        assert_eq!(recycle_bin.impact, CleanupImpact::High);
        assert_eq!(recycle_bin.recovery, CleanupRecovery::Permanent);
    }

    #[test]
    fn unsafe_relative_patterns_are_rejected() {
        assert!(validate_relative_pattern("../outside").is_err());
        assert!(validate_relative_pattern("C:\\Windows").is_err());
        assert!(validate_relative_pattern("\\\\server\\share").is_err());
        assert!(validate_relative_pattern("safe/cache").is_ok());
        assert!(validate_relative_pattern("broken/[pattern").is_err());
    }
}
