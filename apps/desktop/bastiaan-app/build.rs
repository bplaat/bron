/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Windows resources: the icon, version information and manifest

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        build_utils::windows::compile_application(
            "Bastiaan",
            "Bastiaan van der Plaat",
            "Copyright (c) 2026 Bastiaan van der Plaat",
            "meta/windows/icon.ico",
            &[],
        )
        .expect("failed to compile Windows resources");
    }
}
