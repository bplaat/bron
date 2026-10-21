/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::borrow::Cow;
use std::io::{self, Read};
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

// MARK: Body
/// Conversion into a response body. Static data such as embedded assets is borrowed without
/// copying, other bodies are owned.
pub trait IntoBody {
    /// Convert into the body bytes.
    fn into_body(self) -> Cow<'static, [u8]>;
}

impl IntoBody for Cow<'static, [u8]> {
    fn into_body(self) -> Cow<'static, [u8]> {
        self
    }
}

impl IntoBody for Vec<u8> {
    fn into_body(self) -> Cow<'static, [u8]> {
        Cow::Owned(self)
    }
}

impl IntoBody for String {
    fn into_body(self) -> Cow<'static, [u8]> {
        Cow::Owned(self.into_bytes())
    }
}

impl IntoBody for &str {
    fn into_body(self) -> Cow<'static, [u8]> {
        Cow::Owned(self.as_bytes().to_vec())
    }
}

impl IntoBody for &[u8] {
    fn into_body(self) -> Cow<'static, [u8]> {
        Cow::Owned(self.to_vec())
    }
}

impl<const N: usize> IntoBody for &[u8; N] {
    fn into_body(self) -> Cow<'static, [u8]> {
        Cow::Owned(self.to_vec())
    }
}

/// Append exactly `len` body bytes to `buffer`. The buffer grows with the bytes that arrive, so a
/// peer cannot make the server allocate a large declared length it never sends.
pub(crate) fn read_body(reader: &mut dyn Read, buffer: &mut Vec<u8>, len: usize) -> io::Result<()> {
    let read = reader.take(len as u64).read_to_end(buffer)?;
    if read == len {
        Ok(())
    } else {
        Err(io::ErrorKind::UnexpectedEof.into())
    }
}
