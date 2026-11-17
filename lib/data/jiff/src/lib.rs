/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [jiff](https://crates.io/crates/jiff) crate
//!
//! A [Timestamp] is an instant in UTC, [civil::Date] and [civil::DateTime] are wall-clock values
//! without a time zone, and [ZonedDateTime] is a timestamp in a [tz::TimeZone], which is UTC or
//! the system's time zone. Values have whole second precision, sub-second parts are dropped, and
//! years range from -9999 to 9999 like jiff's.
//!
//! Disable default features for `no_std` support. The `alloc` feature adds the RFC 9110 printer,
//! `std` adds [Timestamp::now], `tz-system` adds [tz::TimeZone::system] and `serde` serializes
//! timestamps and dates as RFC 3339 strings.

#![cfg_attr(not(any(test, feature = "std")), no_std)]
#![allow(
    clippy::missing_const_for_fn,
    reason = "the public API is const exactly where jiff's is"
)]

#[cfg(feature = "alloc")]
extern crate alloc;

use core::fmt::{Display, Formatter};

pub use crate::timestamp::Timestamp;
pub use crate::zoned::ZonedDateTime;

pub mod civil;
#[cfg(feature = "alloc")]
pub mod fmt;
#[cfg(feature = "tz-system")]
mod sys;
mod timestamp;
pub mod tz;
mod zoned;

// MARK: Error
/// A date and time error
#[derive(Clone, Debug)]
pub struct Error(&'static str);

impl Error {
    pub(crate) const INVALID_DATE: Self = Self("invalid date");
    pub(crate) const INVALID_TIME: Self = Self("invalid time");
    pub(crate) const OUT_OF_RANGE: Self = Self("timestamp out of range");
    pub(crate) const PARSE: Self = Self("invalid date and time string");
    #[cfg(feature = "tz-system")]
    pub(crate) const TIME_ZONE: Self = Self("time zone conversion failed");
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

impl core::error::Error for Error {}

// MARK: Parsing
/// Parses fields of exactly the given number of digits, which the separator splits
pub(crate) fn parse_fields<const N: usize>(
    s: &str,
    separator: char,
    widths: [usize; N],
) -> Result<[i16; N], Error> {
    let mut parts = s.split(separator);
    let mut fields = [0; N];
    for (field, width) in fields.iter_mut().zip(widths) {
        *field = parts
            .next()
            .filter(|part| part.len() == width && part.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|part| part.parse().ok())
            .ok_or(Error::PARSE)?;
    }
    match parts.next() {
        Some(_) => Err(Error::PARSE),
        None => Ok(fields),
    }
}

// MARK: Serde
#[cfg(feature = "serde")]
macro_rules! serde_from_str {
    ($($type:ty),*) => {$(
        impl serde::Serialize for $type {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $type {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                alloc::string::String::deserialize(deserializer)?
                    .parse()
                    .map_err(serde::de::Error::custom)
            }
        }
    )*};
}

#[cfg(feature = "serde")]
serde_from_str!(Timestamp, civil::Date);

#[cfg(all(test, feature = "serde"))]
mod test {
    use super::*;

    #[test]
    fn test_serde() {
        let timestamp = Timestamp::from_second(1_582_977_600).unwrap();
        let json = serde_json::to_string(&timestamp).unwrap();
        assert_eq!(json, "\"2020-02-29T12:00:00Z\"");
        assert_eq!(serde_json::from_str::<Timestamp>(&json).unwrap(), timestamp);

        let date = civil::date(2024, 6, 15);
        let json = serde_json::to_string(&date).unwrap();
        assert_eq!(json, "\"2024-06-15\"");
        assert_eq!(serde_json::from_str::<civil::Date>(&json).unwrap(), date);
        assert!(serde_json::from_str::<civil::Date>("\"2024-02-30\"").is_err());
    }
}
