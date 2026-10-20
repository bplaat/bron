/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::Path;
use std::process::{Command, exit};

use crate::manifest::BundleMetadata;
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
    ] {
        info.insert(key.into(), value.into());
    }
    info
}

/// Merge the shared and then the platform specific custom Info.plist files into the info
pub(crate) fn merge_custom_info(
    info: &mut plist::Dictionary,
    path: &str,
    bundle: &BundleMetadata,
    platform_info_plist: Option<&str>,
) {
    let shared = bundle.info_plist.as_deref().unwrap_or("Info.plist");
    for name in std::iter::once(shared).chain(platform_info_plist) {
        let custom_info = format!("{path}/{name}");
        if !Path::new(&custom_info).exists() {
            continue;
        }
        let plist::Value::Dictionary(custom) = plist::Value::from_file(&custom_info)
            .unwrap_or_else(|error| {
                eprintln!("Invalid Info.plist file {custom_info}: {error}");
                exit(1);
            })
        else {
            eprintln!("Invalid Info.plist file {custom_info}: root value must be a dictionary");
            exit(1);
        };
        info.extend(custom);
    }
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

/// Compile an Icon Composer icon into Assets.car and return actool's partial Info.plist
pub(crate) fn compile_icon(
    icon: &str,
    output_dir: &str,
    platform: &str,
    target_devices: &[&str],
    minimum_os: &str,
) -> plist::Dictionary {
    let icon = fs::canonicalize(icon).expect("Failed to resolve icon path");
    let output_dir = fs::canonicalize(output_dir).expect("Failed to resolve output directory");
    let partial_plist = output_dir.join("partial.plist");
    let mut command = Command::new("actool");
    command
        .arg(&icon)
        .arg("--compile")
        .arg(&output_dir)
        .args(["--platform", platform])
        .args(["--minimum-deployment-target", minimum_os]);
    for device in target_devices {
        command.args(["--target-device", device]);
    }
    let output = command
        .args(["--app-icon", "icon", "--include-all-app-icons"])
        .arg("--output-partial-info-plist")
        .arg(&partial_plist)
        .output()
        .expect("Failed to run actool");
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("actool failed:\n{stdout}{stderr}");
        exit(1);
    }
    let partial = plist::Value::from_file(&partial_plist).expect("Failed to read partial.plist");
    fs::remove_file(&partial_plist).expect("Failed to remove partial.plist");
    partial
        .into_dictionary()
        .expect("partial.plist root value must be a dictionary")
}
