/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_BUNDLE");

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "macos"
        && std::env::var_os("CARGO_BUNDLE").is_none()
    {
        build_utils::macos::embed_info_plist("meta/macos/Info.plist");
    }

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        build_utils::windows::compile_application(
            "Sequel Explorer",
            "Sequel Explorer",
            "Copyright © 2026 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[("meta/windows/file-types/db.ico", "2")],
        )
        .expect("failed to compile Windows resources");
    }
}
