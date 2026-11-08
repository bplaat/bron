/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;

/// Embeds a custom Info.plist in unbundled macOS binaries.
///
/// The path is relative to the consuming package's manifest directory.
/// Call this only for macOS targets when `CARGO_BUNDLE` is unset.
pub fn embed_info_plist(path: impl AsRef<Path>) {
    let path = path.as_ref();
    println!("cargo:rerun-if-changed={}", path.display());
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let path = Path::new(&manifest).join(path);
    println!(
        "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        path.display()
    );
}
