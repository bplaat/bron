/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [uuid](https://crates.io/crates/uuid) crate
//!
//! The base API supports `no_std`; `Uuid::new_v7` and `Uuid::now_v7` require `std`.

#![cfg_attr(not(any(test, feature = "std")), no_std)]

use core::fmt::{self, Debug, Display, Formatter, Write};
use core::ops::Range;
use core::str::FromStr;

// MARK: Uuid
/// UUID
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// Create zero UUID
    pub const fn nil() -> Uuid {
        Uuid([0; 16])
    }

    /// Create UUID from bytes
    pub const fn from_bytes(bytes: [u8; 16]) -> Uuid {
        Uuid(bytes)
    }

    /// Create UUID from slice
    pub fn from_slice(slice: &[u8]) -> Result<Uuid, Error> {
        slice
            .try_into()
            .map(Uuid)
            .map_err(|_| Error(ErrorKind::ByteLength { len: slice.len() }))
    }

    /// Get bytes from UUID
    pub const fn into_bytes(self) -> [u8; 16] {
        self.0
    }
}

impl Display for Uuid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for (i, byte) in self.0.iter().enumerate() {
            write!(f, "{byte:02x}")?;
            if matches!(i, 3 | 5 | 7 | 9) {
                f.write_char('-')?;
            }
        }
        Ok(())
    }
}

impl Debug for Uuid {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// MARK: Parsing
impl FromStr for Uuid {
    type Err = Error;

    /// Parse the simple, hyphenated, braced or urn formats
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match (s.len(), s.as_bytes()) {
            (32, bytes) => decode_hex(bytes.iter().copied())
                .map(Uuid)
                .ok_or_else(|| parse_error(s, false)),
            (36, _) => parse_hyphenated(s),
            (38, [b'{', .., b'}']) => parse_hyphenated(&s[1..37]),
            (45, bytes) if bytes[..9].eq_ignore_ascii_case(b"urn:uuid:") => {
                parse_hyphenated(&s[9..])
            }
            _ => Err(parse_error(s, false)),
        }
    }
}

/// Parse a 36 character hyphenated UUID
fn parse_hyphenated(s: &str) -> Result<Uuid, Error> {
    let bytes = s.as_bytes();
    [8, 13, 18, 23]
        .iter()
        .all(|&i| bytes[i] == b'-')
        .then(|| {
            decode_hex(
                bytes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !matches!(i, 8 | 13 | 18 | 23))
                    .map(|(_, &byte)| byte),
            )
        })
        .flatten()
        .map(Uuid)
        .ok_or_else(|| parse_error(s, true))
}

/// Decode 32 hex digits into 16 bytes
fn decode_hex(mut digits: impl Iterator<Item = u8>) -> Option<[u8; 16]> {
    let mut bytes = [0; 16];
    for byte in &mut bytes {
        let high = char::from(digits.next()?).to_digit(16)?;
        let low = char::from(digits.next()?).to_digit(16)?;
        *byte = (high << 4 | low) as u8;
    }
    Some(bytes)
}

/// Diagnose why `s` failed to parse, matching upstream error details
fn parse_error(s: &str, mut hyphenated: bool) -> Error {
    if s.is_empty() || s.len() > 45 {
        return Error(ErrorKind::Length { len: s.len() });
    }
    let bytes = s.as_bytes();
    let bounds: Range<usize> = match bytes {
        _ if hyphenated => 0..s.len(),
        [b'{', .., b'}'] => {
            hyphenated = true;
            1..s.len() - 1
        }
        _ if bytes.len() >= 9 && bytes[..9].eq_ignore_ascii_case(b"urn:uuid:") => {
            hyphenated = true;
            9..s.len()
        }
        _ => 0..s.len(),
    };

    let mut hyphen_count = 0;
    let mut group_bounds = [0; 4];
    for (index, character) in s[bounds.clone()].char_indices() {
        match character {
            '-' => {
                hyphenated = true;
                if let Some(bound) = group_bounds.get_mut(hyphen_count) {
                    *bound = index;
                }
                hyphen_count += 1;
            }
            _ if character.is_ascii_hexdigit() => {}
            _ => {
                return Error(ErrorKind::Char {
                    character,
                    index: index + bounds.start,
                });
            }
        }
    }

    if !hyphenated {
        return Error(ErrorKind::Length { len: s.len() });
    }
    if hyphen_count != 4 {
        return Error(ErrorKind::GroupCount {
            count: hyphen_count + 1,
        });
    }
    const GROUP_STARTS: [usize; 5] = [0, 9, 14, 19, 24];
    let group = (0..4)
        .find(|&i| group_bounds[i] != GROUP_STARTS[i + 1] - 1)
        .unwrap_or(4);
    let end = group_bounds.get(group).copied().unwrap_or(s.len());
    Error(ErrorKind::GroupLength {
        group,
        len: end - GROUP_STARTS[group],
    })
}

// MARK: Generation
#[cfg(feature = "v4")]
impl Uuid {
    /// Create UUID v4
    pub fn new_v4() -> Uuid {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).expect("Failed to generate random bytes");
        bytes[6] = bytes[6] & 0x0f | 0x40;
        bytes[8] = bytes[8] & 0x3f | 0x80;
        Uuid(bytes)
    }
}

#[cfg(all(feature = "v7", feature = "std"))]
impl Uuid {
    /// Create UUID v7 with current time
    pub fn now_v7() -> Uuid {
        Self::new_v7(std::time::SystemTime::now())
    }

    /// Create UUID v7 with time
    pub fn new_v7(time: std::time::SystemTime) -> Uuid {
        let timestamp = time
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64;
        let mut bytes = [0; 16];
        bytes[..6].copy_from_slice(&timestamp.to_be_bytes()[2..]);
        getrandom::fill(&mut bytes[6..]).expect("Failed to generate random bytes");
        bytes[6] = bytes[6] & 0x0f | 0x70;
        bytes[8] = bytes[8] & 0x3f | 0x80;
        Uuid(bytes)
    }
}

// MARK: Serde
#[cfg(feature = "serde")]
impl serde::Serialize for Uuid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.collect_str(self)
        } else {
            serializer.serialize_bytes(&self.0)
        }
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Uuid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UuidVisitor;

        fn de_error<E: serde::de::Error>(error: Error) -> E {
            E::custom(format_args!("UUID parsing failed: {error}"))
        }

        impl<'de> serde::de::Visitor<'de> for UuidVisitor {
            type Value = Uuid;

            fn expecting(&self, f: &mut Formatter<'_>) -> fmt::Result {
                f.write_str("a formatted UUID string or a 16 byte array")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Uuid, E> {
                value.parse().map_err(de_error)
            }

            fn visit_bytes<E: serde::de::Error>(self, value: &[u8]) -> Result<Uuid, E> {
                Uuid::from_slice(value).map_err(de_error)
            }

            fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Uuid, A::Error> {
                let mut bytes = [0; 16];
                for (i, byte) in bytes.iter_mut().enumerate() {
                    *byte = seq
                        .next_element()?
                        .ok_or_else(|| serde::de::Error::invalid_length(i, &self))?;
                }
                Ok(Uuid(bytes))
            }
        }

        if deserializer.is_human_readable() {
            deserializer.deserialize_str(UuidVisitor)
        } else {
            deserializer.deserialize_bytes(UuidVisitor)
        }
    }
}

// MARK: Error
/// A general error that can occur when working with UUIDs.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Error(ErrorKind);

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum ErrorKind {
    Char { character: char, index: usize },
    ByteLength { len: usize },
    GroupCount { count: usize },
    GroupLength { group: usize, len: usize },
    Length { len: usize },
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {
            ErrorKind::Char { character, index } => {
                write!(f, "invalid character: found `{character}` at {index}")
            }
            ErrorKind::ByteLength { len } => {
                write!(f, "invalid length: expected 16 bytes, found {len}")
            }
            ErrorKind::GroupCount { count } => {
                write!(f, "invalid group count: expected 5, found {count}")
            }
            ErrorKind::GroupLength { group, len } => {
                let expected = [8, 4, 4, 4, 12][group];
                write!(
                    f,
                    "invalid group length in group {group}: expected {expected}, found {len}"
                )
            }
            ErrorKind::Length { len } => write!(f, "invalid length: found {len}"),
        }
    }
}

impl core::error::Error for Error {}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    const BYTES: [u8; 16] = [
        0xa0, 0xb1, 0xc2, 0xd3, 0xe4, 0xf5, 0x67, 0x89, 0x9a, 0x0b, 0xcd, 0xef, 0x01, 0x23, 0x45,
        0x67,
    ];

    #[test]
    fn test_from_slice() {
        let uuid = Uuid::from_slice(&BYTES).unwrap();
        assert_eq!(uuid.to_string(), "a0b1c2d3-e4f5-6789-9a0b-cdef01234567");
    }

    #[test]
    fn test_from_slice_invalid() {
        let error = Uuid::from_slice(&BYTES[..9]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid length: expected 16 bytes, found 9"
        );
    }

    #[test]
    fn test_to_string() {
        let uuid = Uuid::from_bytes(BYTES);
        assert_eq!(uuid.to_string(), "a0b1c2d3-e4f5-6789-9a0b-cdef01234567");
        assert_eq!(format!("{uuid:?}"), "a0b1c2d3-e4f5-6789-9a0b-cdef01234567");
    }

    #[test]
    fn test_parse_string() {
        for input in [
            "a0b1c2d3-e4f5-6789-9a0b-cdef01234567",
            "A0B1C2D3-E4F5-6789-9A0B-CDEF01234567",
            "a0b1c2d3e4f567899a0bcdef01234567",
            "{a0b1c2d3-e4f5-6789-9a0b-cdef01234567}",
            "urn:uuid:a0b1c2d3-e4f5-6789-9a0b-cdef01234567",
            "URN:UUID:a0b1c2d3-e4f5-6789-9a0b-cdef01234567",
        ] {
            assert_eq!(
                input.parse::<Uuid>(),
                Ok(Uuid::from_bytes(BYTES)),
                "{input}"
            );
        }
    }

    #[test]
    fn test_parse_invalid_string() {
        for (input, message) in [
            ("", "invalid length: found 0"),
            (
                "a0b1c2d3e4f567899a0bcdef0123456",
                "invalid length: found 31",
            ),
            (
                "a0b1c2d3-e4f5-6789-9a0",
                "invalid group count: expected 5, found 4",
            ),
            (
                "a0b1c2d3-e4f5-6789-9a0b-cdef0123456g",
                "invalid character: found `g` at 35",
            ),
            (
                "a0b1c2d3e-4f5-6789-9a0b-cdef01234567",
                "invalid group length in group 0: expected 8, found 9",
            ),
            (
                "{a0b1c2d3-e4f5-6789-9a0b-cdef01234567",
                "invalid character: found `{` at 0",
            ),
            (
                "a0b1c2d3-e4f5-6789-9a0b-cdef012345678",
                "invalid group length in group 4: expected 12, found 13",
            ),
        ] {
            let error = input.parse::<Uuid>().unwrap_err();
            assert_eq!(error.to_string(), message, "{input}");
        }
    }

    #[test]
    #[cfg(feature = "v4")]
    fn test_generate_v4() {
        let bytes = Uuid::new_v4().into_bytes();
        assert_eq!(bytes[6] >> 4, 4);
        assert!(matches!(bytes[8] >> 6, 2 | 3));
    }

    #[test]
    #[cfg(feature = "v7")]
    fn test_generate_v7() {
        let bytes = Uuid::now_v7().into_bytes();
        assert_eq!(bytes[6] >> 4, 7);
        assert!(matches!(bytes[8] >> 6, 2 | 3));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn test_serde_serialization() {
        let serialized = serde_json::to_string(&Uuid::nil()).unwrap();
        assert_eq!(serialized, "\"00000000-0000-0000-0000-000000000000\"");
    }

    #[test]
    #[cfg(feature = "serde")]
    fn test_serde_deserialization() {
        let uuid: Uuid = serde_json::from_str("\"a0b1c2d3-e4f5-6789-9a0b-cdef01234567\"").unwrap();
        assert_eq!(uuid, Uuid::from_bytes(BYTES));
        let deserializer =
            serde::de::value::BytesDeserializer::<serde::de::value::Error>::new(&BYTES);
        let uuid: Uuid = serde::Deserialize::deserialize(deserializer).unwrap();
        assert_eq!(uuid, Uuid::from_bytes(BYTES));
    }

    #[test]
    #[cfg(feature = "serde")]
    fn test_serde_invalid_deserialization() {
        let result: Result<Uuid, _> = serde_json::from_str("\"invalid-uuid-string\"");
        assert!(result.is_err());
    }
}
