/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::Timestamp;
use crate::civil::{Date, DateTime};
use crate::tz::TimeZone;

// MARK: ZonedDateTime
/// A timestamp with its wall-clock date and time in a time zone
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonedDateTime {
    timestamp: Timestamp,
    datetime: DateTime,
    time_zone: TimeZone,
}

impl ZonedDateTime {
    /// Creates a zoned date and time of a timestamp in a time zone
    pub fn new(timestamp: Timestamp, time_zone: TimeZone) -> Self {
        let offset = time_zone.to_offset_second(timestamp);
        Self {
            timestamp,
            datetime: DateTime::from_second(timestamp.as_second() + offset),
            time_zone,
        }
    }

    /// Returns the timestamp
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the wall-clock date and time
    pub fn datetime(&self) -> DateTime {
        self.datetime
    }

    /// Returns the wall-clock date
    pub fn date(&self) -> Date {
        self.datetime.date()
    }

    /// Returns the time zone
    pub fn time_zone(&self) -> &TimeZone {
        &self.time_zone
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;
    use crate::civil::date;

    #[test]
    fn test_utc() {
        let timestamp = Timestamp::from_second(1_709_209_845).unwrap();
        let zoned = timestamp.to_zoned_datetime(TimeZone::UTC);
        assert_eq!(zoned.timestamp(), timestamp);
        assert_eq!(zoned.datetime(), date(2024, 2, 29).at(12, 30, 45, 0));
        assert_eq!(zoned.date(), date(2024, 2, 29));
        assert_eq!(
            date(2024, 2, 29)
                .to_zoned_datetime(TimeZone::UTC)
                .unwrap()
                .timestamp()
                .as_second(),
            1_709_164_800
        );
    }

    #[cfg(feature = "tz-system")]
    #[test]
    fn test_system_round_trips_wall_clock() {
        let datetime = date(2024, 7, 1).at(12, 0, 0, 0);
        let zoned = datetime.to_zoned_datetime(TimeZone::system()).unwrap();
        assert_eq!(zoned.datetime(), datetime);
        assert_eq!(
            zoned.timestamp().to_zoned_datetime(TimeZone::system()),
            zoned
        );
    }
}
