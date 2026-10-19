/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::time::Duration;

#[cfg(feature = "multi-threaded")]
pub use threadpool::Options as ThreadPoolOptions;

pub use crate::client::Client;
pub use crate::enums::{Method, Status};
pub use crate::header_map::HeaderMap;
pub use crate::request::Request;
pub use crate::response::Response;
#[cfg(feature = "cgi")]
pub use crate::serve::serve_cgi;
pub use crate::serve::serve_single_threaded;
#[cfg(feature = "multi-threaded")]
pub use crate::serve::{serve, serve_with_options};

mod client;
mod enums;
mod header_map;
mod request;
mod response;
mod serve;

// MARK: Constants
pub(crate) const KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const IO_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const MAX_REDIRECTS: usize = 5;
pub(crate) const MAX_REQUEST_BODY: usize = 64 * 1024 * 1024;
pub(crate) const MAX_RESPONSE_BODY: usize = 64 * 1024 * 1024;
pub(crate) const MAX_HEADERS: usize = 100;
pub(crate) const MAX_HEADER_LINE: u64 = 8 * 1024;
