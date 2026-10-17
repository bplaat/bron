/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::{Path, PathBuf};
use std::process::Command;

/// Builds the Wii libraries, configures the Rust linker and returns the build directory.
pub fn build(toolchain: impl AsRef<Path>) -> PathBuf {
    let toolchain = toolchain
        .as_ref()
        .canonicalize()
        .expect("Wii toolchain not found");
    let status = Command::new(toolchain.join("build.sh"))
        .status()
        .expect("failed to run build.sh");
    assert!(status.success(), "failed to build the toolchain");
    for path in [
        "build.rs",
        "build.sh",
        "gxtexconv",
        "libc",
        "libogc",
        "wii.ld",
        "wii.json",
    ] {
        println!("cargo:rerun-if-changed={}", toolchain.join(path).display());
    }
    println!(
        "cargo:rustc-link-arg=-T{}",
        toolchain.join("wii.ld").display()
    );
    let build_dir = toolchain.join("../../target/wii-toolchain");
    println!("cargo:rustc-link-search=native={}", build_dir.display());
    println!("cargo:rustc-link-lib=static=ogc");
    println!("cargo:rustc-link-lib=static=c");
    build_dir
}
