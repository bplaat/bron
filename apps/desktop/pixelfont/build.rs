/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");

    // Embed Info.plist so unbundled runs (e.g. cargo run) still know the document types
    println!("cargo:rerun-if-env-changed=CARGO_BUNDLE");
    if target_os == "macos" && std::env::var_os("CARGO_BUNDLE").is_none() {
        println!("cargo:rerun-if-changed=meta/macos/Info.plist");
        println!(
            "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}/meta/macos/Info.plist",
            env!("CARGO_MANIFEST_DIR")
        );
    }

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        build_utils::windows::compile_application(
            "Pixel Font Editor",
            "Pixel Font Editor",
            "Copyright © 2026 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[("meta/windows/file-types/pf.ico", "2")],
        )
        .expect("failed to compile Windows resources");
    }
}
