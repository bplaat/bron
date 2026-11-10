/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use ::chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

use super::*;

impl_value_conversion!(
    NaiveDate,
    Date,
    "date or date text",
    |value| value,
    |value| Ok(value),
    |value: Text| value
        .parse::<NaiveDate>()
        .map_err(|_| ValueError::new(format!("invalid date: {value}"))),
);

impl_value_conversion!(
    DateTime<Utc>,
    DateTime,
    "datetime, Unix milliseconds or datetime text",
    |value| value,
    |value| Ok(value),
    |value: Integer| DateTime::<Utc>::from_timestamp_millis(value)
        .ok_or_else(|| ValueError::new(format!("invalid millisecond timestamp: {value}"))),
    |value: Text| value
        .parse::<NaiveDateTime>()
        .map(|value| value.and_utc())
        .map_err(|_| ValueError::new(format!("invalid datetime: {value}"))),
);

impl_value_conversion!(
    NaiveDateTime,
    DateTime,
    "datetime, Unix milliseconds or datetime text",
    |value| value.and_utc(),
    |value| Ok(value.naive_utc()),
    |value: Integer| DateTime::<Utc>::from_timestamp_millis(value)
        .map(|value| value.naive_utc())
        .ok_or_else(|| ValueError::new(format!("invalid millisecond timestamp: {value}"))),
    |value: Text| value
        .parse::<NaiveDateTime>()
        .map_err(|_| ValueError::new(format!("invalid datetime: {value}"))),
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_temporal_values_preserve_nanoseconds() {
        let datetime = DateTime::<Utc>::from_timestamp(1_700_000_000, 123_456_789).unwrap();
        let date = datetime.naive_utc().date();
        assert_eq!(Value::from(date), Value::Date(date));
        assert_eq!(Value::from(datetime), Value::DateTime(datetime));
        assert_eq!(NaiveDate::try_from(Value::from(date)).unwrap(), date);
        assert_eq!(
            DateTime::<Utc>::try_from(Value::from(datetime)).unwrap(),
            datetime
        );
        assert_eq!(
            NaiveDateTime::try_from(Value::from(datetime.naive_utc())).unwrap(),
            datetime.naive_utc()
        );
        assert_eq!(
            Option::<DateTime<Utc>>::try_from(Value::from(Some(datetime))).unwrap(),
            Some(datetime)
        );
        assert_eq!(Option::<NaiveDate>::try_from(Value::Null).unwrap(), None);
        assert_eq!(
            Option::<DateTime<Utc>>::try_from(Value::Null).unwrap(),
            None
        );
        assert_eq!(
            Option::<NaiveDateTime>::try_from(Value::Null).unwrap(),
            None
        );
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn sqlite_stores_milliseconds_and_date_text() {
        let database = crate::Connection::open_sqlite_memory().unwrap();
        database
            .execute_script("CREATE TABLE events (date TEXT, datetime INTEGER) STRICT")
            .unwrap();
        let date = NaiveDate::from_ymd_opt(1969, 12, 31).unwrap();
        let datetime = DateTime::<Utc>::from_timestamp(-1, 123_456_789).unwrap();
        database
            .execute("INSERT INTO events VALUES (?, ?)", (date, datetime))
            .unwrap();
        assert_eq!(
            database
                .query_some::<(String, i64)>("SELECT * FROM events", ())
                .unwrap(),
            ("1969-12-31".into(), -877)
        );
        let (read_date, read_datetime) = database
            .query_some::<(NaiveDate, DateTime<Utc>)>("SELECT * FROM events", ())
            .unwrap();
        assert_eq!(read_date, date);
        assert_eq!(
            read_datetime,
            DateTime::<Utc>::from_timestamp(-1, 123_000_000).unwrap()
        );
        let mut statement = database
            .query::<NaiveDateTime>("SELECT :datetime", ())
            .unwrap();
        statement
            .bind_named_value(":datetime", datetime.naive_utc())
            .unwrap();
        assert_eq!(
            statement.next().unwrap().unwrap(),
            read_datetime.naive_utc()
        );
    }

    #[test]
    fn sqlite_milliseconds_and_mysql_fractional_text() {
        let datetime = DateTime::<Utc>::from_timestamp(1_700_000_000, 123_456_000).unwrap();
        assert_eq!(
            DateTime::<Utc>::try_from(Value::Text("2023-11-14 22:13:20.123456".into())).unwrap(),
            datetime
        );
        assert_eq!(
            NaiveDate::try_from(Value::Text("2023-11-14".into())).unwrap(),
            datetime.naive_utc().date()
        );
        assert_eq!(
            DateTime::<Utc>::try_from(Value::Integer(-1)).unwrap(),
            DateTime::<Utc>::from_timestamp(-1, 999_000_000).unwrap()
        );
        assert_eq!(
            NaiveDateTime::try_from(Value::Integer(1_700_000_000_123))
                .unwrap()
                .nanosecond(),
            123_000_000
        );
        for text in [
            "0000-00-00 00:00:00",
            "2023-11-14 22:13:20.invalid",
            "2023-11-14 22:13:20.123junk",
        ] {
            assert!(DateTime::<Utc>::try_from(Value::Text(text.into())).is_err());
        }
        assert!(NaiveDate::try_from(Value::Integer(1)).is_err());
        assert!(NaiveDate::try_from(Value::Text("0000-00-00".into())).is_err());
        assert!(DateTime::<Utc>::try_from(Value::Float(1.0)).is_err());
    }
}
