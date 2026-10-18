/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "now")]
use std::time::SystemTime;

use crate::{DateTime, NaiveDate, NaiveDateTime};

// MARK: LocalResult
/// The result of mapping a wall-clock time to a [DateTime]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalResult<T> {
    /// The time maps to one result
    Single(T),
    /// The time is invalid or does not exist
    None,
}

impl<T> LocalResult<T> {
    /// Get the result if there is one
    pub fn single(self) -> Option<T> {
        match self {
            Self::Single(value) => Some(value),
            Self::None => None,
        }
    }
}

// MARK: TimeZone
/// A timezone
pub trait TimeZone: Sized {
    /// Get the [DateTime] of a wall-clock date and time in this timezone
    #[allow(clippy::wrong_self_convention, reason = "matches the chrono API")]
    fn from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<DateTime<Self>>;

    /// Get the [DateTime] of a wall-clock year, month, day, hour, minute and second in this
    /// timezone
    fn with_ymd_and_hms(
        &self,
        year: i32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> LocalResult<DateTime<Self>> {
        let local = u32::try_from(year)
            .ok()
            .and_then(|year| NaiveDate::from_ymd_opt(year, month, day))
            .and_then(|date| date.and_hms_opt(hour, minute, second));
        match local {
            Some(local) => self.from_local_datetime(&local),
            None => LocalResult::None,
        }
    }
}

#[cfg(feature = "now")]
fn now<T: TimeZone>() -> DateTime<T> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("Time went backwards");
    DateTime::from_timestamp_secs(now.as_secs() as i64).expect("Should be some")
}

// MARK: Utc
/// UTC timezone
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Utc;

impl TimeZone for Utc {
    fn from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<DateTime<Self>> {
        LocalResult::Single(local.and_utc())
    }
}

impl Utc {
    /// Get the current [DateTime] in UTC timezone
    #[cfg(feature = "now")]
    pub fn now() -> DateTime<Self> {
        now()
    }
}

// MARK: Local
/// The system's local timezone, as configured for the C library
#[cfg(feature = "clock")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local;

#[cfg(feature = "clock")]
impl TimeZone for Local {
    /// Daylight saving time transitions resolve like C `mktime`.
    fn from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<DateTime<Self>> {
        match crate::sys::local_to_timestamp(local.and_utc().timestamp())
            .and_then(DateTime::from_timestamp_secs)
        {
            Some(datetime) => LocalResult::Single(datetime),
            None => LocalResult::None,
        }
    }
}

#[cfg(feature = "clock")]
impl Local {
    /// Get the current [DateTime] in the local timezone
    pub fn now() -> DateTime<Self> {
        now()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_with_ymd_and_hms() {
        let datetime = Utc.with_ymd_and_hms(2024, 2, 29, 12, 30, 45).single();
        assert_eq!(
            datetime.map(|datetime| datetime.timestamp()),
            Some(1_709_209_845)
        );
        assert_eq!(
            Utc.with_ymd_and_hms(2023, 2, 29, 0, 0, 0),
            LocalResult::None
        );
        assert_eq!(
            Utc.with_ymd_and_hms(2024, 1, 1, 24, 0, 0),
            LocalResult::None
        );
        assert_eq!(Utc.with_ymd_and_hms(-1, 1, 1, 0, 0, 0), LocalResult::None);
    }

    #[cfg(feature = "clock")]
    #[test]
    fn test_local_is_within_utc_offsets() {
        let utc = Utc.with_ymd_and_hms(2024, 7, 1, 12, 0, 0).single().unwrap();
        let local = Local
            .with_ymd_and_hms(2024, 7, 1, 12, 0, 0)
            .single()
            .unwrap();
        // Time zones range from UTC-12 to UTC+14.
        assert!((-14 * 3600..=12 * 3600).contains(&(local.timestamp() - utc.timestamp())));
        assert!((Local::now().timestamp() - Utc::now().timestamp()).abs() <= 1);
    }
}
