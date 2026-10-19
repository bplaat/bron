/*
 * Copyright (c) 2024-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::collections::HashMap;
use std::error;
use std::fmt::{self, Display, Formatter};

#[cfg(feature = "derive")]
pub use validate_derive::Validate;

// MARK: Error
/// Validate result
pub type Result = std::result::Result<(), Error>;

/// Validate error
#[derive(Debug)]
pub struct Error {
    message: String,
}
impl Error {
    /// Create validate error
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Get error message
    pub fn message(&self) -> &str {
        &self.message
    }
}
impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Validate error: {}", self.message)
    }
}
impl error::Error for Error {}

// MARK: Report
/// Validation report
#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct Report(pub HashMap<String, Vec<String>>);

impl Report {
    /// Create new report
    pub fn new() -> Self {
        Self::default()
    }

    /// Is report empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get errors for field
    pub fn get_errors(&self, field: impl AsRef<str>) -> Option<&Vec<String>> {
        self.0.get(field.as_ref())
    }

    /// Insert error for field
    pub fn insert_error(&mut self, field: impl Into<String>, message: impl Into<String>) {
        self.0.entry(field.into()).or_default().push(message.into());
    }
}

// MARK: Validate
/// Validate trait
pub trait Validate {
    /// Validate context
    type Context;

    /// Validate self
    fn validate(&self) -> std::result::Result<(), Report>
    where
        Self::Context: Default,
    {
        let ctx = Self::Context::default();
        self.validate_with(&ctx)
    }

    /// Validate self with context
    fn validate_with(&self, context: &Self::Context) -> std::result::Result<(), Report>;
}

/// Validate email
#[cfg(feature = "email")]
pub fn is_valid_email(email: &str) -> bool {
    regex::regex!(r"^[a-zA-Z0-9.!#$%&’*+/=?^_`{|}~-]+@[a-zA-Z0-9-]+(?:\.[a-zA-Z0-9-]+)*$")
        .is_match(email)
}

/// Validate url
#[cfg(feature = "url")]
pub fn is_valid_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

// MARK: Tests
#[cfg(test)]
mod test {
    #[cfg(any(feature = "email", feature = "url"))]
    use super::*;

    #[test]
    #[cfg(feature = "email")]
    fn test_valid_email() {
        assert!(is_valid_email("test@example.com"));
        assert!(is_valid_email("user.name+tag+sorting@example.com"));
        assert!(is_valid_email("user_name@example.co.uk"));
        assert!(is_valid_email("user-name@example.org"));
    }

    #[test]
    #[cfg(feature = "email")]
    fn test_invalid_email() {
        assert!(!is_valid_email("plainaddress"));
        assert!(!is_valid_email("@missingusername.com"));
        assert!(!is_valid_email("username@.com"));
        assert!(!is_valid_email("username@.com."));
        assert!(!is_valid_email("username@example..com"));
    }

    #[test]
    #[cfg(feature = "url")]
    fn test_valid_url() {
        assert!(is_valid_url("http://example.com"));
        assert!(is_valid_url("https://example.com"));
        assert!(is_valid_url("http://www.example.com"));
        assert!(is_valid_url("https://www.example.com"));
        assert!(is_valid_url("http://example.com/path?name=value"));
    }

    #[test]
    #[cfg(feature = "url")]
    fn test_invalid_url() {
        assert!(!is_valid_url("example"));
        assert!(!is_valid_url("example.com"));
        assert!(!is_valid_url("http://"));
        assert!(!is_valid_url("foo http://example.com"));
        assert!(!is_valid_url("ftp://example.com"));
    }
}
