/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A DEFLATE and zlib compression library with an optimal parsing mode

#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use core::fmt;

mod decode;
mod encode;

pub(crate) const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
pub(crate) const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
pub(crate) const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
pub(crate) const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
pub(crate) const CODE_LENGTH_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

// MARK: Level
/// The compression time and size tradeoff.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Level {
    /// Greedy matching for quick compression.
    Fast,
    /// Lazy matching with block splitting.
    #[default]
    Default,
    /// Optimal parsing, usually smaller than zlib level 9.
    Best,
    /// Repeated optimal parsing with a deep match search, for when size matters most.
    Crunch,
}

// MARK: Error
/// A failure to decompress data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// The input is not a valid stream or has a wrong checksum.
    InvalidData,
    /// The output would exceed the limit.
    TooLarge,
    /// The output could not be allocated.
    OutOfMemory,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidData => "invalid compressed data",
            Self::TooLarge => "decompressed data too large",
            Self::OutOfMemory => "out of memory",
        })
    }
}

impl core::error::Error for Error {}

// MARK: DEFLATE
/// Compress bytes into a raw DEFLATE stream.
pub fn compress(data: &[u8], level: Level) -> Vec<u8> {
    let mut out = Vec::new();
    encode::compress(data, level, &mut out);
    out
}

/// Decompress a raw DEFLATE stream that fills `data` exactly, producing at most `limit` bytes.
pub fn decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    decompress_with_size_hint(data, limit, 0)
}

/// Decompress like [`decompress`], reserving the output size a container records, such as the
/// gzip or ZIP size field, so the output is allocated once. The hint is not trusted: it is capped
/// by `limit` and by how far `data` can expand, and a wrong hint only costs a reallocation.
pub fn decompress_with_size_hint(
    data: &[u8],
    limit: usize,
    size_hint: usize,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    reserve_hint(&mut out, data, limit, size_hint)?;
    let consumed = decode::decompress(data, limit, &mut out)?;
    if consumed != data.len() {
        return Err(Error::InvalidData);
    }
    Ok(fit(out))
}

/// The most a DEFLATE stream expands: a 258 byte match coded in about two bits.
const MAX_EXPANSION: usize = 1032;

fn reserve_hint(
    out: &mut Vec<u8>,
    data: &[u8],
    limit: usize,
    size_hint: usize,
) -> Result<(), Error> {
    let size_hint = size_hint
        .min(limit)
        .min(data.len().saturating_mul(MAX_EXPANSION));
    out.try_reserve_exact(size_hint)
        .map_err(|_| Error::OutOfMemory)
}

/// Release growth slack, so a long-lived output does not hold up to twice its size.
fn fit(mut out: Vec<u8>) -> Vec<u8> {
    if out.capacity() - out.len() > out.len() / 8 {
        out.shrink_to_fit();
    }
    out
}

// MARK: zlib
/// Compress bytes into a zlib stream.
pub fn zlib_compress(data: &[u8], level: Level) -> Vec<u8> {
    let mut out = Vec::new();
    // Deflate with a 32 KiB window; the level hint is informational only.
    out.extend_from_slice(&[0x78, if level == Level::Fast { 0x01 } else { 0xda }]);
    encode::compress(data, level, &mut out);
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

/// Decompress a zlib stream that fills `data` exactly, producing at most `limit` bytes.
pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    let (out, checksum) = zlib_inflate(data, limit)?;
    if checksum != adler32(&out).to_be_bytes() {
        return Err(Error::InvalidData);
    }
    Ok(fit(out))
}

/// Decompress a zlib stream like [`zlib_decompress`], without verifying its Adler-32 checksum, for
/// formats that already checksum the compressed stream, like PNG.
pub fn zlib_decompress_unchecked(data: &[u8], limit: usize) -> Result<Vec<u8>, Error> {
    zlib_inflate(data, limit).map(|(out, _)| fit(out))
}

/// Decompress a zlib stream, returning the output and the 4-byte Adler-32 trailer.
fn zlib_inflate(data: &[u8], limit: usize) -> Result<(Vec<u8>, [u8; 4]), Error> {
    let [cmf, flg, ..] = *data else {
        return Err(Error::InvalidData);
    };
    if cmf & 0x0f != 8
        || cmf >> 4 > 7
        || flg & 0x20 != 0
        || u16::from_be_bytes([cmf, flg]) % 31 != 0
    {
        return Err(Error::InvalidData);
    }
    let mut out = Vec::new();
    let consumed = 2 + decode::decompress(&data[2..], limit, &mut out)?;
    let checksum = data
        .get(consumed..)
        .and_then(|checksum| checksum.try_into().ok())
        .ok_or(Error::InvalidData)?;
    Ok((out, checksum))
}

/// Compute the Adler-32 checksum used by zlib.
fn adler32(data: &[u8]) -> u32 {
    // 5552 is the most bytes that can be summed before `b` may overflow.
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    #[test]
    fn size_hints_allocate_once_and_are_not_trusted() {
        let data = alloc::vec![42u8; 100_000];
        let compressed = compress(&data, Level::Fast);
        let out = decompress_with_size_hint(&compressed, usize::MAX, data.len()).unwrap();
        assert_eq!(
            (out.as_slice(), out.capacity()),
            (data.as_slice(), data.len())
        );
        // A hint beyond what the input can expand to is capped instead of allocated.
        let out = decompress_with_size_hint(&compressed, usize::MAX, usize::MAX / 2).unwrap();
        assert_eq!(out, data);
        assert!(out.capacity() <= compressed.len() * MAX_EXPANSION);
        // Without a hint, growth slack is released.
        let out = decompress(&compressed, usize::MAX).unwrap();
        assert!(out.capacity() - out.len() <= out.len() / 8);
    }

    const LEVELS: [Level; 4] = [Level::Fast, Level::Default, Level::Best, Level::Crunch];

    /// A deterministic xorshift generator for test data.
    fn xorshift(mut state: u32) -> impl FnMut() -> u32 {
        move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        }
    }

    fn samples() -> Vec<Vec<u8>> {
        let mut random = xorshift(0x1234_5678);
        let text = b"The quick brown fox jumps over the lazy dog. ".repeat(500);
        let noise: Vec<u8> = (0..70_000).map(|_| random() as u8).collect();
        let pixels: Vec<u8> = (0..40_000u32)
            .map(|i| ((i % 1024) / 7 + (i / 4096) * 3) as u8)
            .collect();
        let mixed: Vec<u8> = (0..40_000)
            .map(|i| {
                if (i / 2000) % 2 == 0 {
                    random() as u8 & 3
                } else {
                    (i % 251) as u8
                }
            })
            .collect();
        vec![
            Vec::new(),
            vec![42],
            vec![0; 30_000],
            text,
            noise,
            pixels,
            mixed,
        ]
    }

    #[test]
    fn roundtrips_all_levels() {
        for data in samples() {
            for level in LEVELS {
                let compressed = compress(&data, level);
                assert!(
                    decompress(&compressed, data.len()).as_ref() == Ok(&data),
                    "{level:?}"
                );
            }
            let zlib = zlib_compress(&data, Level::Default);
            assert!(zlib_decompress(&zlib, data.len()).as_ref() == Ok(&data));
        }
    }

    #[test]
    fn roundtrips_matches_a_full_window_back() {
        // Low entropy bytes give many partial matches exactly one window back.
        let data: Vec<u8> = samples()[4][..1 << 15]
            .iter()
            .map(|byte| byte & 3)
            .collect::<Vec<_>>()
            .repeat(4)
            .iter()
            .enumerate()
            .map(|(i, &byte)| if i % 61 == 0 { byte ^ 1 } else { byte })
            .collect();
        for level in LEVELS {
            assert!(
                decompress(&compress(&data, level), data.len()).as_ref() == Ok(&data),
                "{level:?}"
            );
        }
    }

    #[test]
    fn roundtrips_random_inputs() {
        let mut next = xorshift(0x9e37_79b9);
        let mut random = || next() as usize;
        for _ in 0..200 {
            // Vary the size, alphabet and repetition to reach every block type.
            let len = random() % 3000;
            let alphabet = 1 + random() % 256;
            let base = random() as u8;
            let mut data: Vec<u8> = (0..len)
                .map(|_| base.wrapping_add((random() % alphabet) as u8))
                .collect();
            if len > 0 && random() % 2 == 0 {
                let period = 1 + random() % len;
                for i in period..len {
                    if random() % 8 != 0 {
                        data[i] = data[i - period];
                    }
                }
            }
            for level in LEVELS {
                assert!(
                    decompress(&compress(&data, level), len).as_ref() == Ok(&data),
                    "{level:?}"
                );
            }
        }
    }

    #[test]
    fn roundtrips_fixed_blocks_with_high_literals() {
        // Short repetitive input favors a fixed Huffman block, whose 9-bit literal
        // codes cover bytes 144 to 255.
        let data = [0xc8, 0x90, 0xff, 0x41].repeat(8);
        for level in LEVELS {
            let compressed = compress(&data, level);
            assert_eq!(compressed[0] & 6, 2, "{level:?} uses a fixed block");
            assert_eq!(
                decompress(&compressed, data.len()),
                Ok(data.clone()),
                "{level:?}"
            );
        }
    }

    #[test]
    fn higher_levels_compress_better() {
        let data = &samples()[5];
        let sizes = LEVELS.map(|level| compress(data, level).len());
        assert!(
            sizes[0] >= sizes[1] && sizes[1] > sizes[2] && sizes[2] >= sizes[3],
            "{sizes:?}"
        );
    }

    #[test]
    fn incompressible_data_is_stored() {
        let data = &samples()[4];
        assert!(compress(data, Level::Default).len() < data.len() + 32);
    }

    #[test]
    fn decompresses_reference_streams() {
        // Python's `zlib.compress(b"hello hello hello")`, which uses a fixed Huffman block.
        let zlib = [
            0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x90, 0x00, 0x3a, 0x2e,
            0x06, 0x7d,
        ];
        assert_eq!(
            zlib_decompress(&zlib, 64).as_deref(),
            Ok(&b"hello hello hello"[..])
        );
        // A stored block holding "abc".
        let stored = [0x01, 0x03, 0x00, 0xfc, 0xff, b'a', b'b', b'c'];
        assert_eq!(decompress(&stored, 3).as_deref(), Ok(&b"abc"[..]));
        assert_eq!(decompress(&[0x03, 0x00], 0), Ok(Vec::new()));
    }

    #[test]
    fn rejects_invalid_streams() {
        let data = b"hello hello hello hello".repeat(20);
        let zlib = zlib_compress(&data, Level::Default);
        assert_eq!(zlib_decompress(&zlib, data.len() - 1), Err(Error::TooLarge));
        assert_eq!(
            zlib_decompress(&zlib[..zlib.len() - 1], data.len()),
            Err(Error::InvalidData)
        );
        let mut bad_checksum = zlib.clone();
        *bad_checksum.last_mut().expect("checksum") ^= 1;
        assert_eq!(
            zlib_decompress(&bad_checksum, data.len()),
            Err(Error::InvalidData)
        );
        let mut trailing = zlib.clone();
        trailing.push(0);
        assert_eq!(
            zlib_decompress(&trailing, data.len()),
            Err(Error::InvalidData)
        );
        assert_eq!(
            zlib_decompress(&[0x78, 0x9d, 0x03, 0x00], 0),
            Err(Error::InvalidData)
        );
        // Reserved block type and a stored block with a mismatched length complement.
        assert_eq!(decompress(&[0x07], 10), Err(Error::InvalidData));
        assert_eq!(
            decompress(&[0x01, 0x03, 0x00, 0xfc, 0xfe, 1, 2, 3], 10),
            Err(Error::InvalidData)
        );
        // A distance reaching before the start of the output.
        assert_eq!(decompress(&[0x03, 0x02, 0x00], 10), Err(Error::InvalidData));
    }

    #[test]
    fn survives_corrupted_input() {
        let data = &samples()[6];
        let compressed = compress(data, Level::Best);
        let mut random = xorshift(99);
        for _ in 0..2000 {
            let state = random();
            let mut corrupt = compressed.clone();
            let index = (state >> 8) as usize % corrupt.len();
            corrupt[index] ^= 1 << (state % 8);
            let truncated = &corrupt[..corrupt.len() - (state as usize >> 20) % 4];
            let _ = decompress(truncated, data.len());
        }
    }

    #[test]
    fn computes_adler32() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(adler32(&[0xff; 100_000]), 0x149a_302c);
    }
}
