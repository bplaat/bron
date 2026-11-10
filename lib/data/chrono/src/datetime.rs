/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::format;
use alloc::string::{String, ToString};
use core::fmt::{self, Debug, Display, Formatter};
use core::marker::PhantomData;
use core::ops::{Add, Sub};
use core::str::FromStr;
use core::time::Duration;

use crate::consts::{DAY_NAMES, DAYS_IN_WEEK, MONTH_NAMES, SECS_IN_DAY, SECS_IN_HOUR, SECS_IN_MIN};
use crate::utils::{parse_fraction, parse_hms, parse_offset, timestamp_to_ymd, write_fraction};
use crate::{NaiveDate, NaiveDateTime, ParseError, TimeZone};

// MARK: DateTime
/// A DateTime in UTC timezone
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DateTime<T: TimeZone>(i64, u32, PhantomData<T>);

impl<T: TimeZone> DateTime<T> {
    /// Create a [DateTime] from a unix timestamp
    pub const fn from_timestamp_secs(secs: i64) -> Option<Self> {
        Self::from_timestamp(secs, 0)
    }

    /// Create a [DateTime] from Unix seconds and fractional nanoseconds.
    pub const fn from_timestamp(secs: i64, nanos: u32) -> Option<Self> {
        if nanos >= 1_000_000_000 {
            return None;
        }
        Some(Self(secs, nanos, PhantomData))
    }

    /// Create a [DateTime] from Unix milliseconds.
    pub const fn from_timestamp_millis(millis: i64) -> Option<Self> {
        Self::from_timestamp(
            millis.div_euclid(1_000),
            millis.rem_euclid(1_000) as u32 * 1_000_000,
        )
    }

    /// Create a [DateTime] from Unix microseconds.
    pub const fn from_timestamp_micros(micros: i64) -> Option<Self> {
        Self::from_timestamp(
            micros.div_euclid(1_000_000),
            micros.rem_euclid(1_000_000) as u32 * 1_000,
        )
    }

    /// Create a [DateTime] from Unix nanoseconds.
    pub const fn from_timestamp_nanos(nanos: i64) -> Self {
        Self(
            nanos.div_euclid(1_000_000_000),
            nanos.rem_euclid(1_000_000_000) as u32,
            PhantomData,
        )
    }

    /// Get the fractional second in nanoseconds.
    pub const fn timestamp_subsec_nanos(&self) -> u32 {
        self.1
    }

    /// Get the fractional second in microseconds.
    pub const fn timestamp_subsec_micros(&self) -> u32 {
        self.1 / 1_000
    }

    /// Get the fractional second in milliseconds.
    pub const fn timestamp_subsec_millis(&self) -> u32 {
        self.1 / 1_000_000
    }

    /// Get the Unix timestamp in milliseconds. Panics on overflow.
    pub fn timestamp_millis(&self) -> i64 {
        i64::try_from(i128::from(self.0) * 1_000 + i128::from(self.1 / 1_000_000))
            .expect("millisecond timestamp overflow")
    }

    /// Get the Unix timestamp in microseconds. Panics on overflow.
    pub fn timestamp_micros(&self) -> i64 {
        i64::try_from(i128::from(self.0) * 1_000_000 + i128::from(self.1 / 1_000))
            .expect("microsecond timestamp overflow")
    }

    /// Get the Unix timestamp in nanoseconds, if representable.
    pub fn timestamp_nanos_opt(&self) -> Option<i64> {
        i64::try_from(i128::from(self.0) * 1_000_000_000 + i128::from(self.1)).ok()
    }

    /// Get the NaiveDateTime
    pub const fn naive_utc(&self) -> NaiveDateTime {
        #[allow(deprecated)]
        NaiveDateTime::from_timestamp(self.0, self.1).expect("Should be some")
    }

    /// Get the NaiveDateTime in the wall-clock time of the timezone
    pub fn naive_local(&self) -> NaiveDateTime {
        #[allow(deprecated)]
        NaiveDateTime::from_timestamp(self.local_secs().0, self.1).expect("Should be some")
    }

    /// Get the same moment in time in another timezone
    pub const fn with_timezone<U: TimeZone>(&self, _tz: &U) -> DateTime<U> {
        DateTime(self.0, self.1, PhantomData)
    }

    /// Get the unix timestamp of the date and time
    pub const fn timestamp(&self) -> i64 {
        self.0
    }

    /// Format to RFC 2822 string
    pub fn to_rfc2822(&self) -> String {
        let (local, offset) = self.local_secs();
        let (year, month, day) = timestamp_to_ymd(local);
        let week_day = (local.div_euclid(SECS_IN_DAY) + 4).rem_euclid(DAYS_IN_WEEK); // 1970-01-01 was a Thursday
        let day_sec = local.rem_euclid(SECS_IN_DAY);
        let zone = if offset == 0 {
            "GMT".to_string()
        } else {
            format!(
                "{}{:02}{:02}",
                if offset < 0 { '-' } else { '+' },
                offset.abs() / SECS_IN_HOUR,
                (offset.abs() % SECS_IN_HOUR) / SECS_IN_MIN
            )
        };
        format!(
            "{}, {:02} {} {} {:02}:{:02}:{:02} {}",
            DAY_NAMES[week_day as usize],
            day,
            MONTH_NAMES[month as usize - 1],
            year,
            day_sec / SECS_IN_HOUR,
            (day_sec % SECS_IN_HOUR) / SECS_IN_MIN,
            day_sec % SECS_IN_MIN,
            zone
        )
    }

    /// Format to RFC 3339 string
    pub fn to_rfc3339(&self) -> String {
        self.to_string()
    }

    /// Get the wall-clock seconds since 1970 as if they were UTC and the timezone offset
    fn local_secs(&self) -> (i64, i64) {
        let offset = T::offset_secs(self.0);
        (self.0 + offset, offset)
    }
}

impl<T: TimeZone> Add<Duration> for DateTime<T> {
    type Output = Self;

    fn add(self, duration: Duration) -> Self::Output {
        let value = (self.naive_utc() + duration).and_utc();
        Self(value.0, value.1, PhantomData)
    }
}

impl<T: TimeZone> Sub<Duration> for DateTime<T> {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self::Output {
        let value = (self.naive_utc() - duration).and_utc();
        Self(value.0, value.1, PhantomData)
    }
}

impl<T: TimeZone> FromStr for DateTime<T> {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (date_part, time_part) = s.split_once('T').ok_or(ParseError)?;
        let (time_part, offset) = match time_part.strip_suffix('Z') {
            Some(time_part) => (time_part, 0),
            None => {
                let (time_part, offset) =
                    time_part.split_at(time_part.rfind(['+', '-']).ok_or(ParseError)?);
                (time_part, parse_offset(offset)?)
            }
        };
        let (time_part, nanos) = parse_fraction(time_part)?;

        let (hour, minute, second) = parse_hms(time_part)?;
        let local = NaiveDate::from_str(date_part)?
            .and_hms_opt(hour, minute, second)
            .ok_or(ParseError)?;
        Self::from_timestamp(local.and_utc().timestamp() - offset, nanos).ok_or(ParseError)
    }
}

impl<T: TimeZone> Display for DateTime<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (local, offset) = self.local_secs();
        let (year, month, day) = timestamp_to_ymd(local);
        let day_sec = local.rem_euclid(SECS_IN_DAY);
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            year,
            month,
            day,
            day_sec / SECS_IN_HOUR,
            (day_sec % SECS_IN_HOUR) / SECS_IN_MIN,
            day_sec % SECS_IN_MIN
        )?;
        write_fraction(f, self.1)?;
        if offset == 0 {
            f.write_str("Z")
        } else {
            write!(
                f,
                "{}{:02}:{:02}",
                if offset < 0 { '-' } else { '+' },
                offset.abs() / SECS_IN_HOUR,
                (offset.abs() % SECS_IN_HOUR) / SECS_IN_MIN
            )
        }
    }
}

impl<T: TimeZone> Debug for DateTime<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("DateTime(")?;
        Display::fmt(self, f)?;
        f.write_str(")")
    }
}

#[cfg(feature = "serde")]
impl<T: TimeZone> serde::Serialize for DateTime<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(feature = "serde")]
impl<'de, T: TimeZone> serde::Deserialize<'de> for DateTime<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;
    use crate::Utc;

    #[test]
    fn nanoseconds_survive_parsing_formatting_and_arithmetic() {
        let datetime: DateTime<Utc> = "2020-02-29T14:00:00.123456789+02:00".parse().unwrap();
        assert_eq!(datetime.to_rfc3339(), "2020-02-29T12:00:00.123456789Z");
        assert_eq!(datetime.timestamp_subsec_nanos(), 123_456_789);
        assert_eq!(datetime.timestamp_millis(), 1_582_977_600_123);
        assert_eq!(datetime.timestamp_micros(), 1_582_977_600_123_456);
        assert_eq!(
            datetime.timestamp_nanos_opt(),
            Some(1_582_977_600_123_456_789)
        );
        assert_eq!(datetime.naive_utc().and_utc(), datetime);
        assert_eq!(datetime.with_timezone(&Utc), datetime);
        let later = datetime + Duration::from_nanos(900_000_000);
        assert_eq!(later.to_string(), "2020-02-29T12:00:01.023456789Z");
        assert_eq!(later - Duration::from_nanos(900_000_000), datetime);
        assert!(DateTime::<Utc>::from_timestamp(0, 1_000_000_000).is_none());
        assert!(
            "2020-02-29T12:00:00.1234567890Z"
                .parse::<DateTime<Utc>>()
                .is_err()
        );
    }

    #[test]
    fn subsecond_timestamps_before_the_epoch() {
        let datetime = DateTime::<Utc>::from_timestamp_nanos(-1);
        assert_eq!(datetime.to_string(), "1969-12-31T23:59:59.999999999Z");
        assert_eq!(datetime.timestamp(), -1);
        assert_eq!(datetime.timestamp_nanos_opt(), Some(-1));
        assert_eq!(datetime.timestamp_millis(), -1);
        assert_eq!(datetime.timestamp_micros(), -1);
        assert_eq!(
            DateTime::<Utc>::from_timestamp_millis(-1)
                .unwrap()
                .to_string(),
            "1969-12-31T23:59:59.999Z"
        );
        assert_eq!(
            DateTime::<Utc>::from_timestamp_micros(-1)
                .unwrap()
                .to_string(),
            "1969-12-31T23:59:59.999999Z"
        );
        assert_eq!(
            (DateTime::<Utc>::from_timestamp_secs(0).unwrap() - Duration::from_nanos(1)),
            datetime
        );
        for millis in [i64::MIN, -1, 0, 1, i64::MAX] {
            assert_eq!(
                DateTime::<Utc>::from_timestamp_millis(millis)
                    .unwrap()
                    .timestamp_millis(),
                millis
            );
        }
        assert_eq!(
            DateTime::<Utc>::from_timestamp_secs(i64::MAX)
                .unwrap()
                .timestamp_nanos_opt(),
            None
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_preserves_fractional_seconds() {
        let datetime = DateTime::<Utc>::from_timestamp(1_700_000_000, 123_456_789).unwrap();
        let json = serde_json::to_string(&datetime).unwrap();
        assert_eq!(json, "\"2023-11-14T22:13:20.123456789Z\"");
        assert_eq!(
            serde_json::from_str::<DateTime<Utc>>(&json).unwrap(),
            datetime
        );
        let naive = datetime.naive_utc();
        let json = serde_json::to_string(&naive).unwrap();
        assert_eq!(serde_json::from_str::<NaiveDateTime>(&json).unwrap(), naive);
    }

    #[test]
    fn test_timestamp() {
        let datetime = DateTime::<Utc>::from_timestamp_secs(1609459345).unwrap();
        assert_eq!(datetime.timestamp(), 1609459345);
    }

    #[test]
    fn test_to_rfc2822() {
        let datetime = DateTime::<Utc>::from_timestamp_secs(1000000).unwrap();
        assert_eq!(datetime.to_rfc2822(), "Mon, 12 Jan 1970 13:46:40 GMT");
        let datetime = DateTime::<Utc>::from_timestamp_secs(1582977600).unwrap();
        assert_eq!(datetime.to_rfc2822(), "Sat, 29 Feb 2020 12:00:00 GMT");
        let datetime = DateTime::<Utc>::from_timestamp_secs(-1000000).unwrap();
        assert_eq!(datetime.to_rfc2822(), "Sat, 20 Dec 1969 10:13:20 GMT");
    }

    #[test]
    fn test_to_rfc3339() {
        let datetime = DateTime::<Utc>::from_timestamp_secs(1609459345).unwrap();
        assert_eq!(datetime.to_rfc3339(), "2021-01-01T00:02:25Z");
        let datetime = DateTime::<Utc>::from_timestamp_secs(1582977600).unwrap();
        assert_eq!(datetime.to_rfc3339(), "2020-02-29T12:00:00Z");
        let datetime = DateTime::<Utc>::from_timestamp_secs(-1000000).unwrap();
        assert_eq!(datetime.to_rfc3339(), "1969-12-20T10:13:20Z");
    }

    #[test]
    fn test_from_str() {
        let datetime: DateTime<Utc> = "2019-02-28T12:00:00Z".parse().unwrap();
        assert_eq!(datetime.timestamp(), 1551355200);
        let datetime: DateTime<Utc> = "2020-02-29T12:00:00Z".parse().unwrap();
        assert_eq!(datetime.timestamp(), 1582977600);
        let datetime: DateTime<Utc> = "1969-12-20T10:13:20Z".parse().unwrap();
        assert_eq!(datetime.timestamp(), -1000000);

        assert!("invalid-datetime".parse::<DateTime::<Utc>>().is_err());
        assert!("2020-02-29 12:00:00Z".parse::<DateTime::<Utc>>().is_err());
        assert!("2020-02-29T12:00:00T".parse::<DateTime::<Utc>>().is_err());
        assert!("2020-02-30T12:00:00Z".parse::<DateTime::<Utc>>().is_err());
        assert!("2020-02-29T25:00:00Z".parse::<DateTime::<Utc>>().is_err());
        assert!("2020-02-29T12:60:00Z".parse::<DateTime::<Utc>>().is_err());
        assert!("2020-02-29T12:00:60Z".parse::<DateTime::<Utc>>().is_err());
        assert!("2020-02-29T12:00:00".parse::<DateTime::<Utc>>().is_err());
        assert!("2021-02-29T12:00:00Z".parse::<DateTime::<Utc>>().is_err());
        assert!("2019-04-31T12:00:00Z".parse::<DateTime::<Utc>>().is_err());
    }

    #[test]
    fn test_from_str_fraction_and_offset() {
        let datetime: DateTime<Utc> = "2020-02-29T12:00:00.123Z".parse().unwrap();
        assert_eq!(datetime.timestamp(), 1582977600);
        let datetime: DateTime<Utc> = "2020-02-29T14:00:00+02:00".parse().unwrap();
        assert_eq!(datetime.timestamp(), 1582977600);
        let datetime: DateTime<Utc> = "2020-02-29T06:30:00.5-05:30".parse().unwrap();
        assert_eq!(datetime.timestamp(), 1582977600);

        assert!("2020-02-29T12:00:00.Z".parse::<DateTime::<Utc>>().is_err());
        assert!(
            "2020-02-29T12:00:00.1aZ"
                .parse::<DateTime::<Utc>>()
                .is_err()
        );
        assert!("2020-02-29T12:00:00+02".parse::<DateTime::<Utc>>().is_err());
        assert!(
            "2020-02-29T12:00:00+2:00"
                .parse::<DateTime::<Utc>>()
                .is_err()
        );
        assert!(
            "2020-02-29T12:00:00+24:00"
                .parse::<DateTime::<Utc>>()
                .is_err()
        );
    }

    #[test]
    fn test_display() {
        let datetime = DateTime::<Utc>::from_timestamp_secs(1551355200).unwrap();
        assert_eq!(datetime.to_string(), "2019-02-28T12:00:00Z");
        let datetime = DateTime::<Utc>::from_timestamp_secs(1582977600).unwrap();
        assert_eq!(datetime.to_string(), "2020-02-29T12:00:00Z");
        let datetime = DateTime::<Utc>::from_timestamp_secs(-1000000).unwrap();
        assert_eq!(datetime.to_string(), "1969-12-20T10:13:20Z");
    }

    #[test]
    fn test_add_duration() {
        let datetime = DateTime::<Utc>::from_timestamp_secs(1609459200).unwrap();
        let new_datetime = datetime + Duration::from_secs(1);
        assert_eq!(new_datetime.timestamp(), 1609459201);
    }

    #[test]
    fn test_sub_duration() {
        let datetime = DateTime::<Utc>::from_timestamp_secs(1609459200).unwrap();
        let new_datetime = datetime - Duration::from_secs(1);
        assert_eq!(new_datetime.timestamp(), 1609459199);
    }
}
