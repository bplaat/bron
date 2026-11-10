/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// A database value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A NULL value
    Null,
    /// A signed 64-bit integer value.
    Integer(i64),
    /// A 64-bit floating point value
    Float(f64),
    /// A text value
    Text(String),
    /// A blob value
    Blob(Vec<u8>),
    /// A calendar date, bound as a native MySQL DATE or SQLite date text.
    #[cfg(feature = "chrono")]
    Date(::chrono::NaiveDate),
    /// A UTC datetime, bound as native MySQL DATETIME or SQLite Unix milliseconds.
    #[cfg(feature = "chrono")]
    DateTime(::chrono::DateTime<::chrono::Utc>),
}

// MARK: ValueError
type Result<T> = std::result::Result<T, ValueError>;

/// A value error
#[derive(Debug)]
pub struct ValueError {
    msg: String,
}

impl ValueError {
    #[doc(hidden)]
    pub fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into() }
    }
}

impl Display for ValueError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Value error: {}", self.msg)
    }
}

impl Error for ValueError {}

// MARK: Value conversions
macro_rules! impl_value_conversion {
    (
        $type:ty,
        $variant:ident,
        $expected:literal,
        |$input:ident| $encode:expr,
        |$stored:ident| $decode:expr
        $(, |$other:ident: $other_variant:ident| $other_decode:expr)* $(,)?
    ) => {
        impl From<$type> for Value {
            fn from($input: $type) -> Self {
                Value::$variant($encode)
            }
        }

        impl TryFrom<Value> for $type {
            type Error = ValueError;

            fn try_from(value: Value) -> Result<Self> {
                match value {
                    Value::$variant($stored) => $decode,
                    $(Value::$other_variant($other) => $other_decode,)*
                    _ => Err(ValueError::new(concat!("expected ", $expected))),
                }
            }
        }

        impl TryFrom<Value> for Option<$type> {
            type Error = ValueError;

            fn try_from(value: Value) -> Result<Self> {
                match value {
                    Value::$variant($stored) => ($decode).map(Some),
                    $(Value::$other_variant($other) => ($other_decode).map(Some),)*
                    Value::Null => Ok(None),
                    _ => Err(ValueError::new(concat!("expected ", $expected, " or null"))),
                }
            }
        }
    };
}

impl<T> From<Option<T>> for Value
where
    T: Into<Value>,
{
    fn from(value: Option<T>) -> Self {
        value.map_or(Value::Null, Into::into)
    }
}

impl_value_conversion!(
    bool,
    Integer,
    "integer",
    |value| if value { 1 } else { 0 },
    |value| Ok(value != 0),
);

macro_rules! impl_signed_value_conversions {
    ($($type:ty),+ $(,)?) => {
        $(
            impl_value_conversion!(
                $type,
                Integer,
                "integer",
                |value| i64::from(value),
                |value| <$type>::try_from(value)
                    .map_err(|_| ValueError::new("integer is out of range")),
            );
        )+
    };
}

impl_signed_value_conversions!(i8, i16, i32);
impl_value_conversion!(i64, Integer, "integer", |value| value, |value| Ok(value));
impl_value_conversion!(f64, Float, "float", |value| value, |value| Ok(value));
impl_value_conversion!(String, Text, "text", |value| value, |value| Ok(value));
impl_value_conversion!(Vec<u8>, Blob, "blob", |value| value, |value| Ok(value));

// MARK: Uuid
#[cfg(feature = "uuid")]
mod uuid_impls {
    use uuid::Uuid;

    use super::*;

    impl_value_conversion!(
        Uuid,
        Blob,
        "blob",
        |value| value.into_bytes().to_vec(),
        |value| Uuid::from_slice(&value).map_err(|error| ValueError::new(error.to_string())),
    );
}

#[cfg(feature = "chrono")]
mod chrono;

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_roundtrips_for_all_supported_widths() {
        assert_eq!(i8::try_from(Value::from(-8_i8)).unwrap(), -8);
        assert_eq!(i16::try_from(Value::from(-16_i16)).unwrap(), -16);
        assert_eq!(i32::try_from(Value::from(-32_i32)).unwrap(), -32);
        assert_eq!(i64::try_from(Value::from(-64_i64)).unwrap(), -64);

        assert_eq!(
            Option::<i8>::try_from(Value::from(Some(-8_i8))).unwrap(),
            Some(-8)
        );
        assert_eq!(
            Option::<i16>::try_from(Value::from(Some(-16_i16))).unwrap(),
            Some(-16)
        );
        assert_eq!(
            Option::<i32>::try_from(Value::from(Some(-32_i32))).unwrap(),
            Some(-32)
        );
        assert_eq!(
            Option::<i64>::try_from(Value::from(Some(-64_i64))).unwrap(),
            Some(-64)
        );
        assert_eq!(Option::<i64>::try_from(Value::Null).unwrap(), None);

        assert!(i8::try_from(Value::Integer(128)).is_err());
        assert!(Option::<i16>::try_from(Value::Integer(65_536)).is_err());
    }

    #[test]
    fn test_bool_float_text_and_blob_roundtrips() {
        assert!(bool::try_from(Value::from(true)).unwrap());
        assert!(
            !Option::<bool>::try_from(Value::from(Some(false)))
                .unwrap()
                .unwrap()
        );
        assert_eq!(Option::<bool>::try_from(Value::Null).unwrap(), None);

        assert_eq!(f64::try_from(Value::from(1.5_f64)).unwrap(), 1.5);
        assert_eq!(
            Option::<f64>::try_from(Value::from(Some(2.5_f64))).unwrap(),
            Some(2.5)
        );
        assert_eq!(Option::<f64>::try_from(Value::Null).unwrap(), None);

        assert_eq!(
            String::try_from(Value::from("hello".to_string())).unwrap(),
            "hello"
        );
        assert_eq!(
            Option::<String>::try_from(Value::from(Some("world".to_string()))).unwrap(),
            Some("world".to_string())
        );
        assert_eq!(Option::<String>::try_from(Value::Null).unwrap(), None);

        assert_eq!(
            Vec::<u8>::try_from(Value::from(vec![1_u8, 2_u8, 3_u8])).unwrap(),
            vec![1, 2, 3]
        );
        assert_eq!(
            Option::<Vec<u8>>::try_from(Value::from(Some(vec![4_u8, 5_u8]))).unwrap(),
            Some(vec![4, 5])
        );
        assert_eq!(Option::<Vec<u8>>::try_from(Value::Null).unwrap(), None);
    }

    #[test]
    fn test_value_type_mismatch_errors() {
        assert_eq!(
            bool::try_from(Value::Text("true".to_string()))
                .unwrap_err()
                .to_string(),
            "Value error: expected integer"
        );
        assert_eq!(
            Option::<String>::try_from(Value::Integer(1))
                .unwrap_err()
                .to_string(),
            "Value error: expected text or null"
        );
        assert_eq!(
            Vec::<u8>::try_from(Value::Integer(1))
                .unwrap_err()
                .to_string(),
            "Value error: expected blob"
        );
    }

    #[cfg(feature = "uuid")]
    #[test]
    fn test_uuid_value_roundtrips_and_errors() {
        use uuid::Uuid;

        let uuid = Uuid::from_bytes([
            0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4,
            0x30, 0xc8,
        ]);

        assert_eq!(Uuid::try_from(Value::from(uuid)).unwrap(), uuid);
        assert_eq!(
            Option::<Uuid>::try_from(Value::from(Some(uuid))).unwrap(),
            Some(uuid)
        );
        assert_eq!(Option::<Uuid>::try_from(Value::Null).unwrap(), None);
        assert!(Uuid::try_from(Value::Blob(vec![1_u8, 2_u8])).is_err());
    }
}
