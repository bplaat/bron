/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;

use build_utils::{link_libraries, search_dirs};

fn main() {
    let target = env::var("CARGO_CFG_TARGET_OS").expect("target OS");
    let vendor = env::var("CARGO_CFG_TARGET_VENDOR").expect("target vendor");
    if vendor != "apple" && !matches!(target.as_str(), "windows" | "android") {
        link_libraries(&search_dirs(), &["cairo"]);
    }
}
