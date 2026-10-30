/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Deserialize;

use super::system::trusted_executable;
use super::{CleanupResult, cancellable_output, command};
use crate::catalog::PathRoot::UserProfile;

fn executable() -> Option<PathBuf> {
    trusted_executable(UserProfile.resolve()?.join(".cargo/bin/rustup.exe"))
}

#[derive(Deserialize)]
struct ToolchainFile {
    toolchain: Toolchain,
}

#[derive(Deserialize)]
struct Toolchain {
    channel: Option<String>,
}

pub(super) fn toolchain_channel(contents: &str) -> Option<String> {
    if let Ok(file) = basic_toml::from_str::<ToolchainFile>(contents) {
        return file.toolchain.channel;
    }
    // Legacy rust-toolchain files only contain the channel name
    let channel = contents.trim();
    (!channel.is_empty() && !channel.contains(['\n', '[', '='])).then(|| channel.to_string())
}

pub(super) fn protect_channel(
    channel: &str,
    installed: &[String],
    protected: &mut HashSet<String>,
) {
    for name in installed {
        if name == channel
            || name
                .strip_prefix(channel)
                .is_some_and(|suffix| suffix.starts_with('-'))
        {
            protected.insert(name.clone());
        }
    }
}

fn protect_project_toolchains(
    root: &Path,
    installed: &[String],
    protected: &mut HashSet<String>,
    cancelled: Option<&AtomicBool>,
) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        if cancelled.is_some_and(|flag| flag.load(Ordering::Relaxed)) {
            return;
        }
        for filename in ["rust-toolchain.toml", "rust-toolchain"] {
            if let Ok(contents) = fs::read_to_string(directory.join(filename))
                && let Some(channel) = toolchain_channel(&contents)
            {
                protect_channel(&channel, installed, protected);
            }
        }
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if entry.file_type().is_ok_and(|kind| kind.is_dir())
                && !matches!(
                    name.to_string_lossy().as_ref(),
                    ".git" | ".hg" | ".svn" | "node_modules" | "target" | "vendor"
                )
            {
                pending.push(entry.path());
            }
        }
    }
}

fn project_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(profile) = UserProfile.resolve() {
        for directory in ["Projects", "Repos", "Source/repos", "src", "dev"] {
            let root = profile.join(directory);
            if root.is_dir() && !roots.contains(&root) {
                roots.push(root);
            }
        }
    }
    roots
}

struct InstalledToolchains {
    names: Vec<String>,
    protected: HashSet<String>,
}

fn installed_toolchains(cancelled: Option<&AtomicBool>) -> Option<InstalledToolchains> {
    let mut command = command(executable()?);
    command.args(["toolchain", "list"]);
    let output = match cancelled {
        Some(cancelled) => cancellable_output(&mut command, cancelled).ok()??,
        None => command.output().ok()?,
    };
    if !output.status.success() {
        return None;
    }
    let mut names = Vec::new();
    let mut protected = HashSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        names.push(name.to_string());
        if line.contains("active") || line.contains("default") {
            protected.insert(name.to_string());
        }
    }
    if let Ok(current) = std::env::current_dir() {
        for directory in current.ancestors() {
            for filename in ["rust-toolchain.toml", "rust-toolchain"] {
                if let Ok(contents) = fs::read_to_string(directory.join(filename))
                    && let Some(channel) = toolchain_channel(&contents)
                {
                    protect_channel(&channel, &names, &mut protected);
                }
            }
        }
    }
    for root in project_roots() {
        protect_project_toolchains(&root, &names, &mut protected, cancelled);
    }
    Some(InstalledToolchains { names, protected })
}

pub(super) fn toolchains_to_remove_except(
    toolchains: &[String],
    protected: &HashSet<String>,
) -> Vec<String> {
    fn stable_rank(name: &str) -> Option<(u8, Vec<u32>)> {
        if name == "stable" || name.starts_with("stable-") {
            return Some((1, Vec::new()));
        }
        let version = name.split('-').next()?;
        let parts: Option<Vec<_>> = version.split('.').map(|part| part.parse().ok()).collect();
        parts
            .filter(|parts| parts.len() >= 2)
            .map(|parts| (0, parts))
    }

    fn nightly_rank(name: &str) -> Option<(u8, String)> {
        if name == "nightly"
            || name.starts_with("nightly-") && !name[8..].starts_with(|c: char| c.is_ascii_digit())
        {
            return Some((1, String::new()));
        }
        name.strip_prefix("nightly-")
            .filter(|suffix| {
                suffix.len() >= 10 && suffix.as_bytes()[4] == b'-' && suffix.as_bytes()[7] == b'-'
            })
            .map(|suffix| (0, suffix[..10].to_string()))
    }

    let keep_stable = toolchains
        .iter()
        .filter_map(|name| stable_rank(name).map(|rank| (rank, name)))
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, name)| name);
    let keep_nightly = toolchains
        .iter()
        .filter_map(|name| nightly_rank(name).map(|rank| (rank, name)))
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
        .map(|(_, name)| name);

    toolchains
        .iter()
        .filter(|name| {
            Some(*name) != keep_stable
                && Some(*name) != keep_nightly
                && !protected.contains(name.as_str())
        })
        .cloned()
        .collect()
}

pub(super) fn scan(cancelled: &AtomicBool, result: &mut CleanupResult) {
    let Some(toolchains) = installed_toolchains(Some(cancelled)) else {
        return;
    };
    let old = toolchains_to_remove_except(&toolchains.names, &toolchains.protected);
    result.available = !old.is_empty();
    result.files = old.len() as u64;
    result.unknown_size = !old.is_empty();
}

pub(super) fn clean(result: &mut CleanupResult) {
    let Some(executable) = executable() else {
        result.errors.push("Rustup is unavailable".to_string());
        return;
    };
    let Some(toolchains) = installed_toolchains(None) else {
        result
            .errors
            .push("Could not list installed Rust toolchains".to_string());
        return;
    };
    let old = toolchains_to_remove_except(&toolchains.names, &toolchains.protected);
    for toolchain in &old {
        match command(&executable)
            .args(["toolchain", "uninstall", toolchain])
            .output()
        {
            Ok(output) if output.status.success() => {}
            Ok(output) => result.errors.push(format!(
                "Could not uninstall {toolchain}: rustup exited with {}",
                output.status
            )),
            Err(error) => result
                .errors
                .push(format!("Could not uninstall {toolchain}: {error}")),
        }
    }
    result.cleaned_files = old.len().saturating_sub(result.errors.len()) as u64;
}
