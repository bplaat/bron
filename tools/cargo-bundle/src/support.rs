/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};
use std::process::Command;

pub(crate) fn cargo_build(path: &str) -> Command {
    let mut command = Command::new("cargo");
    command.args([
        "build",
        "--release",
        "--manifest-path",
        &format!("{path}/Cargo.toml"),
    ]);
    // Lets build scripts skip work the bundle already does, like embedding Info.plist
    command.env("CARGO_BUNDLE", "1");
    command
}

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

pub(crate) fn assert_file_name(value: &str, description: &str) {
    let mut components = Path::new(value).components();
    assert!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "{description} must be a file name"
    );
}

pub(crate) fn remove_path_if_exists(path: &str) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return,
        Err(error) => panic!("Failed to inspect {path}: {error}"),
    };
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path).unwrap_or_else(|error| panic!("Failed to remove {path}: {error}"));
    } else {
        fs::remove_file(path).unwrap_or_else(|error| panic!("Failed to remove {path}: {error}"));
    }
}
