/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

use crate::KEEP_ALIVE_TIMEOUT;
use crate::enums::{Status, Version};
use crate::header_map::HeaderMap;
use crate::request::Request;

// MARK: Response
/// HTTP response
#[derive(Default)]
pub struct Response {
    /// Status
    pub status: Status,
    /// Headers
    pub headers: HeaderMap,
    /// Body
    pub body: Vec<u8>,
    pub(crate) takeover: Option<Box<dyn FnOnce(BufReader<TcpStream>) + Send + 'static>>,
}

impl Response {
    /// Create new response
    pub fn new() -> Self {
        Self::default()
    }

    /// Create new response with status
    pub fn with_status(status: Status) -> Self {
        Self {
            status,
            ..Default::default()
        }
    }

    /// Set status
    pub const fn status(mut self, status: Status) -> Self {
        self.status = status;
        self
    }

    /// Create new response with header
    pub fn with_header(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self::default().header(name.into(), value.into())
    }

    /// Set header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Create new response with body
    pub fn with_body(body: impl Into<Vec<u8>>) -> Self {
        Self {
            body: body.into(),
            ..Default::default()
        }
    }

    /// Set body
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    /// Create new response with json body
    #[cfg(feature = "json")]
    pub fn with_json(value: impl serde::Serialize) -> Self {
        Self::default().json(value)
    }

    /// Set json body
    #[cfg(feature = "json")]
    pub fn json(mut self, value: impl serde::Serialize) -> Self {
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        self.body = serde_json::to_string(&value)
            .expect("Can't serialize json")
            .into();
        self
    }

    /// Create new response with redirect header
    pub fn with_redirect(location: impl Into<String>) -> Self {
        Self::default().redirect(location.into())
    }

    /// Set redirect header
    pub fn redirect(mut self, location: impl Into<String>) -> Self {
        self.status = Status::TemporaryRedirect;
        self.headers.insert("Location".to_string(), location.into());
        self
    }

    /// Set takeover function
    pub fn takeover(mut self, f: impl FnOnce(BufReader<TcpStream>) + Send + 'static) -> Self {
        self.takeover = Some(Box::new(f));
        self
    }

    /// Parse json out of body
    #[cfg(feature = "json")]
    pub fn into_json<T: serde::de::DeserializeOwned>(self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// Read one response from a stream.
    ///
    /// Uses a temporary buffer that may discard bytes past the response, so only use it when the
    /// connection carries nothing else. Use [`Self::read_from_buffered_stream`] otherwise.
    pub fn read_from_stream(stream: &mut dyn Read) -> Result<Self, InvalidResponseError> {
        let mut reader = BufReader::new(stream);
        Self::read_from_buffered_stream(&mut reader)
    }

    /// Read a response from a persistent buffered stream without discarding bytes read ahead for
    /// the next response or upgraded protocol message.
    pub fn read_from_buffered_stream(
        reader: &mut dyn BufRead,
    ) -> Result<Self, InvalidResponseError> {
        // Read first line
        let mut res = {
            let line = read_http_line(reader)?;
            let mut parts = line.splitn(3, ' ');
            let http_version = parts.next().ok_or(InvalidResponseError)?;
            if !matches!(http_version, "HTTP/1.0" | "HTTP/1.1") {
                return Err(InvalidResponseError);
            }
            let status_code = parts
                .next()
                .ok_or(InvalidResponseError)?
                .parse::<i32>()
                .map_err(|_| InvalidResponseError)?;
            Response::default()
                .status(Status::try_from(status_code).map_err(|_| InvalidResponseError)?)
        };

        // Read headers
        loop {
            let line = read_http_line(reader)?;
            if line == "\r\n" {
                break;
            }
            if res.headers.len() >= crate::MAX_HEADERS {
                return Err(InvalidResponseError);
            }
            let split = line.find(':').ok_or(InvalidResponseError)?;
            res.headers.append(
                line[0..split].trim().to_string(),
                line[split + 1..].trim().to_string(),
            );
        }

        if res.headers.get_all("Content-Length").count() > 1
            || res.headers.get_all("Transfer-Encoding").count() > 1
            || res.headers.contains_key("Transfer-Encoding")
                && res.headers.contains_key("Content-Length")
        {
            return Err(InvalidResponseError);
        }

        // Read body
        if let Some(transfer_encoding) = res.headers.get("Transfer-Encoding") {
            if !transfer_encoding.eq_ignore_ascii_case("chunked") {
                return Err(InvalidResponseError);
            }
            let mut body = Vec::new();
            loop {
                // Read chunk size line; strip optional chunk extensions (;...)
                let size_line = read_http_line(reader)?;
                let hex = size_line.split(';').next().unwrap_or("").trim();
                let size = usize::from_str_radix(hex, 16).map_err(|_| InvalidResponseError)?;
                if size == 0 {
                    read_trailers(reader, &mut res.headers)?;
                    break;
                }
                if body.len().saturating_add(size) > crate::MAX_RESPONSE_BODY {
                    return Err(InvalidResponseError);
                }

                // Read chunk data
                let prev_len = body.len();
                body.resize(prev_len + size, 0);
                reader
                    .read_exact(&mut body[prev_len..])
                    .map_err(|_| InvalidResponseError)?;

                // Read trailing CRLF after chunk data
                let mut crlf = [0; 2];
                reader
                    .read_exact(&mut crlf)
                    .map_err(|_| InvalidResponseError)?;
                if crlf != *b"\r\n" {
                    return Err(InvalidResponseError);
                }
            }
            res.body = body;
            return Ok(res);
        }
        if let Some(content_length) = res.headers.get("Content-Length") {
            let content_length = content_length.parse().map_err(|_| InvalidResponseError)?;
            if content_length > crate::MAX_RESPONSE_BODY {
                return Err(InvalidResponseError);
            }
            if content_length > 0 {
                res.body = vec![0; content_length];
                reader
                    .read_exact(&mut res.body)
                    .map_err(|_| InvalidResponseError)?;
            }
        }
        Ok(res)
    }

    pub(crate) fn write_to_stream(
        &mut self,
        stream: &mut dyn Write,
        req: &Request,
        keep_alive: bool,
    ) {
        self.finish_headers(req, keep_alive);

        _ = write!(stream, "{} {}\r\n", req.version, self.status);
        _ = self.headers.write_to(stream, "\r\n");
        _ = write!(stream, "\r\n");
        // HEAD responses must not include a message body
        if req.method != crate::enums::Method::Head {
            _ = stream.write_all(&self.body);
        }
    }

    #[cfg(feature = "cgi")]
    pub(crate) fn write_to_cgi_stdout(&self, stdout: &mut dyn Write) {
        _ = writeln!(stdout, "Status: {}", self.status);
        _ = self.headers.write_to(stdout, "\n");
        _ = writeln!(stdout);
        _ = stdout.write_all(&self.body);
    }

    fn finish_headers(&mut self, req: &Request, keep_alive: bool) {
        #[cfg(feature = "date")]
        self.headers
            .insert("Date".to_string(), chrono::Utc::now().to_rfc2822());
        self.headers
            .insert("Content-Length".to_string(), self.body.len().to_string());
        if req.version == Version::Http1_1 {
            if keep_alive && req.headers.get("Connection") != Some("close") {
                if self.headers.get("Connection").is_none() {
                    self.headers
                        .insert("Connection".to_string(), "keep-alive".to_string());
                    self.headers.insert(
                        "Keep-Alive".to_string(),
                        format!("timeout={}", KEEP_ALIVE_TIMEOUT.as_secs()),
                    );
                }
            } else if self.headers.get("Connection").is_none() {
                self.headers
                    .insert("Connection".to_string(), "close".to_string());
            }
        }
    }
}

fn read_trailers(
    reader: &mut dyn BufRead,
    headers: &mut HeaderMap,
) -> Result<(), InvalidResponseError> {
    loop {
        let line = read_http_line(reader)?;
        if line == "\r\n" {
            return Ok(());
        }
        if headers.len() >= crate::MAX_HEADERS {
            return Err(InvalidResponseError);
        }
        let split = line.find(':').ok_or(InvalidResponseError)?;
        let name = line[..split].trim();
        if name.eq_ignore_ascii_case("Content-Length")
            || name.eq_ignore_ascii_case("Transfer-Encoding")
        {
            return Err(InvalidResponseError);
        }
        headers.append(name.to_string(), line[split + 1..].trim().to_string());
    }
}

fn read_http_line(reader: &mut dyn BufRead) -> Result<String, InvalidResponseError> {
    let mut line = String::new();
    let bytes_read = reader
        .take(crate::MAX_HEADER_LINE + 1)
        .read_line(&mut line)
        .map_err(|_| InvalidResponseError)?;
    if bytes_read == 0 || bytes_read as u64 > crate::MAX_HEADER_LINE || !line.ends_with("\r\n") {
        return Err(InvalidResponseError);
    }
    Ok(line)
}

// MARK: InvalidResponseError
/// Invalid response error
#[derive(Debug)]
pub struct InvalidResponseError;

impl Display for InvalidResponseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid response")
    }
}

impl Error for InvalidResponseError {}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_parse_response() {
        let mut response_stream =
            "HTTP/1.1 200 OK\r\nContent-Length: 13\r\n\r\nHello, world!".as_bytes();
        let response = Response::read_from_stream(&mut response_stream).unwrap();

        assert_eq!(response.status, Status::Ok);
        assert_eq!(response.headers.get("Content-Length").unwrap(), "13");
        assert_eq!(response.body, b"Hello, world!");
    }

    #[test]
    fn test_parse_response_with_headers() {
        let mut response_stream =
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nX-Custom-Header: Value\r\n\r\n"
                .as_bytes();
        let response = Response::read_from_stream(&mut response_stream).unwrap();

        assert_eq!(response.status, Status::NotFound);
        assert_eq!(response.headers.get("Content-Length").unwrap(), "0");
        assert_eq!(response.headers.get("X-Custom-Header").unwrap(), "Value");
        assert!(response.body.is_empty());
    }

    #[test]
    fn test_parse_response_preserves_repeated_set_cookie_headers() {
        let mut response_stream = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nSet-Cookie: one=1\r\nset-cookie: two=2\r\n\r\n".as_bytes();

        let response = Response::read_from_stream(&mut response_stream).unwrap();

        assert_eq!(
            response.headers.get_all("SET-COOKIE").collect::<Vec<_>>(),
            ["one=1", "two=2"]
        );
    }

    #[test]
    fn test_parse_response_invalid() {
        let mut response_stream = "INVALID RESPONSE".as_bytes();
        let result = Response::read_from_stream(&mut response_stream);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_response_rejects_ambiguous_framing() {
        for input in [
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n",
        ] {
            assert!(Response::read_from_stream(&mut input.as_bytes()).is_err());
        }
    }

    #[test]
    fn test_parse_response_rejects_overlong_or_unterminated_lines() {
        let overlong = format!("HTTP/1.1 200 OK\r\nX-Test: {}\r\n\r\n", "a".repeat(8192));
        assert!(Response::read_from_stream(&mut overlong.as_bytes()).is_err());
        assert!(Response::read_from_stream(&mut b"HTTP/1.1 200 OK".as_slice()).is_err());
    }

    #[test]
    fn test_parse_response_rejects_framing_headers_in_trailers() {
        let mut input =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 0\r\n\r\n"
                .as_slice();
        assert!(Response::read_from_stream(&mut input).is_err());
    }

    #[test]
    fn test_write_to_stream_preserves_connection_header() {
        let request = Request::get("http://localhost/");
        let mut response =
            Response::with_status(Status::SwitchingProtocols).header("Connection", "Upgrade");
        let mut buffer = Vec::new();

        response.write_to_stream(&mut buffer, &request, false);

        let response = String::from_utf8(buffer).unwrap();
        assert!(response.contains("\r\nConnection: Upgrade\r\n"));
        assert!(!response.contains("\r\nConnection: close\r\n"));
    }

    #[test]
    fn test_parse_response_chunked_encoding() {
        let mut response_stream = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nBast\r\n4\r\niaan\r\n0\r\n\r\n".as_bytes();
        let response = Response::read_from_stream(&mut response_stream).unwrap();

        assert_eq!(response.status, Status::Ok);
        assert_eq!(
            response.headers.get("Transfer-Encoding").unwrap(),
            "chunked"
        );
        assert_eq!(response.body, b"Bastiaan");
    }

    #[test]
    fn test_parse_chunked_response_trailers_before_pipelined_response() {
        let input = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ntest\r\n0\r\nX-Checksum: valid\r\n\r\nHTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n";
        let mut reader = BufReader::new(&input[..]);

        let first = Response::read_from_buffered_stream(&mut reader).unwrap();
        assert_eq!(first.body, b"test");
        assert_eq!(first.headers.get("X-Checksum"), Some("valid"));

        let second = Response::read_from_buffered_stream(&mut reader).unwrap();
        assert_eq!(second.status, Status::NoContent);
    }

    #[test]
    fn test_parse_chunked_response_rejects_invalid_chunk_terminator() {
        let mut response_stream =
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ntestXX0\r\n\r\n".as_bytes();

        assert!(Response::read_from_stream(&mut response_stream).is_err());
    }

    #[test]
    #[cfg(feature = "json")]
    fn test_parse_response_with_json() {
        let mut response_stream = "HTTP/1.1 200 OK\r\nContent-Length: 15\r\nContent-Type: application/json\r\n\r\n{\"key\":\"value\"}".as_bytes();
        let response = Response::read_from_stream(&mut response_stream).unwrap();

        assert_eq!(response.status, Status::Ok);
        assert_eq!(
            response.headers.get("Content-Type").unwrap(),
            "application/json"
        );
        assert_eq!(response.body, b"{\"key\":\"value\"}");

        let json_value: serde_json::Value = response.into_json().unwrap();
        assert_eq!(json_value["key"], "value");
    }

    #[test]
    fn test_write_response() {
        let mut response = Response::with_status(Status::Ok)
            .header("Content-Length", "13")
            .body("Hello, world!");
        let mut response_stream = Vec::new();
        let request = Request {
            version: Version::Http1_1,
            ..Default::default()
        };
        response.write_to_stream(&mut response_stream, &request, true);

        let response_text = String::from_utf8(response_stream).unwrap();
        assert!(response_text.contains("HTTP/1.1 200 OK"));
        assert!(response_text.contains("Content-Length: 13"));
        assert!(response_text.contains("\r\n\r\nHello, world!"));
    }

    #[test]
    fn test_write_response_with_headers() {
        let mut response = Response::with_status(Status::NotFound)
            .header("Content-Length", "0")
            .header("X-Custom-Header", "Value");
        let mut response_stream = Vec::new();
        let request = Request {
            version: Version::Http1_1,
            ..Default::default()
        };
        response.write_to_stream(&mut response_stream, &request, true);

        let response_text = String::from_utf8(response_stream).unwrap();
        assert!(response_text.contains("HTTP/1.1 404 Not Found"));
        assert!(response_text.contains("Content-Length: 0"));
        assert!(response_text.contains("X-Custom-Header: Value"));
        assert!(response_text.contains("\r\n\r\n"));
    }

    #[test]
    fn test_write_response_has_one_automatic_content_length() {
        let mut response = Response::with_body("test").header("content-length", "999");
        let mut response_stream = Vec::new();
        let request = Request::default();

        response.write_to_stream(&mut response_stream, &request, false);

        let response_text = String::from_utf8(response_stream).unwrap();
        let content_lengths = response_text
            .lines()
            .filter(|line| line.to_ascii_lowercase().starts_with("content-length:"))
            .collect::<Vec<_>>();
        assert_eq!(content_lengths, ["Content-Length: 4"]);
    }

    #[test]
    #[cfg(feature = "json")]
    fn test_write_response_with_json() {
        let mut response = Response::with_json(serde_json::json!({"key": "value"}));
        let mut response_stream = Vec::new();
        let request = Request {
            version: Version::Http1_1,
            ..Default::default()
        };
        response.write_to_stream(&mut response_stream, &request, true);

        let response_text = String::from_utf8(response_stream).unwrap();
        assert!(response_text.contains("HTTP/1.1 200 OK"));
        assert!(response_text.contains("Content-Type: application/json"));
        assert!(response_text.contains("\r\n\r\n{\"key\":\"value\"}"));
    }
}
