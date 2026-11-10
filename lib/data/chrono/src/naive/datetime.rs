/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "serde")]
use alloc::string::{String, ToString};
use core::fmt::{self, Debug, Display, Formatter};
use core::ops::{Add, Sub};
use core::str::FromStr;
use core::time::Duration;

use crate::consts::{SECS_IN_DAY, SECS_IN_HOUR, SECS_IN_MIN};
use crate::utils::{parse_fraction, parse_hms, timestamp_to_ymd, write_fraction};
use crate::{DateTime, NaiveDate, ParseError, Utc};

// MARK: NaiveDateTime
/// A DateTime without timezone
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NaiveDateTime(i64, u32);

impl NaiveDateTime {
    /// Create a [NaiveDateTime] from a unix timestamp
    #[deprecated]
    pub const fn from_timestamp(secs: i64, nsecs: u32) -> Option<Self> {
        if nsecs >= 1_000_000_000 {
            return None;
        }
        Some(Self(secs, nsecs))
    }

    /// Get the [NaiveDate] of the date and time
    pub const fn date(&self) -> NaiveDate {
        NaiveDate::from_timestamp(self.0)
    }

    /// Get the year
    pub fn year(&self) -> i32 {
        self.date().year()
    }

    /// Get the month, starting at 1
    pub fn month(&self) -> u32 {
        self.date().month()
    }

    /// Get the day of the month, starting at 1
    pub fn day(&self) -> u32 {
        self.date().day()
    }

    /// Get the hour
    pub const fn hour(&self) -> u32 {
        (self.0.rem_euclid(SECS_IN_DAY) / SECS_IN_HOUR) as u32
    }

    /// Get the minute
    pub const fn minute(&self) -> u32 {
        (self.0.rem_euclid(SECS_IN_HOUR) / SECS_IN_MIN) as u32
    }

    /// Get the second
    pub const fn second(&self) -> u32 {
        self.0.rem_euclid(SECS_IN_MIN) as u32
    }

    /// Get the fractional second in nanoseconds.
    pub const fn nanosecond(&self) -> u32 {
        self.1
    }

    /// Get the [DateTime] in UTC timezone
    pub const fn and_utc(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(self.0, self.1).expect("Should be some")
    }

    fn shift(self, nanos: i128) -> Self {
        let total = i128::from(self.0) * 1_000_000_000 + i128::from(self.1) + nanos;
        Self(
            i64::try_from(total.div_euclid(1_000_000_000)).expect("datetime overflow"),
            total.rem_euclid(1_000_000_000) as u32,
        )
    }

    /// Get the unix timestamp of the [NaiveDateTime]
    #[deprecated]
    pub const fn timestamp(&self) -> i64 {
        self.0
    }
}

impl Add<Duration> for NaiveDateTime {
    type Output = Self;

    fn add(self, duration: Duration) -> Self::Output {
        self.shift(duration.as_nanos() as i128)
    }
}

impl Sub<Duration> for NaiveDateTime {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self::Output {
        self.shift(-(duration.as_nanos() as i128))
    }
}

impl FromStr for NaiveDateTime {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (date_part, time_part) = s.split_once(' ').ok_or(ParseError)?;
        let (time_part, nanos) = parse_fraction(time_part)?;
        let (hour, minute, second) = parse_hms(time_part)?;
        NaiveDate::from_str(date_part)?
            .and_hms_nano_opt(hour, minute, second, nanos)
            .ok_or(ParseError)
    }
}

impl Display for NaiveDateTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (year, month, day) = timestamp_to_ymd(self.0);
        let day_sec = self.0.rem_euclid(SECS_IN_DAY);
        write!(
            f,
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            year,
            month,
            day,
            day_sec / SECS_IN_HOUR,
            (day_sec % SECS_IN_HOUR) / SECS_IN_MIN,
            day_sec % SECS_IN_MIN
        )?;
        write_fraction(f, self.1)
    }
}

impl Debug for NaiveDateTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("NaiveDateTime(")?;
        Display::fmt(self, f)?;
        f.write_str(")")
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for NaiveDateTime {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for NaiveDateTime {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

// MARK: Tests
#[cfg(test)]
#[allow(deprecated)]
mod test {
    use super::*;

    #[test]
    fn fractional_datetimes_cross_second_and_day_boundaries() {
        let datetime: NaiveDateTime = "1969-12-31 23:59:59.999999999".parse().unwrap();
        assert_eq!(datetime.nanosecond(), 999_999_999);
        let midnight = datetime + Duration::from_nanos(1);
        assert_eq!(midnight.to_string(), "1970-01-01 00:00:00");
        assert_eq!(midnight - Duration::from_nanos(1), datetime);
        assert_eq!(
            NaiveDate::from_ymd_opt(1969, 12, 31)
                .unwrap()
                .and_hms_nano_opt(23, 59, 59, 999_999_999),
            Some(datetime)
        );
        assert!(NaiveDateTime::from_timestamp(0, 1_000_000_000).is_none());
        for fraction in ["", "1234567890", "12x"] {
            assert!(
                format!("2020-01-01 00:00:00.{fraction}")
                    .parse::<NaiveDateTime>()
                    .is_err()
            );
        }
    }

    #[test]
    fn test_timestamp() {
        let datetime = NaiveDateTime::from_timestamp(1609459345, 0).unwrap();
        assert_eq!(datetime.timestamp(), 1609459345);
    }

    #[test]
    fn test_from_str() {
        let datetime: NaiveDateTime = "2019-02-28 12:00:00".parse().unwrap();
        assert_eq!(datetime.timestamp(), 1551355200);
        let datetime: NaiveDateTime = "2020-02-29 12:00:00".parse().unwrap();
        assert_eq!(datetime.timestamp(), 1582977600);
        let datetime: NaiveDateTime = "1969-12-20 10:13:20".parse().unwrap();
        assert_eq!(datetime.timestamp(), -1000000);

        assert!("invalid-datetime".parse::<NaiveDateTime>().is_err());
        assert!("2020-02-29 12:00:00Z".parse::<NaiveDateTime>().is_err());
        assert!("2020-02-29 12:00:00T".parse::<NaiveDateTime>().is_err());
        assert!("2020-02-30 12:00:00".parse::<NaiveDateTime>().is_err());
        assert!("2020-02-29 25:00:00".parse::<NaiveDateTime>().is_err());
        assert!("2020-02-29 12:60:00".parse::<NaiveDateTime>().is_err());
        assert!("2020-02-29 12:00:60".parse::<NaiveDateTime>().is_err());
        assert!("2021-02-29 12:00:00".parse::<NaiveDateTime>().is_err());
        assert!("2019-04-31 12:00:00".parse::<NaiveDateTime>().is_err());
    }

    #[test]
    fn test_accessors() {
        let datetime: NaiveDateTime = "2020-02-29 12:34:56".parse().unwrap();
        assert_eq!(
            (datetime.year(), datetime.month(), datetime.day()),
            (2020, 2, 29)
        );
        assert_eq!(
            (datetime.hour(), datetime.minute(), datetime.second()),
            (12, 34, 56)
        );
        let datetime: NaiveDateTime = "1969-12-20 10:13:20".parse().unwrap();
        assert_eq!(
            (datetime.hour(), datetime.minute(), datetime.second()),
            (10, 13, 20)
        );
        assert_eq!(
            format!("{datetime:?}"),
            "NaiveDateTime(1969-12-20 10:13:20)"
        );
    }

    #[test]
    fn test_display() {
        let datetime = NaiveDateTime::from_timestamp(1551355200, 0).unwrap();
        assert_eq!(datetime.to_string(), "2019-02-28 12:00:00");
        let datetime = NaiveDateTime::from_timestamp(1582977600, 0).unwrap();
        assert_eq!(datetime.to_string(), "2020-02-29 12:00:00");
        let datetime = NaiveDateTime::from_timestamp(-1000000, 0).unwrap();
        assert_eq!(datetime.to_string(), "1969-12-20 10:13:20");
    }

    #[test]
    fn test_add_duration() {
        let datetime = NaiveDateTime::from_timestamp(1609459200, 0).unwrap();
        let new_datetime = datetime + Duration::from_secs(1);
        assert_eq!(new_datetime.timestamp(), 1609459201);
    }

    #[test]
    fn test_sub_duration() {
        let datetime = NaiveDateTime::from_timestamp(1609459200, 0).unwrap();
        let new_datetime = datetime - Duration::from_secs(1);
        assert_eq!(new_datetime.timestamp(), 1609459199);
    }
}
