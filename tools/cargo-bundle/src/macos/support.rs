/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::process::Command;

use crate::support::cargo_build;

pub(crate) fn codesign(bundle: &str, entitlements: Option<&str>, runtime: bool) -> Command {
    let mut command = Command::new("codesign");
    command.args(["--force", "--sign", "-"]);
    if runtime {
        command.args(["--options", "runtime"]);
    }
    if let Some(entitlements) = entitlements {
        command.args(["--entitlements", entitlements]);
    }
    command.arg(bundle);
    command
}

pub(crate) fn bundle_info(
    name: &str,
    identifier: &str,
    version: &str,
    package_type: &str,
    minimum_os: &str,
) -> plist::Dictionary {
    let mut info = plist::Dictionary::new();
    for (key, value) in [
        ("CFBundleInfoDictionaryVersion", "6.0"),
        ("CFBundlePackageType", package_type),
        ("CFBundleName", name),
        ("CFBundleDisplayName", name),
        ("CFBundleIdentifier", identifier),
        ("CFBundleVersion", version),
        ("CFBundleShortVersionString", version),
        ("CFBundleExecutable", name),
        ("LSMinimumSystemVersion", minimum_os),
    ] {
        info.insert(key.into(), value.into());
    }
    info
}

pub(crate) fn compile_universal(path: &str, binary: &str, output: &str) {
    for target in ["x86_64-apple-darwin", "aarch64-apple-darwin"] {
        let status = cargo_build(path)
            .args(["--target", target])
            .status()
            .expect("Failed to run cargo build");
        assert!(status.success(), "cargo build failed for {target}");
    }
    let status = Command::new("lipo")
        .args([
            "-create",
            &format!("target/x86_64-apple-darwin/release/{binary}"),
            &format!("target/aarch64-apple-darwin/release/{binary}"),
            "-output",
            output,
        ])
        .status()
        .expect("Failed to run lipo");
    assert!(status.success(), "lipo failed");
}

pub(crate) fn remove_empty_directory(path: &str) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return;
    }
    let is_empty = fs::read_dir(path)
        .expect("Failed to read directory")
        .next()
        .is_none();
    if is_empty {
        fs::remove_dir(path).expect("Failed to remove empty directory");
    }
}
