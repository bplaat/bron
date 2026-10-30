/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::fmt::{self, Display, Formatter};

cfg_select! {
    target_os = "macos" => {
        use crate::macos as imp;
        mod macos;
    }
    windows => {
        use crate::windows as imp;
        mod windows;
    }
    any(
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ) => {
        use crate::libsecret as imp;
        mod libsecret;
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}

/// A platform-specific error attached to an [`Error`].
type PlatformError = Box<dyn std::error::Error + Send + Sync>;

/// An error returned by the system credential store.
#[non_exhaustive]
#[derive(Debug)]
pub enum Error {
    /// The platform credential store failed.
    PlatformFailure(PlatformError),
    /// The requested credential does not exist.
    NoEntry,
    /// A parameter was invalid, with the parameter name and the reason.
    Invalid(String, String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlatformFailure(err) => write!(f, "Platform failure: {err}"),
            Self::NoEntry => write!(f, "No matching credential found"),
            Self::Invalid(attr, reason) => write!(f, "Value of '{attr}' is invalid: {reason}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PlatformFailure(err) => Some(err.as_ref()),
            _ => None,
        }
    }
}

fn platform_failure(message: impl Into<PlatformError>) -> Error {
    Error::PlatformFailure(message.into())
}

fn reject_nul(name: &str, value: &str) -> Result<()> {
    if value.contains('\0') {
        return Err(Error::Invalid(
            name.to_string(),
            "must not contain NUL bytes".to_string(),
        ));
    }
    Ok(())
}

/// A result returned by the system credential store.
pub type Result<T> = std::result::Result<T, Error>;

/// A secret identified by a service and account name.
pub struct Entry {
    service: String,
    account: String,
}

impl Entry {
    /// Creates an entry in the platform credential store.
    pub fn new(service: &str, account: &str) -> Result<Self> {
        reject_nul("service", service)?;
        reject_nul("username", account)?;
        Ok(Self {
            service: service.to_string(),
            account: account.to_string(),
        })
    }

    /// Stores or replaces this entry's secret.
    pub fn set_secret(&self, secret: &[u8]) -> Result<()> {
        imp::set_secret(&self.service, &self.account, secret)
    }

    /// Loads this entry's secret.
    pub fn get_secret(&self) -> Result<Vec<u8>> {
        imp::get_secret(&self.service, &self.account)
    }

    /// Deletes this entry's secret.
    pub fn delete_credential(&self) -> Result<()> {
        imp::delete_credential(&self.service, &self.account)
    }
}

#[cfg(test)]
mod tests {
    use zeroize::Zeroizing;

    use super::*;

    #[test]
    fn rejects_embedded_nul() {
        assert!(matches!(
            Entry::new("service\0suffix", "account"),
            Err(Error::Invalid(..))
        ));
        assert!(matches!(
            Entry::new("service", "account\0suffix"),
            Err(Error::Invalid(..))
        ));
    }

    #[test]
    #[ignore = "uses the user's native credential store"]
    fn native_store_round_trip() {
        let account = format!("test-{}", std::process::id());
        let entry = Entry::new("nl.bplaat.keyring.tests", &account).expect("entry creation failed");
        entry.set_secret(b"test\0secret\xff").expect("store failed");
        let secret = Zeroizing::new(entry.get_secret().expect("load failed"));
        entry.set_secret(b"").expect("store empty failed");
        let empty_secret = Zeroizing::new(entry.get_secret().expect("load empty failed"));
        entry.delete_credential().expect("delete failed");
        assert_eq!(secret.as_slice(), b"test\0secret\xff");
        assert!(empty_secret.is_empty());
    }
}
