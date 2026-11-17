/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::fmt::{self, Debug, Display, Formatter};
use core::ops::{Add, AddAssign, Sub, SubAssign};
use core::str::FromStr;
use core::time::Duration;

use crate::civil::DateTime;
use crate::tz::TimeZone;
use crate::{Error, ZonedDateTime, parse_fields};

// MARK: Timestamp
/// An instant in time, as seconds since 1970-01-01T00:00:00Z
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The earliest timestamp, -009999-01-02T01:59:59Z, it's -9999-01-01 in every time zone
    pub const MIN: Self = Self(-377_705_023_201);
    /// The latest timestamp, 9999-12-30T22:00:00Z, it's 9999-12-31 in every time zone
    pub const MAX: Self = Self(253_402_207_200);
    /// 1970-01-01T00:00:00Z
    pub const UNIX_EPOCH: Self = Self(0);

    /// Returns the current time of the system clock
    #[cfg(feature = "std")]
    pub fn now() -> Self {
        let second = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => duration.as_secs() as i64,
            // Round down before 1970 too
            Err(error) => {
                let duration = error.duration();
                -(duration.as_secs() as i64) - i64::from(duration.subsec_nanos() > 0)
            }
        };
        Self::from_second(second).expect("system time out of range")
    }

    /// Creates a timestamp from seconds since 1970-01-01T00:00:00Z
    pub fn from_second(second: i64) -> Result<Self, Error> {
        if !(Self::MIN.0..=Self::MAX.0).contains(&second) {
            return Err(Error::OUT_OF_RANGE);
        }
        Ok(Self(second))
    }

    /// Returns the seconds since 1970-01-01T00:00:00Z
    pub fn as_second(self) -> i64 {
        self.0
    }

    /// Returns this timestamp in a time zone
    pub fn to_zoned_datetime(self, tz: TimeZone) -> ZonedDateTime {
        ZonedDateTime::new(self, tz)
    }

    /// Adds the whole seconds of a duration, fails when the result is out of range
    pub fn checked_add(self, duration: Duration) -> Result<Self, Error> {
        i64::try_from(duration.as_secs())
            .ok()
            .and_then(|second| self.0.checked_add(second))
            .map_or(Err(Error::OUT_OF_RANGE), Self::from_second)
    }

    /// Subtracts the whole seconds of a duration, fails when the result is out of range
    pub fn checked_sub(self, duration: Duration) -> Result<Self, Error> {
        i64::try_from(duration.as_secs())
            .ok()
            .and_then(|second| self.0.checked_sub(second))
            .map_or(Err(Error::OUT_OF_RANGE), Self::from_second)
    }
}

/// Adds the whole seconds of a duration, panics when the result is out of range
impl Add<Duration> for Timestamp {
    type Output = Self;

    fn add(self, duration: Duration) -> Self {
        self.checked_add(duration)
            .expect("adding duration to timestamp overflowed")
    }
}

impl AddAssign<Duration> for Timestamp {
    fn add_assign(&mut self, duration: Duration) {
        *self = *self + duration;
    }
}

/// Subtracts the whole seconds of a duration, panics when the result is out of range
impl Sub<Duration> for Timestamp {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self {
        self.checked_sub(duration)
            .expect("subtracting duration from timestamp overflowed")
    }
}

impl SubAssign<Duration> for Timestamp {
    fn sub_assign(&mut self, duration: Duration) {
        *self = *self - duration;
    }
}

impl FromStr for Timestamp {
    type Err = Error;

    /// Parses an RFC 3339 date and time with a `Z` or `±HH:MM` offset
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (datetime, offset) = match s.strip_suffix('Z') {
            Some(datetime) => (datetime, 0),
            None => {
                let (datetime, offset) = s
                    .split_at_checked(s.len().wrapping_sub(6))
                    .ok_or(Error::PARSE)?;
                let sign = match offset.as_bytes()[0] {
                    b'+' => 1,
                    b'-' => -1,
                    _ => return Err(Error::PARSE),
                };
                let [hours, minutes] = parse_fields(&offset[1..], ':', [2, 2])?;
                if hours > 23 || minutes > 59 {
                    return Err(Error::PARSE);
                }
                (
                    datetime,
                    sign * (i64::from(hours) * 3600 + i64::from(minutes) * 60),
                )
            }
        };
        let datetime: DateTime = datetime.parse()?;
        Self::from_second(datetime.to_second() - offset)
    }
}

/// Formats as RFC 3339 in UTC
impl Display for Timestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}Z", DateTime::from_second(self.0))
    }
}

impl Debug for Timestamp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_range() {
        assert_eq!(Timestamp::MIN.to_string(), "-009999-01-02T01:59:59Z");
        assert_eq!(Timestamp::MAX.to_string(), "9999-12-30T22:00:00Z");
        assert!(Timestamp::from_second(Timestamp::MIN.as_second() - 1).is_err());
        assert!(Timestamp::from_second(Timestamp::MAX.as_second() + 1).is_err());
    }

    #[test]
    fn test_display() {
        let display = |second| Timestamp::from_second(second).unwrap().to_string();
        assert_eq!(display(0), "1970-01-01T00:00:00Z");
        assert_eq!(display(1_582_977_600), "2020-02-29T12:00:00Z");
        assert_eq!(display(-1_000_000), "1969-12-20T10:13:20Z");
    }

    #[test]
    fn test_parse() {
        let parse = |s: &str| s.parse::<Timestamp>().ok().map(Timestamp::as_second);
        assert_eq!(parse("2020-02-29T12:00:00Z"), Some(1_582_977_600));
        assert_eq!(parse("2020-02-29T12:00:00.123Z"), Some(1_582_977_600));
        assert_eq!(parse("2020-02-29T14:00:00+02:00"), Some(1_582_977_600));
        assert_eq!(parse("2020-02-29T06:30:00.5-05:30"), Some(1_582_977_600));
        assert_eq!(parse("1969-12-20T10:13:20Z"), Some(-1_000_000));
        for s in [
            "",
            "2020-02-29",
            "2020-02-29T12:00:00",
            "2020-02-29 12:00:00Z",
            "2020-02-30T12:00:00Z",
            "2020-02-29T24:00:00Z",
            "2020-02-29T12:00:00.Z",
            "2020-02-29T12:00:00+02",
            "2020-02-29T12:00:00+2:00",
            "2020-02-29T12:00:00+24:00",
            "9999-12-31T23:59:59Z",
        ] {
            assert!(parse(s).is_none(), "{s}");
        }
    }

    #[test]
    fn test_duration_arithmetic() {
        let timestamp = Timestamp::from_second(1_609_459_200).unwrap();
        assert_eq!(
            (timestamp + Duration::from_millis(1500)).as_second(),
            1_609_459_201
        );
        assert_eq!(
            (timestamp - Duration::from_secs(1)).as_second(),
            1_609_459_199
        );
        assert!(Timestamp::MAX.checked_add(Duration::from_secs(1)).is_err());
        assert!(Timestamp::MIN.checked_sub(Duration::from_secs(1)).is_err());
        assert!(Timestamp::UNIX_EPOCH.checked_add(Duration::MAX).is_err());
    }

    #[cfg(feature = "std")]
    #[test]
    fn test_now() {
        let system = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        assert!((Timestamp::now().as_second() - system).abs() <= 1);
    }
}
