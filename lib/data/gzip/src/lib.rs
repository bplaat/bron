/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Decode a single gzip member into memory.

#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::vec::Vec;

use crc::{CRC_32_ISO_HDLC, Crc};

const MAX_OUTPUT_SIZE: usize = 1024 * 1024 * 1024;

static CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

fn skip_c_string(data: &[u8], offset: &mut usize) -> Option<()> {
    *offset += data.get(*offset..)?.iter().position(|&byte| byte == 0)? + 1;
    Some(())
}

/// Compress bytes into a single gzip member.
pub fn compress(data: &[u8]) -> Vec<u8> {
    let payload = deflate::compress(data, deflate::Level::Default);
    let mut output = Vec::with_capacity(18 + payload.len());
    output.extend_from_slice(&[0x1f, 0x8b, 0x08, 0, 0, 0, 0, 0, 0, 255]);
    output.extend_from_slice(&payload);
    output.extend_from_slice(&CRC32.checksum(data).to_le_bytes());
    output.extend_from_slice(&(data.len() as u32).to_le_bytes());
    output
}

/// Decompress a single gzip member, returning `None` for invalid data or output over 1 GiB.
pub fn decompress(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 18 || data[..3] != [0x1f, 0x8b, 0x08] || data[3] & 0xe0 != 0 {
        return None;
    }
    let flags = data[3];
    let footer = data.len() - 8;
    let mut offset = 10usize;
    if flags & 0x04 != 0 {
        let xlen = u16::from_le_bytes(data.get(offset..offset + 2)?.try_into().ok()?) as usize;
        offset = offset.checked_add(2 + xlen)?;
    }
    if flags & 0x08 != 0 {
        skip_c_string(data.get(..footer)?, &mut offset)?;
    }
    if flags & 0x10 != 0 {
        skip_c_string(data.get(..footer)?, &mut offset)?;
    }
    if flags & 0x02 != 0 {
        let expected = u16::from_le_bytes(data.get(offset..offset + 2)?.try_into().ok()?);
        if CRC32.checksum(data.get(..offset)?) as u16 != expected {
            return None;
        }
        offset += 2;
    }
    let compressed = data.get(offset..footer)?;
    let expected_crc = u32::from_le_bytes(data.get(footer..footer + 4)?.try_into().ok()?);
    let expected_size = u32::from_le_bytes(data.get(footer + 4..)?.try_into().ok()?);
    // The trailer records the size, so the output is allocated once at its exact size.
    let output =
        deflate::decompress_with_size_hint(compressed, MAX_OUTPUT_SIZE, expected_size as usize)
            .ok()?;
    (CRC32.checksum(&output) == expected_crc && output.len() as u32 == expected_size)
        .then_some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SMALL: &[u8] = include_bytes!("../tests/fixtures/small.gz");

    #[test]
    fn decompresses_gzip() {
        assert_eq!(decompress(SMALL).as_deref(), Some(&b"hello world"[..]));
    }

    #[test]
    fn roundtrips_gzip() {
        for size in [0, 1, 1024, 1024 * 1024] {
            let data = (0..size).map(|i| i as u8).collect::<Vec<_>>();
            assert_eq!(decompress(&compress(&data)), Some(data));
        }
    }

    #[test]
    fn rejects_invalid_header_and_trailer() {
        let mut data = SMALL.to_vec();
        data[3] = 0x20;
        assert!(decompress(&data).is_none());
        data[3] = 0;
        let footer = data.len() - 8;
        data[footer] ^= 1;
        assert!(decompress(&data).is_none());
        assert!(decompress(&SMALL[..SMALL.len() - 1]).is_none());
    }

    #[test]
    fn handles_optional_header_fields() {
        let mut data = SMALL.to_vec();
        data[3] = 0x1c;
        data.splice(10..10, [2, 0, 1, 2, b'a', 0, b'b', 0]);
        assert_eq!(decompress(&data).as_deref(), Some(&b"hello world"[..]));
        data[3] |= 0x02;
        let header_crc = CRC32.checksum(&data[..18]) as u16;
        data.splice(18..18, header_crc.to_le_bytes());
        assert_eq!(decompress(&data).as_deref(), Some(&b"hello world"[..]));
        data[18] ^= 1;
        assert!(decompress(&data).is_none());
    }
}
