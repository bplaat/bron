/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(not(any(test, feature = "std")), no_std)]

extern crate alloc;

use alloc::vec::Vec;

use hmac::Hmac;
use sha2::Sha256;

#[cfg(feature = "std")]
pub use crate::utils::password_hash;
pub use crate::utils::{
    DEFAULT_SAFE_ITERATIONS, MAX_PASSWORD_HASH_ITERATIONS, PasswordHashDecodeError,
    password_hash_customized, password_verify,
};

mod utils;

/// PBKDF2-HMAC-SHA256 key derivation function
pub fn pbkdf2_hmac_sha256(password: &[u8], salt: &[u8], iterations: u32, dklen: usize) -> Vec<u8> {
    assert!(
        iterations > 0,
        "PBKDF2 iterations must be greater than zero"
    );
    assert!(
        dklen.div_ceil(32) <= u32::MAX as usize,
        "PBKDF2 derived key is too large"
    );

    let mac = Hmac::<Sha256>::new(password);
    let mut input = Vec::with_capacity(salt.len() + 4);
    input.extend_from_slice(salt);
    input.extend_from_slice(&[0; 4]);
    let mut derived_key = Vec::with_capacity(dklen);
    for i in 1..=dklen.div_ceil(32) {
        input[salt.len()..].copy_from_slice(&(i as u32).to_be_bytes());
        let mut u = mac.sign(&input);
        let mut block = u;
        for _ in 1..iterations {
            u = mac.sign(&u);
            for (byte, next) in block.iter_mut().zip(u) {
                *byte ^= next;
            }
        }
        derived_key.extend_from_slice(&block);
    }
    derived_key.truncate(dklen);
    derived_key
}

#[cfg(test)]
mod tests {
    use super::pbkdf2_hmac_sha256;

    #[test]
    fn known_sha256_vectors() {
        for (iterations, length, expected) in [
            (
                1,
                32,
                "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b",
            ),
            (
                2,
                64,
                "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43\
                 830651afcb5c862f0b249bd031f7a67520d136470f5ec271ece91c07773253d9",
            ),
        ] {
            let actual = pbkdf2_hmac_sha256(b"password", b"salt", iterations, length);
            let actual = actual
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn long_password_and_multiple_blocks() {
        let actual = pbkdf2_hmac_sha256(&[b'P'; 100], &[b'S'; 40], 3, 65);
        let actual = actual
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            actual,
            "59a7bfa5e66f9b96abca53f6606357f1b8ab0c076a293f6d9984e5f695716e4\
             727ba252dca1883182d8d15d7149c5a175ca72804c7c78ca5c9e9a7f8e5c40a6c99"
        );
    }
}
