/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [base64](https://crates.io/crates/base64) crate

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use core::fmt;

pub use engine::Engine;

// MARK: DecodeError
/// Errors that can occur while decoding.
#[derive(Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// An invalid byte was found in the input. The offset and offending byte are provided.
    InvalidByte(usize, u8),
    /// The length of the input, as measured in valid base64 symbols, is invalid.
    InvalidLength(usize),
    /// The last non-padding input symbol's encoded 6 bits have nonzero bits that will be discarded.
    InvalidLastSymbol {
        /// Offset in the input
        offset: usize,
        /// The offending symbol
        symbol: u8,
        /// The bits the symbol corresponds to.
        symbol_value: u8,
    },
    /// The nature of the padding was not as configured.
    InvalidPadding,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::InvalidByte(index, byte) => write!(f, "Invalid symbol {byte}, offset {index}."),
            Self::InvalidLength(len) => write!(f, "Invalid input length: {len}"),
            Self::InvalidLastSymbol {
                offset,
                symbol,
                symbol_value,
            } => write!(
                f,
                "Invalid last symbol {symbol:#4x} ('{}') at offset {offset}, decoded as {symbol_value:#010b}.",
                char::from(symbol)
            ),
            Self::InvalidPadding => write!(f, "Invalid padding"),
        }
    }
}

impl fmt::Debug for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl core::error::Error for DecodeError {}

// MARK: Engine
/// Base64 engines and the [`Engine`] trait.
pub mod engine {
    use alloc::string::String;
    use alloc::vec::Vec;

    use crate::DecodeError;

    /// Trait for base64 encoding and decoding engines.
    pub trait Engine: Send + Sync {
        /// Encode bytes to a base64 `String`.
        fn encode<T: AsRef<[u8]>>(&self, input: T) -> String;
        /// Decode a base64 byte string to `Vec<u8>`.
        fn decode<T: AsRef<[u8]>>(&self, input: T) -> Result<Vec<u8>, DecodeError>;
    }

    /// The general-purpose engine and its common configurations.
    pub mod general_purpose {
        use alloc::string::String;
        use alloc::vec::Vec;

        use super::Engine;
        use crate::DecodeError;

        const INVALID_VALUE: u8 = 0xff;
        const PAD_BYTE: u8 = b'=';

        /// A general-purpose base64 engine.
        #[derive(Debug, Clone)]
        pub struct GeneralPurpose {
            encode_table: &'static [u8; 64],
            decode_table: &'static [u8; 256],
            /// Pad when encoding and require canonical padding when decoding, or
            /// omit padding when encoding and reject it when decoding.
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
                        out.push(PAD_BYTE);
                    }
                    if self.padding {
                        out.push(PAD_BYTE);
                    }
                }
                // SAFETY: The encoder only pushes bytes from the base64 alphabet (A-Z, a-z, 0-9, +, /, -, _, =), all of which are valid UTF-8.
                unsafe { String::from_utf8_unchecked(out) }
            }

            fn decode<T: AsRef<[u8]>>(&self, input: T) -> Result<Vec<u8>, DecodeError> {
                let input = input.as_ref();
                let rem = input.len() % 4;

                // Report a trailing invalid byte (like a newline) before a length error
                if rem == 1
                    && let Some(&last) = input.last()
                    && last != PAD_BYTE
                    && self.decode_table[last as usize] == INVALID_VALUE
                {
                    return Err(DecodeError::InvalidByte(input.len() - 1, last));
                }

                // The last quad is decoded separately, as it may contain padding
                let quads_len = input
                    .len()
                    .saturating_sub(rem)
                    .saturating_sub(usize::from(rem == 0) * 4);
                let mut out = Vec::with_capacity(input.len().div_ceil(4) * 3);
                let (quads, _) = input[..quads_len].as_chunks::<4>();
                for (index, quad) in quads.iter().enumerate() {
                    let [a, b, c, d] = quad.map(|byte| self.decode_table[byte as usize]);
                    if (a | b | c | d) == INVALID_VALUE {
                        return Err(self.invalid_byte(quad, index * 4));
                    }
                    out.extend_from_slice(&[
                        (a << 2) | (b >> 4),
                        (b << 4) | (c >> 2),
                        (c << 6) | d,
                    ]);
                }
                self.decode_suffix(&input[quads_len..], quads_len, out)
            }
        }

        impl GeneralPurpose {
            /// Return the error for the first invalid byte in a quad at `offset`.
            fn invalid_byte(&self, quad: &[u8; 4], offset: usize) -> DecodeError {
                let index = quad
                    .iter()
                    .position(|&byte| self.decode_table[byte as usize] == INVALID_VALUE)
                    .unwrap_or_default();
                DecodeError::InvalidByte(offset + index, quad[index])
            }

            /// Decode the last 0-4 input bytes (starting at `offset`), checking padding and trailing bits.
            fn decode_suffix(
                &self,
                input: &[u8],
                offset: usize,
                mut out: Vec<u8>,
            ) -> Result<Vec<u8>, DecodeError> {
                let mut symbols = 0;
                let mut padding = 0;
                let mut accum = 0u32;
                let mut last = (0, 0);
                for (index, &byte) in input.iter().enumerate() {
                    if byte == PAD_BYTE {
                        // Padding may only follow at least two symbols
                        if index < 2 {
                            return Err(DecodeError::InvalidByte(offset + index, byte));
                        }
                        padding += 1;
                        continue;
                    }
                    // Symbols may not follow padding
                    if padding > 0 {
                        return Err(DecodeError::InvalidByte(offset + symbols, PAD_BYTE));
                    }
                    let value = self.decode_table[byte as usize];
                    if value == INVALID_VALUE {
                        return Err(DecodeError::InvalidByte(offset + index, byte));
                    }
                    accum |= u32::from(value) << (26 - 6 * symbols);
                    symbols += 1;
                    last = (byte, value);
                }
                if !input.is_empty() && symbols < 2 {
                    return Err(DecodeError::InvalidLength(offset + symbols));
                }
                if (self.padding && (symbols + padding) % 4 != 0) || (!self.padding && padding > 0)
                {
                    return Err(DecodeError::InvalidPadding);
                }

                // Reject non-canonical encodings whose last symbol has unused bits set
                let bytes_len = symbols * 6 / 8;
                if accum & (!0 >> (bytes_len * 8)) != 0 {
                    return Err(DecodeError::InvalidLastSymbol {
                        offset: offset + symbols - 1,
                        symbol: last.0,
                        symbol_value: last.1,
                    });
                }
                out.extend_from_slice(&accum.to_be_bytes()[..bytes_len]);
                Ok(out)
            }
        }

        // MARK: Alphabet tables
        const fn build_decode_table(encode_table: &[u8; 64]) -> [u8; 256] {
            let mut table = [INVALID_VALUE; 256];
            let mut i = 0usize;
            while i < 64 {
                table[encode_table[i] as usize] = i as u8;
                i += 1;
            }
            table
        }

        const STANDARD_ENCODE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        static STANDARD_DECODE: [u8; 256] = build_decode_table(STANDARD_ENCODE);

        const URL_SAFE_ENCODE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        static URL_SAFE_DECODE: [u8; 256] = build_decode_table(URL_SAFE_ENCODE);

        // MARK: Engines
        /// Standard alphabet (`+` and `/`), padded when encoding and canonical padding required when decoding.
        ///
        /// Does not allow trailing bits when decoding.
        pub const STANDARD: GeneralPurpose = GeneralPurpose {
            encode_table: STANDARD_ENCODE,
            decode_table: &STANDARD_DECODE,
            padding: true,
        };

        /// Standard alphabet (`+` and `/`), no padding when encoding and padding rejected when decoding.
        ///
        /// Does not allow trailing bits when decoding.
        pub const STANDARD_NO_PAD: GeneralPurpose = GeneralPurpose {
            encode_table: STANDARD_ENCODE,
            decode_table: &STANDARD_DECODE,
            padding: false,
        };

        /// URL-safe alphabet (`-` and `_`), padded when encoding and canonical padding required when decoding.
        ///
        /// Does not allow trailing bits when decoding.
        pub const URL_SAFE: GeneralPurpose = GeneralPurpose {
            encode_table: URL_SAFE_ENCODE,
            decode_table: &URL_SAFE_DECODE,
            padding: true,
        };

        /// URL-safe alphabet (`-` and `_`), no padding when encoding and padding rejected when decoding.
        ///
        /// Does not allow trailing bits when decoding.
        pub const URL_SAFE_NO_PAD: GeneralPurpose = GeneralPurpose {
            encode_table: URL_SAFE_ENCODE,
            decode_table: &URL_SAFE_DECODE,
            padding: false,
        };
    }
}

// MARK: Prelude
/// Preconfigured engines under more explicit names, plus the [`Engine`] trait.
pub mod prelude {
    pub use crate::engine::Engine;
    pub use crate::engine::general_purpose::{
        STANDARD as BASE64_STANDARD, STANDARD_NO_PAD as BASE64_STANDARD_NO_PAD,
        URL_SAFE as BASE64_URL_SAFE, URL_SAFE_NO_PAD as BASE64_URL_SAFE_NO_PAD,
    };
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::DecodeError;
    use super::engine::general_purpose::GeneralPurpose;
    use super::prelude::*;

    const ENGINES: [&GeneralPurpose; 4] = [
        &BASE64_STANDARD,
        &BASE64_STANDARD_NO_PAD,
        &BASE64_URL_SAFE,
        &BASE64_URL_SAFE_NO_PAD,
    ];

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
            for engine in ENGINES {
                let encoded = engine.encode(&input);
                assert_eq!(engine.decode(encoded).expect("decode"), input, "len={len}");
            }
        }
    }

    #[test]
    fn test_decode_invalid_byte() {
        assert_eq!(
            BASE64_STANDARD.decode(b"Z!!!"),
            Err(DecodeError::InvalidByte(1, b'!'))
        );
        assert_eq!(
            BASE64_STANDARD.decode(b"Zm9v!mFy"),
            Err(DecodeError::InvalidByte(4, b'!'))
        );
        assert_eq!(
            BASE64_STANDARD.decode(b"Zm9vYm!y"),
            Err(DecodeError::InvalidByte(6, b'!'))
        );
        assert_eq!(
            BASE64_STANDARD.decode(b"Zm9v\n"),
            Err(DecodeError::InvalidByte(4, b'\n'))
        );
        assert_eq!(
            BASE64_STANDARD.decode(b"Zg==ignored"),
            Err(DecodeError::InvalidByte(2, b'='))
        );
        assert_eq!(
            BASE64_STANDARD.decode(b"Z==="),
            Err(DecodeError::InvalidByte(1, b'='))
        );
        assert_eq!(
            BASE64_STANDARD.decode(b"Zm=8"),
            Err(DecodeError::InvalidByte(2, b'='))
        );
        assert_eq!(
            BASE64_URL_SAFE.decode(b"+w=="),
            Err(DecodeError::InvalidByte(0, b'+'))
        );
    }

    #[test]
    fn test_decode_invalid_length() {
        assert_eq!(
            BASE64_STANDARD.decode(b"Z"),
            Err(DecodeError::InvalidLength(1))
        );
        assert_eq!(
            BASE64_STANDARD_NO_PAD.decode(b"Zm9vY"),
            Err(DecodeError::InvalidLength(5))
        );
    }

    #[test]
    fn test_decode_padding() {
        assert_eq!(
            BASE64_STANDARD.decode(b"Zg"),
            Err(DecodeError::InvalidPadding)
        );
        assert_eq!(
            BASE64_STANDARD.decode(b"Zg="),
            Err(DecodeError::InvalidPadding)
        );
        assert_eq!(
            BASE64_STANDARD_NO_PAD.decode(b"Zg=="),
            Err(DecodeError::InvalidPadding)
        );
        assert_eq!(
            BASE64_URL_SAFE_NO_PAD.decode(b"Zm8="),
            Err(DecodeError::InvalidPadding)
        );
    }

    #[test]
    fn test_decode_trailing_bits() {
        assert_eq!(
            BASE64_STANDARD.decode(b"Zh=="),
            Err(DecodeError::InvalidLastSymbol {
                offset: 1,
                symbol: b'h',
                symbol_value: 33,
            })
        );
        assert_eq!(
            BASE64_STANDARD_NO_PAD.decode(b"Zm9"),
            Err(DecodeError::InvalidLastSymbol {
                offset: 2,
                symbol: b'9',
                symbol_value: 61,
            })
        );
    }

    #[test]
    fn test_decode_error_display() {
        assert_eq!(
            DecodeError::InvalidByte(4, b'!').to_string(),
            "Invalid symbol 33, offset 4."
        );
        assert_eq!(
            DecodeError::InvalidLength(5).to_string(),
            "Invalid input length: 5"
        );
        assert_eq!(
            DecodeError::InvalidLastSymbol {
                offset: 1,
                symbol: b'h',
                symbol_value: 33,
            }
            .to_string(),
            "Invalid last symbol 0x68 ('h') at offset 1, decoded as 0b00100001."
        );
        assert_eq!(DecodeError::InvalidPadding.to_string(), "Invalid padding");
    }
}
