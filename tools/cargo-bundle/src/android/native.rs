/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::Path;

use super::toolchain::Toolchain;
use crate::support::cargo;

/// Android ABIs with their Rust target triple and NDK clang target prefix.
pub(super) const ABIS: [(&str, &str, &str); 3] = [
    (
        "armeabi-v7a",
        "armv7-linux-androideabi",
        "armv7a-linux-androideabi",
    ),
    (
        "arm64-v8a",
        "aarch64-linux-android",
        "aarch64-linux-android",
    ),
    ("x86_64", "x86_64-linux-android", "x86_64-linux-android"),
];

// Only the entry point NativeActivity calls is exported.
const EXPORTS: &str = "{ global: ANativeActivity_onCreate; local: *; };\n";

/// Build the binary as a shared library for every ABI into `<target_dir>/lib/<abi>/lib<name>.so`.
pub(super) fn build(path: &str, target_dir: &str, name: &str, toolchain: &Toolchain, min_sdk: u32) {
    let exports = Path::new(target_dir).join("exports.map");
    fs::write(&exports, EXPORTS).expect("Failed to write exports.map");
    let exports = fs::canonicalize(exports).expect("Failed to resolve exports.map");
    let ar = toolchain.ndk_bin.join("llvm-ar");
    for (abi, triple, clang_target) in ABIS {
        let clang = toolchain.clang(clang_target, min_sdk);
        let variable = triple.replace('-', "_");
        let linker = format!("CARGO_TARGET_{}_LINKER", variable.to_uppercase());
        // rustc links the binary as a position independent executable, -shared
        // turns it into a library that still exports the generated C main function.
        let status = cargo("rustc", path)
            .args([
                "--bin",
                name,
                "--target",
                triple,
                "--",
                "-C",
                "link-arg=-shared",
                "-C",
                "link-arg=-Wno-unused-command-line-argument",
                // Align segments for devices with 16 KB pages, older NDKs default to 4 KB.
                "-C",
                "link-arg=-Wl,-z,max-page-size=16384",
                "-C",
                &format!("link-arg=-Wl,--version-script={}", exports.display()),
            ])
            .env(linker, &clang)
            .env(format!("CC_{variable}"), &clang)
            .env(format!("AR_{variable}"), &ar)
            .status()
            .expect("Failed to run cargo rustc");
        assert!(status.success(), "cargo rustc failed for {triple}");

        let lib_dir = format!("{target_dir}/lib/{abi}");
        fs::create_dir_all(&lib_dir).expect("Failed to create native library directory");
        fs::copy(
            format!("target/{triple}/release/{name}"),
            format!("{lib_dir}/lib{name}.so"),
        )
        .expect("Failed to copy native library");
    }
}
