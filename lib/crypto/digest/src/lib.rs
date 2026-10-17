/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [digest](https://crates.io/crates/digest) crate

#![cfg_attr(not(test), no_std)]

// MARK: Digest
/// A trait for cryptographic hash functions.
pub trait Digest: Default {
    /// The byte block size used by this hash (e.g. 64 for SHA-1 and SHA-256).
    const BLOCK_SIZE: usize;
    /// The output type produced by this hash (e.g. `[u8; 20]` or `[u8; 32]`).
    type Output: AsRef<[u8]> + Copy;

    /// Feed bytes into the hasher.
    fn update(&mut self, data: &[u8]);

    /// Finalize, return the digest, and reset the hasher to its initial state.
    fn finalize_reset(&mut self) -> Self::Output;

    /// Finalize and return the digest, consuming the hasher.
    fn finalize(mut self) -> Self::Output
    where
        Self: Sized,
    {
        self.finalize_reset()
    }

    /// One-shot: hash `data` and return the digest.
    fn digest(data: &[u8]) -> Self::Output
    where
        Self: Sized,
    {
        let mut h = Self::default();
        h.update(data);
        h.finalize()
    }
}
