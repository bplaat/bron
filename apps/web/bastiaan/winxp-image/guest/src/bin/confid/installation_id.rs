/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Installation ID decoding: decimal digit groups with check digits, holding a little-endian
//! number whose decryption contains the hardware ID and the product ID.

use crate::Error;
use crate::cipher::feistel;

const KEY: [u8; 4] = [0x6A, 0xC8, 0x5E, 0xD4];

/// The number of bytes of the decoded number that matter
const LEN: usize = 19;

/// The fields of a decoded installation ID
#[derive(Debug, PartialEq, Eq)]
pub struct InstallationId {
    pub hardware_id: u64,
    /// The four parts of the product ID AAAAA-BBB-CCCCCCC-DDEEE
    pub product_id: [u64; 4],
}

/// Reads a little-endian unsigned integer
pub fn le_int(bytes: &[u8]) -> u128 {
    bytes
        .iter()
        .rev()
        .fold(0, |acc, &b| acc << 8 | u128::from(b))
}

/// Reads the digits into a number modulo 2^(8 * LEN) and checks the groups, which hold five
/// digits followed by a check digit, the last group can be shorter. Spaces and dashes are
/// ignored. Returns the number and the count of digits without check digits.
fn read_digits(text: &str) -> Result<([u8; LEN], usize), Error> {
    let bytes = text.as_bytes();
    let mut number = [0u8; LEN];
    let (mut digits, mut group, mut sum) = (0, 0, 0);
    for (i, &c) in bytes.iter().enumerate() {
        if c == b' ' || c == b'-' {
            continue;
        }
        if !c.is_ascii_digit() {
            return Err(Error::InvalidCharacter);
        }
        let digit = c - b'0';
        if group == 5 || i == bytes.len() - 1 {
            if group == 0 {
                return Err(if digits == 45 {
                    Error::TooLarge
                } else {
                    Error::TooShort
                });
            }
            if u32::from(digit) != sum % 7 {
                return Err(if group < 5 {
                    Error::TooShort
                } else {
                    Error::InvalidCheckDigit
                });
            }
            (group, sum) = (0, 0);
            continue;
        }
        sum += u32::from(digit) << (group & 1);
        group += 1;
        digits += 1;
        if digits > 50 {
            return Err(Error::TooLarge);
        }
        let mut carry = u16::from(digit);
        for byte in &mut number {
            carry += u16::from(*byte) * 10;
            *byte = carry as u8;
            carry >>= 8;
        }
    }
    if digits != 41 && digits < 45 {
        return Err(Error::TooShort);
    }
    Ok((number, digits))
}

/// Decodes and decrypts an installation ID. The 41 digit format belongs to version 9, the 45
/// digit (and longer) format to version 10.
pub fn decode(text: &str) -> Result<InstallationId, Error> {
    let (mut data, digits) = read_digits(text)?;
    let short = digits == 41;
    feistel(&mut data[..if short { 17 } else { LEN }], &KEY, false);
    if data[18] >= 0x10 {
        return Err(Error::UnknownVersion);
    }
    let product = le_int(&data[8..16]) as u64;
    let high = u64::from(data[16]);
    if (product >> 51) & 0xF != if short { 9 } else { 10 } {
        return Err(Error::UnknownVersion);
    }
    Ok(InstallationId {
        hardware_id: le_int(&data[..8]) as u64,
        product_id: [
            product & 0x1_FFFF,
            (product >> 17) & 0x3FF,
            (product >> 27) & 0xFF_FFFF,
            (product >> 55) | high << 9,
        ],
    })
}
