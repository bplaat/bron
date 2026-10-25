/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path};
use std::process::Command;

#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
pub(crate) fn cargo_build(path: &str) -> Command {
    cargo("build", path)
}

pub(crate) fn cargo(subcommand: &str, path: &str) -> Command {
    let mut command = Command::new("cargo");
    command.args([
        subcommand,
        "--release",
        "--manifest-path",
        &format!("{path}/Cargo.toml"),
    ]);
    // Lets build scripts skip work the bundle already does, like embedding Info.plist
    crate::assets::configure(&mut command);
    command
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
