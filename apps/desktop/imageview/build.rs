/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        build_utils::windows::compile_application(
            "ImageView",
            "ImageView",
            "Copyright © 2026 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[
                ("meta/windows/file-types/apng.ico", "2"),
                ("meta/windows/file-types/bmp.ico", "3"),
                ("meta/windows/file-types/gif.ico", "4"),
                ("meta/windows/file-types/icns.ico", "5"),
                ("meta/windows/file-types/ico.ico", "6"),
                ("meta/windows/file-types/jpeg2000.ico", "7"),
                ("meta/windows/file-types/jpeg.ico", "8"),
                ("meta/windows/file-types/netpbm.ico", "9"),
                ("meta/windows/file-types/png.ico", "10"),
                ("meta/windows/file-types/qoi.ico", "11"),
                ("meta/windows/file-types/svg.ico", "12"),
                ("meta/windows/file-types/tga.ico", "13"),
                ("meta/windows/file-types/tiff.ico", "14"),
                ("meta/windows/file-types/tinyvg.ico", "15"),
                ("meta/windows/file-types/webp.ico", "16"),
            ],
        )
        .expect("failed to compile Windows resources");
    }

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
