/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [rust-embed](https://crates.io/crates/rust-embed) crate

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::borrow::Cow;

pub use rust_embed_impl::*;

/// A trait that allows you to access embedded files
pub trait RustEmbed {
    /// Get an embedded file by its path.
    fn get(file_path: &str) -> Option<EmbeddedFile>;

    /// Iterate over the relative paths of all embedded files.
    fn iter() -> impl Iterator<Item = Cow<'static, str>> + 'static;
}

pub use RustEmbed as Embed;

/// A file embedded in the binary
pub struct EmbeddedFile {
    /// The content of the file
    pub data: Cow<'static, [u8]>,
    /// The metadata of the file
    pub metadata: Metadata,
}

/// Metadata about an embedded file
pub struct Metadata {
    _private: (),
}

impl Metadata {
    #[doc(hidden)]
    pub const fn __rust_embed_new(
        _hash: [u8; 32],
        _last_modified: Option<u64>,
        _created: Option<u64>,
    ) -> Self {
        Self { _private: () }
    }
}
