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
use crate::utils::timestamp_to_ymd;
use crate::{DateTime, NaiveDate, ParseError, Utc};

// MARK: NaiveDateTime
/// A DateTime without timezone
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NaiveDateTime(i64);

impl NaiveDateTime {
    /// Create a [NaiveDateTime] from a unix timestamp
    #[deprecated]
    #[allow(unused_variables)]
    pub const fn from_timestamp(secs: i64, nsecs: u32) -> Option<Self> {
        Some(Self(secs))
    }

    /// Get the [NaiveDate] of the date and time
    pub const fn date(&self) -> NaiveDate {
        NaiveDate::from_timestamp(self.0)
    }

    /// Get the [DateTime] in UTC timezone
    pub const fn and_utc(&self) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp_secs(self.0).expect("Should be some")
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
        #[allow(deprecated)]
        Self::from_timestamp(self.0 + duration.as_secs() as i64, 0).expect("Should be some")
    }
}

impl Sub<Duration> for NaiveDateTime {
    type Output = Self;

    fn sub(self, duration: Duration) -> Self::Output {
        #[allow(deprecated)]
        Self::from_timestamp(self.0 - duration.as_secs() as i64, 0).expect("Should be some")
    }
}

impl FromStr for NaiveDateTime {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut datetime_parts = s.split(' ');
        let date_part = datetime_parts.next().ok_or(ParseError)?;
        let time_part = datetime_parts.next().ok_or(ParseError)?;
        if datetime_parts.next().is_some() {
            return Err(ParseError);
        }

        let mut time_parts = time_part.split(':');
        let hour: u32 = time_parts
            .next()
            .ok_or(ParseError)?
            .parse()
            .map_err(|_| ParseError)?;
        let minute: u32 = time_parts
            .next()
            .ok_or(ParseError)?
            .parse()
            .map_err(|_| ParseError)?;
        let second: u32 = time_parts
            .next()
            .ok_or(ParseError)?
            .parse()
            .map_err(|_| ParseError)?;
        if time_parts.next().is_some() || hour >= 24 || minute >= 60 || second >= 60 {
            return Err(ParseError);
        }

        NaiveDate::from_str(date_part)?
            .and_hms_opt(hour, minute, second)
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
        )
    }
}

impl Debug for NaiveDateTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("NaiveDate(")?;
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
