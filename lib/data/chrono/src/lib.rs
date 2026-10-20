/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [chrono](https://crates.io/crates/chrono) crate
//!
//! Disable default features for `no_std` support with `alloc`. The `now` feature requires `std`,
//! and the `clock` feature adds the [Local] timezone through the C library.

#![cfg_attr(not(any(test, feature = "now")), no_std)]

extern crate alloc;

use core::error::Error;
use core::fmt::{self, Display, Formatter};

pub use crate::datetime::DateTime;
pub use crate::naive::date::NaiveDate;
pub use crate::naive::datetime::NaiveDateTime;
#[cfg(feature = "clock")]
pub use crate::timezone::Local;
pub use crate::timezone::{LocalResult, TimeZone, Utc};

mod consts;
mod datetime;
mod naive;
#[cfg(feature = "clock")]
mod sys;
mod timezone;
mod utils;

// MARK: ParseError
/// Parser error
#[derive(Debug)]
pub struct ParseError;

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Date and time parse error")
    }
}

impl Error for ParseError {}
