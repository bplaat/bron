/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [hmac](https://crates.io/crates/hmac) crate

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

use digest::Digest;

fn key_block<D: Digest>(key: &[u8]) -> Vec<u8> {
    let mut block = vec![0; D::BLOCK_SIZE];
    if key.len() > D::BLOCK_SIZE {
        let hashed = D::digest(key);
        block[..hashed.as_ref().len()].copy_from_slice(hashed.as_ref());
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    block
}

// MARK: hmac
/// Computes HMAC over `message` with `key` and returns the raw `D::Output` bytes.
pub fn hmac<D: Digest>(key: &[u8], message: &[u8]) -> D::Output {
    let mut pad = key_block::<D>(key);
    for byte in &mut pad {
        *byte ^= 0x36;
    }
    let mut h = D::default();
    h.update(&pad);
    h.update(message);
    let inner = h.finalize_reset();

    for byte in &mut pad {
        *byte ^= 0x36 ^ 0x5c;
    }
    h.update(&pad);
    h.update(inner.as_ref());
    h.finalize_reset()
}

/// An HMAC key prepared for signing multiple messages.
pub struct Hmac<D: Digest + Clone> {
    inner: D,
    outer: D,
}

impl<D: Digest + Clone> Hmac<D> {
    /// Prepare a key for repeated HMAC computations.
    pub fn new(key: &[u8]) -> Self {
        let mut pad = key_block::<D>(key);
        for byte in &mut pad {
            *byte ^= 0x36;
        }
        let mut inner = D::default();
        inner.update(&pad);

        for byte in &mut pad {
            *byte ^= 0x36 ^ 0x5c;
        }
        let mut outer = D::default();
        outer.update(&pad);
        Self { inner, outer }
    }

    /// Compute an HMAC without changing the prepared key.
    pub fn sign(&self, message: &[u8]) -> D::Output {
        let mut inner = self.inner.clone();
        inner.update(message);
        let digest = inner.finalize();

        let mut outer = self.outer.clone();
        outer.update(digest.as_ref());
        outer.finalize()
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use sha2::Sha256;

    use super::*;

    #[test]
    fn test_hmac_sha256_rfc4231_tc1() {
        // RFC 4231 Test Case 1
        let expected = [
            0xb0, 0x34, 0x4c, 0x61, 0xd8, 0xdb, 0x38, 0x53, 0x5c, 0xa8, 0xaf, 0xce, 0xaf, 0x0b,
            0xf1, 0x2b, 0x88, 0x1d, 0xc2, 0x00, 0xc9, 0x83, 0x3d, 0xa7, 0x26, 0xe9, 0x37, 0x6c,
            0x2e, 0x32, 0xcf, 0xf7,
        ];
        assert_eq!(
            hmac::<Sha256>(
                b"\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b\x0b",
                b"Hi There"
            ),
            expected
        );
    }

    #[test]
    fn reusable_key_matches_one_shot_hmac() {
        for key in [&b"short"[..], &[0x5a; 100][..]] {
            let mac = Hmac::<Sha256>::new(key);
            for message in [&b""[..], &b"first"[..], &b"second message"[..]] {
                assert_eq!(mac.sign(message), hmac::<Sha256>(key, message));
            }
        }
    }

    #[test]
    fn long_key_matches_rfc4231() {
        let key = [0xaa; 131];
        let message = b"Test Using Larger Than Block-Size Key - Hash Key First";
        let expected = [
            0x60, 0xe4, 0x31, 0x59, 0x1e, 0xe0, 0xb6, 0x7f, 0x0d, 0x8a, 0x26, 0xaa, 0xcb, 0xf5,
            0xb7, 0x7f, 0x8e, 0x0b, 0xc6, 0x21, 0x37, 0x28, 0xc5, 0x14, 0x05, 0x46, 0x04, 0x0f,
            0x0e, 0xe3, 0x7f, 0x54,
        ];
        assert_eq!(hmac::<Sha256>(&key, message), expected);
        assert_eq!(Hmac::<Sha256>::new(&key).sign(message), expected);
    }
}
