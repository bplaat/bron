/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::io;

cfg_select! {
    any(target_os = "macos", target_os = "ios") => {
        mod apple;
        use apple::Store;
    }
    target_os = "android" => {
        mod android;
        use android::Store;
    }
    any(target_os = "linux", target_os = "windows") => {
        use file::Store;
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}

#[cfg(any(test, target_os = "linux", target_os = "windows"))]
mod file;

/// A native preference value. Floats use Android's native 32-bit precision.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A boolean.
    Bool(bool),
    /// A signed 64-bit integer.
    Integer(i64),
    /// A finite 32-bit floating-point number.
    Float(f32),
    /// A UTF-8 string.
    String(String),
}

macro_rules! value_from {
    ($ty:ty, $variant:ident) => {
        impl From<$ty> for Value {
            fn from(value: $ty) -> Self {
                Self::$variant(value)
            }
        }
    };
}

value_from!(bool, Bool);
value_from!(i64, Integer);
value_from!(f32, Float);
value_from!(String, String);

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

/// A named preference store backed by the platform's native storage.
pub struct Preferences {
    store: Store,
}

impl Preferences {
    /// Opens a store identified by a reverse-DNS application ID.
    ///
    /// Uses SharedPreferences on Android, an NSUserDefaults suite on Apple, or an
    /// application config directory on Windows and Linux. IDs may contain ASCII
    /// letters, digits, '.', '_' and '-'. Android needs a registered Java VM.
    pub fn new(application_id: &str) -> io::Result<Self> {
        validate_id(application_id)?;
        Ok(Self {
            store: Store::new(application_id)?,
        })
    }

    /// Reads a value, returning `None` when the key does not exist.
    ///
    /// Unsupported values written by other libraries return an error.
    pub fn get(&self, key: &str) -> io::Result<Option<Value>> {
        self.store.get(key)
    }

    /// Stores or replaces a value. Non-finite floats are rejected.
    ///
    /// Android commits synchronously; Apple schedules persistence through
    /// NSUserDefaults; desktop JSON writes replace the file atomically.
    pub fn set(&self, key: &str, value: impl Into<Value>) -> io::Result<()> {
        let value = value.into();
        if matches!(value, Value::Float(number) if !number.is_finite()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "float must be finite",
            ));
        }
        self.store.set(key, value)
    }

    /// Removes a key. A missing key is a successful no-op.
    pub fn remove(&self, key: &str) -> io::Result<()> {
        self.store.remove(key)
    }
}

fn validate_id(id: &str) -> io::Result<()> {
    if id.is_empty()
        || id == "."
        || id == ".."
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid application ID",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_ids() {
        for id in ["", ".", "..", "../other", "/tmp/app", "a\\b", "a\0b"] {
            assert!(validate_id(id).is_err(), "accepted {id:?}");
        }
        assert!(validate_id("nl.bplaat.test-app_1").is_ok());
    }
}
