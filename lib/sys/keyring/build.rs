/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

use std::env;
use std::path::PathBuf;

use build_utils::{display_paths, find_library, library_has_symbol, link_library, push_unique};

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

    let secret = require_library(&search_dirs, "secret-1");
    if !library_has_symbol(&secret, b"secret_password_store_binary_sync") {
        panic!("keyring requires libsecret-1 0.19 or newer");
    }
    link_library(&secret);
    link_library(&require_library(&search_dirs, "glib-2.0"));
}

fn require_library(search_dirs: &[PathBuf], name: &str) -> PathBuf {
    find_library(search_dirs, name).unwrap_or_else(|| {
        panic!(
            "could not find the {name} runtime library; searched: {}. Set KEYRING_LIB_DIR to its library directory",
            display_paths(search_dirs)
        )
    })
}
