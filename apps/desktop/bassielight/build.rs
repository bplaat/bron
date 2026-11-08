/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

fn main() {
    build_utils::npm_build(
        "../../..",
        "web",
        if cfg!(debug_assertions) {
            "build-debug"
        } else {
            "build-release"
        },
    );

    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        build_utils::windows::compile_application(
            "BassieLight",
            "BassieLight",
            "Copyright © 2025 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[],
        )
        .expect("failed to compile Windows resources");
    }
}
