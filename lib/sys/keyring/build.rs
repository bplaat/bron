/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;
use std::path::PathBuf;

use build_utils::{display_paths, find_library, link_library, push_unique, require_symbol};

fn main() {
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set");
    if target_os == "macos" || target_os == "windows" {
        return;
    }

    println!("cargo::rerun-if-env-changed=KEYRING_LIB_DIR");
    let mut search_dirs = Vec::new();
    if let Some(path) = env::var_os("KEYRING_LIB_DIR") {
        search_dirs.push(PathBuf::from(path));
    }
    for path in build_utils::search_dirs() {
        push_unique(&mut search_dirs, path);
    }

    for name in ["secret-1", "glib-2.0"] {
        let library = find_library(&search_dirs, name).unwrap_or_else(|| {
            panic!(
                "could not find the {name} runtime library; searched: {}. Set KEYRING_LIB_DIR to its library directory",
                display_paths(&search_dirs)
            )
        });
        if name == "secret-1" {
            require_symbol(
                &library,
                b"secret_password_store_binary_sync",
                "keyring requires libsecret-1 0.19 or newer",
            );
        }
        link_library(&library);
    }
}
