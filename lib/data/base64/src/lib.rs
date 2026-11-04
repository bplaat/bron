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
        // Input bytes of a bulk encoding block and symbols of a bulk decoding block. Both turn
        // into 64 symbols, which arithmetic maps at once where table lookups go one by one.
        const ENCODE_BLOCK: usize = 48;
        const DECODE_BLOCK: usize = 64;

        // The symbol of a 6-bit value, with the two symbols that differ between alphabets.
        #[inline(always)]
        const fn symbol(value: u8, plus: u8, slash: u8) -> u8 {
            let letter = if value < 26 { value + b'A' } else { value + 71 };
            let digit = if value < 62 {
                value.wrapping_sub(4)
            } else if value == 62 {
                plus
            } else {
                slash
            };
            if value < 52 { letter } else { digit }
        }

        // The 6-bit value of a symbol, or `INVALID_VALUE`.
        #[inline(always)]
        const fn symbol_value(byte: u8, plus: u8, slash: u8) -> u8 {
            let mut value = INVALID_VALUE;
            if byte.wrapping_sub(b'A') < 26 {
                value = byte.wrapping_sub(b'A');
            }
            if byte.wrapping_sub(b'a') < 26 {
                value = byte.wrapping_sub(71);
            }
            if byte.wrapping_sub(b'0') < 10 {
                value = byte.wrapping_add(4);
            }
            if byte == plus {
                value = 62;
            }
            if byte == slash {
                value = 63;
            }
            value
        }

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
                let [plus, slash] = [self.encode_table[62], self.encode_table[63]];
                let (blocks, rest) = input.as_chunks::<ENCODE_BLOCK>();
                for block in blocks {
                    let mut values = [0u8; 64];
                    for (group, value) in block
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .zip(values.as_chunks_mut::<4>().0)
                    {
                        let &[b0, b1, b2] = group;
                        *value = [
                            b0 >> 2,
                            (b0 & 0x03) << 4 | b1 >> 4,
                            (b1 & 0x0f) << 2 | b2 >> 6,
                            b2 & 0x3f,
                        ];
                    }
                    for value in &mut values {
                        *value = symbol(*value, plus, slash);
                    }
                    out.extend_from_slice(&values);
                }
                let (chunks, tail) = rest.as_chunks::<3>();
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
                let [plus, slash] = [self.encode_table[62], self.encode_table[63]];
                let blocks = input[..quads_len].as_chunks::<DECODE_BLOCK>().0;
                let mut valid_len = 0;
                for block in blocks {
                    let mut values = [0u8; DECODE_BLOCK];
                    let mut seen = 0;
                    for (value, &byte) in values.iter_mut().zip(block) {
                        *value = symbol_value(byte, plus, slash);
                        seen |= *value;
                    }
                    // Valid values fit in 6 bits; the quads below find any invalid byte.
                    if seen & 0xc0 != 0 {
                        break;
                    }
                    let mut bytes = [0u8; 48];
                    for (bytes, group) in bytes
                        .as_chunks_mut::<3>()
                        .0
                        .iter_mut()
                        .zip(values.as_chunks::<4>().0)
                    {
                        let &[a, b, c, d] = group;
                        *bytes = [(a << 2) | (b >> 4), (b << 4) | (c >> 2), (c << 6) | d];
                    }
                    out.extend_from_slice(&bytes);
                    valid_len += DECODE_BLOCK;
                }
                let (quads, _) = input[valid_len..quads_len].as_chunks::<4>();
                for (index, quad) in quads.iter().enumerate() {
                    let [a, b, c, d] = quad.map(|byte| self.decode_table[byte as usize]);
                    if (a | b | c | d) == INVALID_VALUE {
                        return Err(self.invalid_byte(quad, valid_len + index * 4));
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
    fn bulk_blocks_match_the_tables() {
        for engine in ENGINES {
            let alphabet: Vec<u8> = (0..64)
                .map(|value| engine.encode([value << 2]).into_bytes()[0])
                .collect();
            for byte in 0..=u8::MAX {
                let mut symbols = [b'A'; 128];
                symbols[77] = byte;
                let value = alphabet.iter().position(|&symbol| symbol == byte);
                let decoded = engine.decode(symbols);
                match value {
                    Some(value) => {
                        let bytes = decoded.expect("valid symbol");
                        let packed = u32::from_be_bytes([bytes[57], bytes[58], 0, 0]);
                        assert_eq!(packed >> 20 & 63, value as u32);
                    }
                    None => assert_eq!(decoded, Err(DecodeError::InvalidByte(77, byte))),
                }
            }
            for length in [0, 1, 2, 3, 47, 48, 49, 50, 95, 96, 97, 480, 1024] {
                let data: Vec<u8> = (0..length).map(|index| (index * 211 + 7) as u8).collect();
                let mut expected = String::new();
                for group in data.chunks(3) {
                    let mut value = [0u8; 3];
                    value[..group.len()].copy_from_slice(group);
                    let value = u32::from_be_bytes([0, value[0], value[1], value[2]]);
                    for index in 0..=group.len() {
                        expected.push(char::from(
                            alphabet[(value >> (18 - 6 * index) & 63) as usize],
                        ));
                    }
                }
                let encoded = engine.encode(&data);
                assert_eq!(encoded.trim_end_matches('='), expected);
                assert_eq!(engine.decode(encoded).expect("round trip"), data);
            }
        }
    }

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
