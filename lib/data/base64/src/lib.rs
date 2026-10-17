/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [base64](https://crates.io/crates/base64) crate

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// MARK: Engine
/// Trait for base64 encoding and decoding engines.
pub trait Engine {
    /// Encode bytes to a base64 `String`.
    fn encode<T: AsRef<[u8]>>(&self, input: T) -> String;
    /// Decode a base64 byte string to `Vec<u8>`.
    fn decode<T: AsRef<[u8]>>(&self, input: T) -> Result<Vec<u8>, DecodeError>;
}

/// Error returned when decoding invalid base64 input.
#[derive(Debug)]
pub struct DecodeError;

impl core::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid base64")
    }
}

impl core::error::Error for DecodeError {}

// MARK: GeneralPurpose
/// A general-purpose base64 engine with configurable alphabet and padding.
pub struct GeneralPurpose {
    encode_table: &'static [u8; 64],
    decode_table: &'static [u8; 256],
    padding: bool,
}

impl Engine for GeneralPurpose {
    #[allow(unsafe_code)]
    fn encode<T: AsRef<[u8]>>(&self, input: T) -> String {
        let input = input.as_ref();
        let mut out = Vec::with_capacity(input.len().div_ceil(3) * 4);
        let (chunks, tail) = input.as_chunks::<3>();
        for &[b0, b1, b2] in chunks {
            out.extend_from_slice(&[
                self.encode_table[(b0 >> 2) as usize],
                self.encode_table[((b0 & 0x03) << 4 | b1 >> 4) as usize],
                self.encode_table[((b1 & 0x0f) << 2 | b2 >> 6) as usize],
                self.encode_table[(b2 & 0x3f) as usize],
            ]);
        }
        if let Some(&b0) = tail.first() {
            out.push(self.encode_table[(b0 >> 2) as usize]);
            let b1 = tail.get(1).copied().unwrap_or(0);
            out.push(self.encode_table[((b0 & 0x03) << 4 | b1 >> 4) as usize]);
            if tail.len() == 2 {
                out.push(self.encode_table[((b1 & 0x0f) << 2) as usize]);
            } else if self.padding {
                out.push(b'=');
            }
            if self.padding {
                out.push(b'=');
            }
        }
        // SAFETY: The encoder only pushes bytes from the base64 alphabet (A-Z, a-z, 0-9, +, /, =), all of which are valid UTF-8.
        unsafe { String::from_utf8_unchecked(out) }
    }

    fn decode<T: AsRef<[u8]>>(&self, input: T) -> Result<Vec<u8>, DecodeError> {
        let input = input.as_ref();
        let mut out = Vec::with_capacity((input.len() * 3) / 4);
        let (chunks, tail) = input.as_chunks::<4>();
        for (index, &[a, b, c, d]) in chunks.iter().enumerate() {
            let a = self.decode_table[a as usize];
            let b = self.decode_table[b as usize];
            let c = self.decode_table[c as usize];
            let d = self.decode_table[d as usize];
            if (a | b | c | d) == 0xff {
                return self.decode_tail(&input[index * 4..], out);
            }
            out.extend_from_slice(&[(a << 2) | (b >> 4), (b << 4) | (c >> 2), (c << 6) | d]);
        }
        self.decode_tail(tail, out)
    }
}

impl GeneralPurpose {
    fn decode_tail(&self, input: &[u8], mut out: Vec<u8>) -> Result<Vec<u8>, DecodeError> {
        let mut buf = 0u32;
        let mut bits = 0u32;
        for &byte in input {
            if byte == b'=' {
                break;
            }
            let value = self.decode_table[byte as usize];
            if value == 0xff {
                return Err(DecodeError);
            }
            buf = (buf << 6) | u32::from(value);
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buf >> bits) as u8);
                buf &= (1 << bits) - 1;
            }
        }
        Ok(out)
    }
}

// MARK: Alphabet tables
const fn build_decode_table(encode_table: &[u8; 64]) -> [u8; 256] {
    let mut table = [0xFFu8; 256];
    let mut i = 0usize;
    while i < 64 {
        table[encode_table[i] as usize] = i as u8;
        i += 1;
    }
    table
}

static STANDARD_ENCODE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
static STANDARD_DECODE: [u8; 256] = build_decode_table(STANDARD_ENCODE);

static URL_SAFE_ENCODE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
static URL_SAFE_DECODE: [u8; 256] = build_decode_table(URL_SAFE_ENCODE);

// MARK: Engines
/// Standard base64 alphabet (`+` and `/`) with `=` padding.
pub static BASE64_STANDARD: GeneralPurpose = GeneralPurpose {
    encode_table: STANDARD_ENCODE,
    decode_table: &STANDARD_DECODE,
    padding: true,
};

/// Standard base64 alphabet (`+` and `/`) without padding.
pub static BASE64_STANDARD_NO_PAD: GeneralPurpose = GeneralPurpose {
    encode_table: STANDARD_ENCODE,
    decode_table: &STANDARD_DECODE,
    padding: false,
};

/// URL-safe base64 alphabet (`-` and `_`) without padding.
pub static BASE64_URL_SAFE_NO_PAD: GeneralPurpose = GeneralPurpose {
    encode_table: URL_SAFE_ENCODE,
    decode_table: &URL_SAFE_DECODE,
    padding: false,
};

/// URL-safe base64 alphabet (`-` and `_`) with `=` padding.
pub static BASE64_URL_SAFE: GeneralPurpose = GeneralPurpose {
    encode_table: URL_SAFE_ENCODE,
    decode_table: &URL_SAFE_DECODE,
    padding: true,
};

// MARK: Modules
/// Common engine constants (mirrors `base64::engine::general_purpose`).
pub mod engine {
    /// General-purpose base64 engines.
    pub mod general_purpose {
        pub use crate::{
            BASE64_STANDARD as STANDARD, BASE64_STANDARD_NO_PAD as STANDARD_NO_PAD,
            BASE64_URL_SAFE as URL_SAFE, BASE64_URL_SAFE_NO_PAD,
        };
    }
}

/// Prelude - import this to get the most common engines and the `Engine` trait.
pub mod prelude {
    pub use crate::{
        BASE64_STANDARD, BASE64_STANDARD_NO_PAD, BASE64_URL_SAFE, BASE64_URL_SAFE_NO_PAD, Engine,
    };
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_encode_standard() {
        assert_eq!(BASE64_STANDARD.encode(b""), "");
        assert_eq!(BASE64_STANDARD.encode(b"f"), "Zg==");
        assert_eq!(BASE64_STANDARD.encode(b"fo"), "Zm8=");
        assert_eq!(BASE64_STANDARD.encode(b"foo"), "Zm9v");
        assert_eq!(BASE64_STANDARD.encode(b"foob"), "Zm9vYg==");
        assert_eq!(BASE64_STANDARD.encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(BASE64_STANDARD.encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn test_decode_standard() {
        assert_eq!(BASE64_STANDARD.decode(b"").expect("decode"), b"");
        assert_eq!(BASE64_STANDARD.decode(b"Zg==").expect("decode"), b"f");
        assert_eq!(BASE64_STANDARD.decode(b"Zm8=").expect("decode"), b"fo");
        assert_eq!(BASE64_STANDARD.decode(b"Zm9v").expect("decode"), b"foo");
        assert_eq!(
            BASE64_STANDARD.decode(b"Zm9vYmFy").expect("decode"),
            b"foobar"
        );
    }

    #[test]
    fn test_encode_no_pad() {
        assert_eq!(BASE64_STANDARD_NO_PAD.encode(b"f"), "Zg");
        assert_eq!(BASE64_STANDARD_NO_PAD.encode(b"fo"), "Zm8");
        assert_eq!(BASE64_STANDARD_NO_PAD.encode(b"foo"), "Zm9v");
    }

    #[test]
    fn test_decode_no_pad() {
        assert_eq!(BASE64_STANDARD_NO_PAD.decode(b"Zg").expect("decode"), b"f");
        assert_eq!(
            BASE64_STANDARD_NO_PAD.decode(b"Zm8").expect("decode"),
            b"fo"
        );
    }

    #[test]
    fn test_url_safe() {
        let data = b"\xfb\xff\xfe";
        let encoded = BASE64_URL_SAFE_NO_PAD.encode(data);
        assert_eq!(encoded, "-__-");
        assert_eq!(
            BASE64_URL_SAFE_NO_PAD.decode(b"-__-").expect("decode"),
            data
        );
    }

    #[test]
    fn test_decode_invalid() {
        assert!(BASE64_STANDARD.decode(b"Z!!!").is_err());
    }

    #[test]
    fn test_roundtrip() {
        let original = b"Hello, world! \x00\xFF\xAB";
        assert_eq!(
            BASE64_STANDARD
                .decode(BASE64_STANDARD.encode(original))
                .expect("decode"),
            original
        );
    }

    #[test]
    fn test_block_boundaries() {
        for len in 0..130 {
            let input = (0..len).map(|i| (i * 37) as u8).collect::<Vec<_>>();
            for engine in [
                &BASE64_STANDARD,
                &BASE64_STANDARD_NO_PAD,
                &BASE64_URL_SAFE,
                &BASE64_URL_SAFE_NO_PAD,
            ] {
                let encoded = engine.encode(&input);
                assert_eq!(engine.decode(encoded).expect("decode"), input, "len={len}");
            }
        }
    }

    #[test]
    fn test_decode_padding_and_invalid_bytes() {
        assert_eq!(
            BASE64_STANDARD.decode(b"Zg==ignored").expect("decode"),
            b"f"
        );
        assert_eq!(
            BASE64_STANDARD_NO_PAD.decode(b"Zg==").expect("decode"),
            b"f"
        );
        assert!(BASE64_STANDARD.decode(b"Zm9v!mFy").is_err());
        assert!(BASE64_STANDARD.decode(b"Zm9vYm!y").is_err());
    }
}
