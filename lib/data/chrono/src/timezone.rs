/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(feature = "now")]
use std::time::SystemTime;

/// A timezone
pub trait TimeZone {}

/// UTC timezone
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Utc;

impl TimeZone for Utc {}

impl Utc {
    /// Get the current [DateTime] in UTC timezone
    #[cfg(feature = "now")]
    pub fn now() -> crate::DateTime<Self> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("Time went backwards");
        crate::DateTime::<Self>::from_timestamp_secs(now.as_secs() as i64).expect("Should be some")
    }
}
