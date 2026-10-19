/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

fn main() {
    // Embed Info.plist so unbundled runs (e.g. cargo run) still know the document types
    println!("cargo:rerun-if-env-changed=CARGO_BUNDLE");
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "macos"
        && std::env::var_os("CARGO_BUNDLE").is_none()
    {
        println!("cargo:rerun-if-changed=Info.plist");
        println!(
            "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}/Info.plist",
            env!("CARGO_MANIFEST_DIR")
        );
    }
}
