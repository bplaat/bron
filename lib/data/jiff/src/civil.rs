/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Wall-clock dates and times without a time zone

use core::fmt::{self, Debug, Display, Formatter};
use core::str::FromStr;

use crate::tz::TimeZone;
use crate::{Error, ZonedDateTime, parse_fields};

const SECS_IN_DAY: i64 = 24 * 60 * 60;

// MARK: Date
/// A date in the proleptic Gregorian calendar
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: i16,
    month: i8,
    day: i8,
}

/// Creates a [Date], panics when it's invalid
pub const fn date(year: i16, month: i8, day: i8) -> Date {
    match Date::constant(year, month, day) {
        Ok(date) => date,
        Err(_) => panic!("invalid date"),
    }
}

impl Date {
    /// Creates a date, years range from -9999 to 9999
    pub fn new(year: i16, month: i8, day: i8) -> Result<Self, Error> {
        Self::constant(year, month, day)
    }

    const fn constant(year: i16, month: i8, day: i8) -> Result<Self, Error> {
        if year < -9999 || year > 9999 || month < 1 || month > 12 || day < 1 {
            return Err(Error::INVALID_DATE);
        }
        let days_in_month = match month {
            2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if day > days_in_month {
            return Err(Error::INVALID_DATE);
        }
        Ok(Self { year, month, day })
    }

    /// Returns the year
    pub fn year(self) -> i16 {
        self.year
    }

    /// Returns the month, from 1 to 12
    pub fn month(self) -> i8 {
        self.month
    }

    /// Returns the day of the month, from 1
    pub fn day(self) -> i8 {
        self.day
    }

    /// Returns the date and time at this date, panics when the time is invalid
    pub const fn at(self, hour: i8, minute: i8, second: i8, subsec_nanosecond: i32) -> DateTime {
        match DateTime::from_date(self, hour, minute, second, subsec_nanosecond) {
            Ok(datetime) => datetime,
            Err(_) => panic!("invalid time"),
        }
    }

    /// Returns the start of this date in a time zone
    pub fn to_zoned_datetime(self, tz: TimeZone) -> Result<ZonedDateTime, Error> {
        self.at(0, 0, 0, 0).to_zoned_datetime(tz)
    }

    /// Returns the days since 1970-01-01, with the algorithm of
    /// <https://howardhinnant.github.io/date_algorithms.html#days_from_civil>
    const fn to_days(self) -> i64 {
        let (month, day) = (self.month as i64, self.day as i64);
        let year = self.year as i64 - (month <= 2) as i64;
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// Returns the date of days since 1970-01-01, with the algorithm of
    /// <https://howardhinnant.github.io/date_algorithms.html#civil_from_days>
    const fn from_days(days: i64) -> Self {
        let days = days + 719_468;
        let era = days.div_euclid(146_097);
        let day_of_era = days - era * 146_097;
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = day_of_year - (153 * month_index + 2) / 5 + 1;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        };
        let year = year_of_era + era * 400 + (month <= 2) as i64;
        Self {
            year: year as i16,
            month: month as i8,
            day: day as i8,
        }
    }
}

impl FromStr for Date {
    type Err = Error;

    /// Parses `YYYY-MM-DD`, or `±YYYYYY-MM-DD` for other years
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (year, month_day) = s
            .split_at_checked(s.len().wrapping_sub(6))
            .ok_or(Error::PARSE)?;
        let [month, day] = parse_fields(
            month_day.strip_prefix('-').ok_or(Error::PARSE)?,
            '-',
            [2, 2],
        )?;
        let year = match year.as_bytes().first() {
            Some(b'+') => parse_fields(&year[1..], '-', [6])?[0],
            // RFC 3339 doesn't allow a negative zero year
            Some(b'-') if &year[1..] != "000000" => -parse_fields(&year[1..], '-', [6])?[0],
            _ => parse_fields(year, '-', [4])?[0],
        };
        Self::new(year, month as i8, day as i8).map_err(|_| Error::PARSE)
    }
}

impl Display for Date {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if (0..=9999).contains(&self.year) {
            write!(f, "{:04}", self.year)?;
        } else {
            write!(f, "{:+07}", self.year)?;
        }
        write!(f, "-{:02}-{:02}", self.month, self.day)
    }
}

impl Debug for Date {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// MARK: DateTime
/// A date and time of day
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    date: Date,
    hour: i8,
    minute: i8,
    second: i8,
}

impl DateTime {
    /// Creates a date and time, the sub-second part is checked and dropped
    pub fn new(
        year: i16,
        month: i8,
        day: i8,
        hour: i8,
        minute: i8,
        second: i8,
        subsec_nanosecond: i32,
    ) -> Result<Self, Error> {
        Self::from_date(
            Date::new(year, month, day)?,
            hour,
            minute,
            second,
            subsec_nanosecond,
        )
    }

    const fn from_date(
        date: Date,
        hour: i8,
        minute: i8,
        second: i8,
        subsec_nanosecond: i32,
    ) -> Result<Self, Error> {
        if hour < 0
            || hour > 23
            || minute < 0
            || minute > 59
            || second < 0
            || second > 59
            || subsec_nanosecond < 0
            || subsec_nanosecond > 999_999_999
        {
            return Err(Error::INVALID_TIME);
        }
        Ok(Self {
            date,
            hour,
            minute,
            second,
        })
    }

    /// Returns the date
    pub fn date(self) -> Date {
        self.date
    }

    /// Returns the hour, from 0 to 23
    pub fn hour(self) -> i8 {
        self.hour
    }

    /// Returns the minute, from 0 to 59
    pub fn minute(self) -> i8 {
        self.minute
    }

    /// Returns the second, from 0 to 59
    pub fn second(self) -> i8 {
        self.second
    }

    /// Returns this date and time in a time zone, see [TimeZone] for how it resolves daylight
    /// saving time transitions
    pub fn to_zoned_datetime(self, tz: TimeZone) -> Result<ZonedDateTime, Error> {
        let timestamp = tz.to_timestamp(self)?;
        Ok(ZonedDateTime::new(timestamp, tz))
    }

    /// Returns the seconds since 1970-01-01T00:00:00
    pub(crate) const fn to_second(self) -> i64 {
        self.date.to_days() * SECS_IN_DAY
            + self.hour as i64 * 3600
            + self.minute as i64 * 60
            + self.second as i64
    }

    /// Returns the date and time of seconds since 1970-01-01T00:00:00
    pub(crate) const fn from_second(second: i64) -> Self {
        let day_second = second.rem_euclid(SECS_IN_DAY);
        Self {
            date: Date::from_days(second.div_euclid(SECS_IN_DAY)),
            hour: (day_second / 3600) as i8,
            minute: (day_second % 3600 / 60) as i8,
            second: (day_second % 60) as i8,
        }
    }
}

impl FromStr for DateTime {
    type Err = Error;

    /// Parses `YYYY-MM-DDTHH:MM:SS` with optional fractional seconds, which are dropped
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (date, time) = s.split_once('T').ok_or(Error::PARSE)?;
        let time = match time.split_once('.') {
            Some((time, fraction))
                if !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit()) =>
            {
                time
            }
            Some(_) => return Err(Error::PARSE),
            None => time,
        };
        let [hour, minute, second] = parse_fields(time, ':', [2, 2, 2])?;
        Self::from_date(date.parse()?, hour as i8, minute as i8, second as i8, 0)
            .map_err(|_| Error::PARSE)
    }
}

impl Display for DateTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}T{:02}:{:02}:{:02}",
            self.date, self.hour, self.minute, self.second
        )
    }
}

impl Debug for DateTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self, f)
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_date_new() {
        assert!(Date::new(2024, 2, 29).is_ok());
        assert!(Date::new(2000, 2, 29).is_ok());
        assert!(Date::new(1900, 2, 29).is_err());
        assert!(Date::new(2023, 2, 29).is_err());
        assert!(Date::new(2024, 4, 31).is_err());
        assert!(Date::new(2024, 13, 1).is_err());
        assert!(Date::new(2024, 1, 0).is_err());
        assert!(Date::new(10000, 1, 1).is_err());
        assert!(Date::new(-9999, 1, 1).is_ok());
    }

    #[test]
    fn test_days_round_trip() {
        assert_eq!(date(1970, 1, 1).to_days(), 0);
        assert_eq!(date(2000, 3, 1).to_days(), 11_017);
        assert_eq!(date(1969, 12, 31).to_days(), -1);
        for date in [
            date(-9999, 1, 1),
            date(-1, 2, 28),
            date(0, 2, 29),
            date(9999, 12, 31),
        ] {
            assert_eq!(Date::from_days(date.to_days()), date);
        }
        let mut days = date(1899, 12, 31).to_days();
        for year in 1900..2101 {
            for month in 1..=12 {
                let mut day = 1;
                while let Ok(date) = Date::new(year, month, day) {
                    days += 1;
                    assert_eq!(date.to_days(), days);
                    assert_eq!(Date::from_days(days), date);
                    day += 1;
                }
            }
        }
    }

    #[test]
    fn test_date_parse_and_display() {
        for s in ["2024-06-15", "0000-01-01", "-009999-01-01"] {
            assert_eq!(
                s.parse::<Date>()
                    .map(|date| date.to_string())
                    .ok()
                    .as_deref(),
                Some(s)
            );
        }
        assert_eq!(
            "+002024-06-15".parse::<Date>().ok(),
            Some(date(2024, 6, 15))
        );
        for s in [
            "2024-6-15",
            "2024-02-30",
            "+010000-01-01",
            "-000000-01-01",
            "20240615",
            "2024-06-15T",
            "",
        ] {
            assert!(s.parse::<Date>().is_err(), "{s}");
        }
    }

    #[test]
    fn test_datetime() {
        let datetime = DateTime::new(2024, 2, 29, 12, 30, 45, 0).unwrap();
        assert_eq!(datetime.to_string(), "2024-02-29T12:30:45");
        assert_eq!(datetime.to_second(), 1_709_209_845);
        assert_eq!(DateTime::from_second(1_709_209_845), datetime);
        assert_eq!(
            DateTime::from_second(-1),
            date(1969, 12, 31).at(23, 59, 59, 0)
        );
        assert_eq!("2024-02-29T12:30:45.123".parse().ok(), Some(datetime));
        assert!(DateTime::new(2024, 1, 1, 24, 0, 0, 0).is_err());
        assert!(DateTime::new(2024, 1, 1, 0, 60, 0, 0).is_err());
        assert!(DateTime::new(2024, 1, 1, 0, 0, 60, 0).is_err());
        assert!(DateTime::new(2024, 1, 1, 0, 0, 0, 1_000_000_000).is_err());
        for s in [
            "2024-02-29 12:30:45",
            "2024-02-29T12:30",
            "2024-02-29T12:30:45.",
            "2024-02-29T1:30:45",
        ] {
            assert!(s.parse::<DateTime>().is_err(), "{s}");
        }
    }
}
