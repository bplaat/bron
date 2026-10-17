/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [chrono](https://crates.io/crates/chrono) crate
//!
//! Disable default features for `no_std` support with `alloc`. The `now` feature requires `std`.

#![cfg_attr(not(any(test, feature = "now")), no_std)]

extern crate alloc;

use core::error::Error;
use core::fmt::{self, Display, Formatter};

pub use crate::datetime::DateTime;
pub use crate::naive::date::NaiveDate;
pub use crate::naive::datetime::NaiveDateTime;
pub use crate::timezone::{TimeZone, Utc};

mod consts;
mod datetime;
mod naive;
mod timezone;
mod utils;

// MARK: ParseError
/// Parser error
#[derive(Debug)]
pub struct ParseError;

impl Display for ParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "URL parse error")
    }
}

impl Error for ParseError {}
