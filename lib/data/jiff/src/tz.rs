/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Time zones

use crate::civil::DateTime;
use crate::{Error, Timestamp};

// MARK: TimeZone
/// A time zone, UTC or the system's
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeZone(Kind);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Utc,
    #[cfg(feature = "tz-system")]
    System,
}

#[cfg_attr(
    not(feature = "tz-system"),
    allow(
        unused_variables,
        reason = "only the system time zone needs the timestamp"
    )
)]
impl TimeZone {
    /// The UTC time zone
    pub const UTC: Self = Self(Kind::Utc);

    /// The system's time zone, which converts with the C library on Unix and Win32 on Windows.
    /// Wall-clock times in daylight saving time transitions resolve as the system does.
    #[cfg(feature = "tz-system")]
    pub fn system() -> Self {
        Self(Kind::System)
    }

    /// Returns the offset from UTC in seconds at a timestamp, the system's time zone falls back
    /// to UTC when the system can't convert it
    pub(crate) fn to_offset_second(&self, timestamp: Timestamp) -> i64 {
        match self.0 {
            Kind::Utc => 0,
            #[cfg(feature = "tz-system")]
            Kind::System => crate::sys::utc_offset(timestamp.as_second()).unwrap_or(0),
        }
    }

    /// Returns the timestamp of a wall-clock date and time in this time zone
    pub(crate) fn to_timestamp(&self, datetime: DateTime) -> Result<Timestamp, Error> {
        let second = match self.0 {
            Kind::Utc => datetime.to_second(),
            #[cfg(feature = "tz-system")]
            Kind::System => crate::sys::local_to_timestamp(datetime).ok_or(Error::TIME_ZONE)?,
        };
        Timestamp::from_second(second)
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;
    use crate::civil::date;

    #[test]
    fn test_utc() {
        let datetime = date(2024, 2, 29).at(12, 30, 45, 0);
        let timestamp = TimeZone::UTC.to_timestamp(datetime).unwrap();
        assert_eq!(timestamp.as_second(), 1_709_209_845);
        assert_eq!(TimeZone::UTC.to_offset_second(timestamp), 0);
        assert!(
            TimeZone::UTC
                .to_timestamp(date(9999, 12, 31).at(0, 0, 0, 0))
                .is_err()
        );
    }

    #[cfg(feature = "tz-system")]
    #[test]
    fn test_system() {
        let datetime = date(2024, 7, 1).at(12, 0, 0, 0);
        let local = TimeZone::system().to_timestamp(datetime).unwrap();
        let offset = TimeZone::system().to_offset_second(local);
        // Time zones range from UTC-12 to UTC+14
        assert!((-12 * 3600..=14 * 3600).contains(&offset));
        assert_eq!(local.as_second() + offset, datetime.to_second());
    }
}
