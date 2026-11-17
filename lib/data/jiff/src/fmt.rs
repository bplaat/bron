/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Date and time formats

/// The RFC 2822 format of emails and its RFC 9110 form of HTTP dates
pub mod rfc2822 {
    use alloc::format;
    use alloc::string::String;

    use crate::civil::DateTime;
    use crate::{Error, Timestamp};

    const DAY_NAMES: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];

    /// Prints dates and times in RFC 2822 formats
    #[derive(Debug, Default)]
    pub struct DateTimePrinter(());

    impl DateTimePrinter {
        /// Creates a printer
        pub const fn new() -> Self {
            Self(())
        }

        /// Prints a timestamp as an RFC 9110 HTTP date, like `Sun, 06 Nov 1994 08:49:37 GMT`,
        /// fails for years before 0
        pub fn timestamp_to_rfc9110_string(&self, timestamp: &Timestamp) -> Result<String, Error> {
            let second = timestamp.as_second();
            let datetime = DateTime::from_second(second);
            let date = datetime.date();
            if date.year() < 0 {
                return Err(Error::OUT_OF_RANGE);
            }
            // 1970-01-01 was a Thursday
            let day_name = DAY_NAMES[second.div_euclid(24 * 60 * 60).rem_euclid(7) as usize];
            Ok(format!(
                "{day_name}, {:02} {} {:04} {:02}:{:02}:{:02} GMT",
                date.day(),
                MONTH_NAMES[date.month() as usize - 1],
                date.year(),
                datetime.hour(),
                datetime.minute(),
                datetime.second()
            ))
        }
    }

    #[cfg(test)]
    mod test {
        use super::*;

        #[test]
        fn test_timestamp_to_rfc9110_string() {
            let print = |second| {
                DateTimePrinter::new()
                    .timestamp_to_rfc9110_string(&Timestamp::from_second(second).unwrap())
            };
            assert_eq!(print(784_111_777).unwrap(), "Sun, 06 Nov 1994 08:49:37 GMT");
            assert_eq!(
                print(1_582_977_600).unwrap(),
                "Sat, 29 Feb 2020 12:00:00 GMT"
            );
            assert_eq!(print(-1_000_000).unwrap(), "Sat, 20 Dec 1969 10:13:20 GMT");
            assert!(print(Timestamp::MIN.as_second()).is_err());
        }
    }
}
