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
use crate::utils::{parse_hms, parse_offset, timestamp_to_ymd};
use crate::{NaiveDate, NaiveDateTime, ParseError, TimeZone};

// MARK: DateTime
/// A DateTime in UTC timezone
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DateTime<T: TimeZone>(i64, PhantomData<T>);

impl<T: TimeZone> DateTime<T> {
    /// Create a [DateTime] from a unix timestamp
    pub const fn from_timestamp_secs(secs: i64) -> Option<Self> {
        Some(Self(secs, PhantomData))
    }

    /// Get the NaiveDateTime
    pub const fn naive_utc(&self) -> NaiveDateTime {
        #[allow(deprecated)]
        NaiveDateTime::from_timestamp(self.0, 0).expect("Should be some")
    }

    /// Get the NaiveDateTime in the wall-clock time of the timezone
    pub fn naive_local(&self) -> NaiveDateTime {
        #[allow(deprecated)]
        NaiveDateTime::from_timestamp(self.local_secs().0, 0).expect("Should be some")
    }

    /// Get the same moment in time in another timezone
    pub const fn with_timezone<U: TimeZone>(&self, _tz: &U) -> DateTime<U> {
        DateTime(self.0, PhantomData)
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
        Self(self.0 + duration.as_secs() as i64, PhantomData)
    }
}

impl<T: TimeZone> Sub<Duration> for DateTime<T> {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self::Output {
        Self(self.0 - duration.as_secs() as i64, PhantomData)
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
        // Fractional seconds are dropped, timestamps have whole second precision
        let time_part = match time_part.split_once('.') {
            Some((time_part, fraction))
                if !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit()) =>
            {
                time_part
            }
            Some(_) => return Err(ParseError),
            None => time_part,
        };

        let (hour, minute, second) = parse_hms(time_part)?;
        let local = NaiveDate::from_str(date_part)?
            .and_hms_opt(hour, minute, second)
            .ok_or(ParseError)?;
        Self::from_timestamp_secs(local.and_utc().timestamp() - offset).ok_or(ParseError)
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
