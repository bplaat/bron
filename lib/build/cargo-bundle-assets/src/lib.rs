/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::borrow::Cow;
use std::io;

pub use cargo_bundle_assets_impl::Assets;
pub use rust_embed::{EmbeddedFile, RustEmbed};

/// Implementation details used by the derive macro.
#[doc(hidden)]
pub mod __private {
    pub use std::borrow::Cow;
    pub use std::io;

    pub use super::read;
}

/// Read a declared asset from the package's resource collection.
#[doc(hidden)]
pub fn read(collection: &str, name: &str, metadata: &[u8]) -> io::Result<EmbeddedFile> {
    // Keep the inline metadata reachable through release LTO and linker dead stripping.
    std::hint::black_box(metadata);
    let relative = format!("{collection}/{name}");
    #[cfg(target_os = "android")]
    let data = bwindow::ffi::android_read_asset(&relative)?;
    #[cfg(not(target_os = "android"))]
    let data = std::fs::read(resource_root()?.join(relative))?;
    Ok(EmbeddedFile {
        data: Cow::Owned(data),
    })
}

#[cfg(not(target_os = "android"))]
fn resource_root() -> io::Result<std::path::PathBuf> {
    let executable = std::env::current_exe()?;
    let directory = executable
        .parent()
        .ok_or_else(|| io::Error::other("Executable has no parent directory"))?;
    #[cfg(target_os = "macos")]
    if directory.file_name().is_some_and(|name| name == "MacOS")
        && let Some(contents) = directory.parent()
        && contents.file_name().is_some_and(|name| name == "Contents")
    {
        return Ok(contents.join("Resources"));
    }
    #[cfg(target_os = "ios")]
    return Ok(directory.join("assets"));
    #[cfg(windows)]
    return Ok(directory.join("Assets"));
    #[cfg(not(any(target_os = "ios", windows)))]
    Ok(directory.to_path_buf())
}
