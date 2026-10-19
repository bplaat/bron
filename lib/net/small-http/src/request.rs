/*
 * Copyright (c) 2023-2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, BufRead, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::{self, FromStr};

use url::Url;

use crate::client::MaybeHttpsStream;
use crate::enums::{Method, Status, Version};
use crate::header_map::HeaderMap;
use crate::response::Response;
use crate::{KEEP_ALIVE_TIMEOUT, MAX_REDIRECTS};

// MARK: Request
/// HTTP request
#[derive(Clone)]
pub struct Request {
    /// HTTP version
    pub version: Version,
    /// URL
    pub url: Url,
    /// Method
    pub method: Method,
    /// Headers
    pub headers: HeaderMap,
    /// Parameters (mostly added for small-router)
    pub params: HashMap<String, String>,
    /// Body
    pub body: Option<Vec<u8>>,
    /// Client address
    pub client_addr: SocketAddr,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            version: Version::Http1_1,
            url: Url::from_str("http://localhost").expect("Should parse"),
            method: Method::Get,
            headers: HeaderMap::new(),
            params: HashMap::new(),
            body: None,
            client_addr: (Ipv4Addr::LOCALHOST, 0).into(),
        }
    }
}

impl Request {
    /// Create new request
    pub fn new() -> Self {
        Self::default()
    }

    /// Create new request with method
    pub fn with_method(method: Method) -> Self {
        Self {
            method,
            ..Self::default()
        }
    }

    /// Create new request with URL
    pub fn with_url(url: impl AsRef<str>) -> Self {
        Self {
            url: url.as_ref().parse().expect("Invalid url"),
            ..Self::default()
        }
    }

    /// Create new request with specific method and URL
    pub fn with_method_and_url(method: Method, url: impl AsRef<str>) -> Self {
        Self {
            method,
            url: url.as_ref().parse().expect("Invalid url"),
            ..Self::default()
        }
    }

    /// Create new GET request with URL
    pub fn get(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Get, url)
    }

    /// Create new HEAD request with URL
    pub fn head(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Head, url)
    }

    /// Create new POST request with URL
    pub fn post(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Post, url)
    }

    /// Create new PUT request with URL
    pub fn put(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Put, url)
    }

    /// Create new DELETE request with URL
    pub fn delete(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Delete, url)
    }

    /// Create new CONNECT request with URL
    pub fn connect(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Connect, url)
    }

    /// Create new OPTIONS request with URL
    pub fn options(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Options, url)
    }

    /// Create new TRACE request with URL
    pub fn trace(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Trace, url)
    }

    /// Create new PATCH request with URL
    pub fn patch(url: impl AsRef<str>) -> Self {
        Self::with_method_and_url(Method::Patch, url)
    }

    /// Set URL
    pub fn url(mut self, url: Url) -> Self {
        self.url = url;
        self
    }

    /// Set method
    pub const fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    /// Set header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Set body
    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set JSON body
    #[cfg(feature = "json")]
    pub fn json(mut self, value: impl serde::Serialize) -> Self {
        self.headers
            .insert("Content-Type".to_string(), "application/json".to_string());
        self.body = Some(
            serde_json::to_string(&value)
                .expect("Can't serialize json")
                .into(),
        );
        self
    }

    /// Set URL-encoded form body from a slice of key-value pairs.
    #[cfg(feature = "form")]
    pub fn urlencoded(mut self, data: &[(&str, &str)]) -> Self {
        self.headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        self.body = Some(
            serde_urlencoded::to_string(data)
                .expect("Can't serialize urlencoded")
                .into_bytes(),
        );
        self
    }

    /// Parse the request body based on the Content-Type header.
    ///
    /// Supports:
    /// - `application/json` (requires `json` feature)
    /// - `application/x-www-form-urlencoded` (requires `form` feature)
    ///
    /// Returns `Status::UnsupportedMediaType` if the Content-Type is missing or unsupported.
    /// Returns `Status::BadRequest` if the body cannot be deserialized.
    #[cfg(any(feature = "json", feature = "form"))]
    pub fn parse_body<T: serde::de::DeserializeOwned>(&self) -> Result<T, Status> {
        let content_type = self
            .headers
            .get("Content-Type")
            .unwrap_or_default()
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase();

        let body = self.body.as_deref().unwrap_or(&[]);

        match content_type.as_str() {
            #[cfg(feature = "json")]
            "application/json" => serde_json::from_slice(body).map_err(|_| Status::BadRequest),
            #[cfg(feature = "form")]
            "application/x-www-form-urlencoded" => {
                serde_urlencoded::from_bytes(body).map_err(|_| Status::BadRequest)
            }
            _ => Err(Status::UnsupportedMediaType),
        }
    }
}

impl Request {
    pub(crate) fn read_from_reader(
        reader: &mut dyn BufRead,
        client_addr: SocketAddr,
    ) -> Result<Request, InvalidRequestError> {
        // Read first line
        let (method, path, version) = {
            let line = read_http_line(reader, "first line")?;
            let parts = line.trim_end_matches("\r\n").split(' ').collect::<Vec<_>>();
            let [method, path, version] = parts.as_slice() else {
                return Err(InvalidRequestError("Invalid request line".to_string()));
            };
            (
                method
                    .parse()
                    .map_err(|_| InvalidRequestError("Can't parse method".to_string()))?,
                (*path).to_string(),
                version
                    .parse()
                    .map_err(|_| InvalidRequestError("Can't parse HTTP version".to_string()))?,
            )
        };

        // Read headers
        let mut headers = HeaderMap::new();
        loop {
            let line = read_http_line(reader, "header line")?;
            if line == "\r\n" {
                break;
            }
            if headers.len() >= crate::MAX_HEADERS {
                return Err(InvalidRequestError("Too many headers".to_string()));
            }
            let split = line
                .find(':')
                .ok_or(InvalidRequestError("Can't parse header line".to_string()))?;
            headers.append(
                line[0..split].trim().to_string(),
                line[split + 1..].trim().to_string(),
            );
        }

        if headers.get_all("Content-Length").count() > 1
            || headers.get_all("Transfer-Encoding").count() > 1
            || headers.get_all("Host").count() > 1
        {
            return Err(InvalidRequestError("Duplicate framing header".to_string()));
        }
        if headers.contains_key("Transfer-Encoding") && headers.contains_key("Content-Length") {
            return Err(InvalidRequestError(
                "Transfer-Encoding conflicts with Content-Length".to_string(),
            ));
        }

        // Read body
        let mut body = None;
        let transfer_encoding = headers.get("Transfer-Encoding").map(|s| s.to_lowercase());
        if transfer_encoding.as_deref() == Some("chunked") {
            let mut chunks: Vec<u8> = Vec::new();
            loop {
                let size_line = read_http_line(reader, "chunk size")?;
                // Strip optional chunk extensions (;...) and whitespace
                let hex = size_line.split(';').next().unwrap_or("").trim();
                let chunk_size = usize::from_str_radix(hex, 16)
                    .map_err(|_| InvalidRequestError("Can't parse chunk size".to_string()))?;
                if chunk_size == 0 {
                    read_trailers(reader, &mut headers)?;
                    break;
                }
                if chunks.len().saturating_add(chunk_size) > crate::MAX_REQUEST_BODY {
                    return Err(InvalidRequestError("Chunked body too large".to_string()));
                }
                let prev_len = chunks.len();
                chunks.resize(prev_len + chunk_size, 0);
                reader
                    .read_exact(&mut chunks[prev_len..])
                    .map_err(|_| InvalidRequestError("Can't read chunk data".to_string()))?;
                // Consume trailing CRLF after chunk data
                let mut crlf = [0u8; 2];
                reader.read_exact(&mut crlf).map_err(|_| {
                    InvalidRequestError("Can't read chunk trailing CRLF".to_string())
                })?;
                if crlf != *b"\r\n" {
                    return Err(InvalidRequestError(
                        "Invalid chunk trailing CRLF".to_string(),
                    ));
                }
            }
            body = Some(chunks);
        } else if transfer_encoding.is_some() {
            return Err(InvalidRequestError(
                "Unsupported Transfer-Encoding".to_string(),
            ));
        } else if let Some(content_length) = headers.get("Content-Length") {
            let content_length = content_length
                .parse()
                .map_err(|_| InvalidRequestError("Can't parse Content-Length".to_string()))?;
            if content_length > crate::MAX_REQUEST_BODY {
                return Err(InvalidRequestError("Content-Length too large".to_string()));
            }
            if content_length > 0 {
                let mut buffer = vec![0; content_length];
                reader.read_exact(&mut buffer).map_err(|_| {
                    InvalidRequestError(
                        "Can't read Content-Length amount of bytes from stream".to_string(),
                    )
                })?;
                body = Some(buffer);
            }
        }

        // Parse URL
        let url = Url::from_str(&if version == Version::Http1_1 {
            format!(
                "http://{}{}",
                headers.get("Host").ok_or(InvalidRequestError(
                    "HTTP version is 1.1 but Host header is not set".to_string()
                ))?,
                path
            )
        } else {
            format!("http://localhost{path}")
        })
        .map_err(|_| InvalidRequestError("Can't parse request url".to_string()))?;

        Ok(Request {
            version,
            url,
            method,
            headers,
            params: HashMap::new(),
            body,
            client_addr,
        })
    }

    #[cfg(feature = "cgi")]
    pub(crate) fn from_cgi_env() -> Result<Request, InvalidRequestError> {
        use std::env;

        // Read method, path and version
        let method = env::var("REQUEST_METHOD")
            .ok()
            .and_then(|m| m.parse().ok())
            .ok_or(InvalidRequestError(
                "Can't read REQUEST_METHOD from env".to_string(),
            ))?;
        let mut path = env::var("PATH_INFO")
            .map_err(|_| InvalidRequestError("Can't read PATH_INFO from env".to_string()))?;
        if path.is_empty() {
            path = "/".to_string();
        }
        if let Ok(query_string) = env::var("QUERY_STRING")
            && !query_string.is_empty()
        {
            path = format!("{path}?{query_string}");
        }
        let version = match env::var("SERVER_PROTOCOL").as_deref() {
            Ok("HTTP/1.0") => Version::Http1_0,
            _ => Version::Http1_1,
        };

        // Read headers
        let mut headers = HeaderMap::new();
        for (key, value) in env::vars() {
            if let Some(key) = key.strip_prefix("HTTP_") {
                headers.insert(key.replace('_', "-"), value);
            }
        }

        // Read body
        let mut body = None;
        if let Ok(content_length) = env::var("CONTENT_LENGTH")
            && let Ok(content_length) = content_length.parse::<usize>()
        {
            if content_length > crate::MAX_REQUEST_BODY {
                return Err(InvalidRequestError("Content-Length too large".to_string()));
            }
            if content_length > 0 {
                let mut buffer = vec![0; content_length];
                io::stdin().read_exact(&mut buffer).map_err(|_| {
                    InvalidRequestError(
                        "Can't read Content-Length amount of bytes from stdin".to_string(),
                    )
                })?;
                body = Some(buffer);
            }
        }

        // Read remote address
        let client_addr = if let Ok(mut remote_addr) = env::var("REMOTE_ADDR") {
            if remote_addr.starts_with("::ffff:") {
                remote_addr = remote_addr.trim_start_matches("::ffff:").to_string();
            }
            let remote_port = env::var("REMOTE_PORT").unwrap_or_else(|_| "0".to_string());
            format!("{remote_addr}:{remote_port}")
                .parse()
                .map_err(|_| {
                    InvalidRequestError("Can't parse REMOTE_ADDR and REMOTE_PORT".to_string())
                })?
        } else {
            (Ipv4Addr::LOCALHOST, 0).into()
        };

        // Parse URL
        let url = Url::from_str(&if version == Version::Http1_1 {
            format!(
                "http://{}{}",
                headers.get("Host").ok_or(InvalidRequestError(
                    "HTTP version is 1.1 but Host header is not set".to_string()
                ))?,
                path
            )
        } else {
            format!("http://localhost{path}")
        })
        .map_err(|_| InvalidRequestError("Can't parse request url".to_string()))?;

        Ok(Request {
            version,
            url,
            method,
            headers,
            params: HashMap::new(),
            body,
            client_addr,
        })
    }

    /// Get the direct client IP address.
    ///
    /// Proxy forwarding headers are intentionally ignored because they can be supplied by any
    /// client. Use [`Self::ip_from_trusted_proxies`] when the server runs behind a trusted proxy.
    pub const fn ip(&self) -> IpAddr {
        self.client_addr.ip()
    }

    /// Get the forwarded client IP address when the direct peer is a trusted proxy.
    ///
    /// The first valid `X-Forwarded-For` value is preferred over `X-Real-IP`. Forwarding headers
    /// are ignored unless the direct peer address appears in `trusted_proxies`.
    pub fn ip_from_trusted_proxies(&self, trusted_proxies: &[IpAddr]) -> IpAddr {
        if !trusted_proxies.contains(&self.client_addr.ip()) {
            return self.ip();
        }
        self.headers
            .get("X-Forwarded-For")
            .and_then(|value| value.split(',').next())
            .or_else(|| self.headers.get("X-Real-IP"))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or_else(|| self.ip())
    }

    /// Write request to TCP stream
    pub fn write_to_stream(&mut self, stream: &mut dyn Write, keep_alive: bool) {
        // Finish headers
        let host = self.url.host().expect("No host in URL");
        self.headers.insert(
            "Host".to_string(),
            if host.contains(':') {
                if let Some(port) = self.url.port() {
                    format!("[{host}]:{port}")
                } else {
                    format!("[{host}]")
                }
            } else if let Some(port) = self.url.port() {
                format!("{host}:{port}")
            } else {
                host.to_string()
            },
        );
        self.headers.insert(
            "Content-Length".to_string(),
            if let Some(body) = &self.body {
                body.len()
            } else {
                0
            }
            .to_string(),
        );
        if self.version == Version::Http1_1 && !self.headers.contains_key("Connection") {
            if keep_alive {
                self.headers
                    .insert("Connection".to_string(), "keep-alive".to_string());
                self.headers.insert(
                    "Keep-Alive".to_string(),
                    format!("timeout={}", KEEP_ALIVE_TIMEOUT.as_secs()),
                );
            } else {
                self.headers
                    .insert("Connection".to_string(), "close".to_string());
            }
        }

        // Write request
        let path = self.url.path();
        let path = if let Some(query) = self.url.query() {
            format!("{path}?{query}")
        } else {
            path.to_string()
        };
        _ = write!(stream, "{} {} HTTP/1.1\r\n", self.method, path);
        for (name, value) in &self.headers {
            let safe_name = name.replace(['\r', '\n'], "");
            let safe_value = value.replace(['\r', '\n'], "");
            _ = write!(stream, "{safe_name}: {safe_value}\r\n");
        }
        _ = write!(stream, "\r\n");
        if let Some(body) = &self.body {
            _ = stream.write_all(body);
        }
    }

    /// Fetch request with http client, following redirects
    pub fn fetch(mut self) -> Result<Response, FetchError> {
        for _ in 0..=MAX_REDIRECTS {
            let mut stream = MaybeHttpsStream::connect(&self.url)?;
            self.write_to_stream(&mut stream, false);
            let res =
                Response::read_from_stream(&mut stream).map_err(|e| FetchError(e.to_string()))?;
            if !matches!(
                res.status,
                Status::MovedPermanently
                    | Status::Found
                    | Status::SeeOther
                    | Status::TemporaryRedirect
                    | Status::PermanentRedirect
            ) {
                return Ok(res);
            }

            let location = res
                .headers
                .get("Location")
                .ok_or_else(|| FetchError("Redirect has no Location header".to_string()))?;
            let url = self
                .url
                .join(location)
                .map_err(|e| FetchError(e.to_string()))?;
            if self.url.scheme() == "https" && url.scheme() != "https" {
                return Err(FetchError("Refusing HTTPS to HTTP redirect".to_string()));
            }
            if (url.scheme(), url.host(), url.port())
                != (self.url.scheme(), self.url.host(), self.url.port())
            {
                self.headers.remove("Authorization");
                self.headers.remove("Cookie");
            }
            if res.status == Status::SeeOther
                || (matches!(res.status, Status::MovedPermanently | Status::Found)
                    && self.method == Method::Post)
            {
                self.method = Method::Get;
                self.body = None;
                self.headers.remove("Content-Type");
            }
            self.url = url;
        }
        Err(FetchError("Too many redirects".to_string()))
    }
}

fn read_trailers(
    reader: &mut dyn BufRead,
    headers: &mut HeaderMap,
) -> Result<(), InvalidRequestError> {
    loop {
        let line = read_http_line(reader, "chunk trailer")?;
        if line == "\r\n" {
            return Ok(());
        }
        if headers.len() >= crate::MAX_HEADERS {
            return Err(InvalidRequestError("Too many headers".to_string()));
        }
        let split = line
            .find(':')
            .ok_or_else(|| InvalidRequestError("Can't parse chunk trailer".to_string()))?;
        let name = line[..split].trim();
        if name.eq_ignore_ascii_case("Content-Length")
            || name.eq_ignore_ascii_case("Transfer-Encoding")
            || name.eq_ignore_ascii_case("Host")
        {
            return Err(InvalidRequestError(
                "Framing header is not allowed in trailers".to_string(),
            ));
        }
        headers.append(name.to_string(), line[split + 1..].trim().to_string());
    }
}

fn read_http_line(
    reader: &mut dyn BufRead,
    description: &str,
) -> Result<String, InvalidRequestError> {
    let mut line = String::new();
    let bytes_read = reader
        .take(crate::MAX_HEADER_LINE + 1)
        .read_line(&mut line)
        .map_err(|_| InvalidRequestError(format!("Can't read {description}")))?;
    if bytes_read == 0 || bytes_read as u64 > crate::MAX_HEADER_LINE || !line.ends_with("\r\n") {
        return Err(InvalidRequestError(format!("Invalid {description}")));
    }
    Ok(line)
}

// MARK: InvalidRequestError
#[derive(Debug)]
pub(crate) struct InvalidRequestError(String);

impl Display for InvalidRequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid request: {}", self.0)
    }
}

impl Error for InvalidRequestError {}

// MARK: FetchError
/// HTTP client fetch error
#[derive(Debug)]
pub struct FetchError(pub(crate) String);

impl Display for FetchError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Fetch error: {}", self.0)
    }
}

impl From<io::Error> for FetchError {
    fn from(err: io::Error) -> Self {
        Self(err.to_string())
    }
}

impl Error for FetchError {}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::io::Write;
    use std::net::{Ipv4Addr, TcpListener};
    use std::thread;

    use super::*;

    fn fetch_from_local_server(response: &'static [u8]) -> Response {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let server_addr = listener.local_addr().unwrap();
        let server = thread::Builder::new()
            .name("test-http-server".to_string())
            .spawn(move || {
                let (stream, client_addr) = listener.accept().unwrap();
                let mut reader = io::BufReader::new(stream);
                Request::read_from_reader(&mut reader, client_addr).unwrap();
                let mut stream = reader.into_inner();
                stream.write_all(response).unwrap();
                stream.flush().unwrap();
            })
            .expect("Failed to spawn test HTTP server thread");

        let res = Request::get(format!("http://{server_addr}/"))
            .fetch()
            .unwrap();
        server.join().unwrap();
        res
    }

    #[test]
    fn test_read_from_stream() {
        let mut stream = &b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n"[..];
        let request =
            Request::read_from_reader(&mut stream, (Ipv4Addr::LOCALHOST, 12345).into()).unwrap();
        assert_eq!(request.method, Method::Get);
        assert_eq!(request.url.to_string(), "http://localhost/");
        assert_eq!(request.version, Version::Http1_1);
        assert_eq!(request.headers.get("Host").unwrap(), "localhost");
    }

    #[test]
    fn test_read_from_stream_with_body() {
        let mut stream =
            &b"POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 13\r\n\r\nHello, world!"[..];
        let request =
            Request::read_from_reader(&mut stream, (Ipv4Addr::LOCALHOST, 12345).into()).unwrap();
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.url.to_string(), "http://localhost/");
        assert_eq!(request.version, Version::Http1_1);
        assert_eq!(request.headers.get("Host").unwrap(), "localhost");
        assert_eq!(request.body.unwrap(), b"Hello, world!");
    }

    #[test]
    fn test_read_from_stream_with_body_lowercase_headers() {
        let mut stream =
            &b"POST / HTTP/1.1\r\nhost: localhost\r\ncontent-Length: 13\r\n\r\nHello, world!"[..];
        let request =
            Request::read_from_reader(&mut stream, (Ipv4Addr::LOCALHOST, 12345).into()).unwrap();
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.url.to_string(), "http://localhost/");
        assert_eq!(request.version, Version::Http1_1);
        assert_eq!(request.headers.get("Host").unwrap(), "localhost");
        assert_eq!(request.body.unwrap(), b"Hello, world!");
    }

    #[test]
    fn test_read_chunked_request_trailers_before_pipelined_request() {
        let input = b"POST /first HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n4\r\ntest\r\n0\r\nX-Checksum: valid\r\n\r\nGET /second HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let mut reader = io::BufReader::new(&input[..]);
        let client_addr = (Ipv4Addr::LOCALHOST, 12345).into();

        let first = Request::read_from_reader(&mut reader, client_addr).unwrap();
        assert_eq!(first.url.path(), "/first");
        assert_eq!(first.body.as_deref(), Some(b"test".as_slice()));
        assert_eq!(first.headers.get("X-Checksum"), Some("valid"));

        let second = Request::read_from_reader(&mut reader, client_addr).unwrap();
        assert_eq!(second.url.path(), "/second");
    }

    #[test]
    fn test_invalid_request_error() {
        let mut stream = &b"INVALID REQUEST"[..];
        let result = Request::read_from_reader(&mut stream, (Ipv4Addr::LOCALHOST, 12345).into());
        assert!(result.is_err());
    }

    #[test]
    fn test_rejects_ambiguous_request_framing() {
        for input in [
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost\r\nHost: example.com\r\n\r\n",
        ] {
            let mut stream = input.as_bytes();
            assert!(
                Request::read_from_reader(&mut stream, (Ipv4Addr::LOCALHOST, 12345).into())
                    .is_err()
            );
        }
    }

    #[test]
    fn test_rejects_overlong_or_unterminated_request_lines() {
        let overlong = format!(
            "GET /{} HTTP/1.1\r\nHost: localhost\r\n\r\n",
            "a".repeat(8192)
        );
        assert!(
            Request::read_from_reader(
                &mut overlong.as_bytes(),
                (Ipv4Addr::LOCALHOST, 12345).into()
            )
            .is_err()
        );

        let mut unterminated = &b"GET / HTTP/1.1"[..];
        assert!(
            Request::read_from_reader(&mut unterminated, (Ipv4Addr::LOCALHOST, 12345).into())
                .is_err()
        );
    }

    #[test]
    fn test_rejects_framing_headers_in_trailers() {
        let mut stream = &b"POST / HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 0\r\n\r\n"[..];
        assert!(
            Request::read_from_reader(&mut stream, (Ipv4Addr::LOCALHOST, 12345).into()).is_err()
        );
    }

    #[test]
    fn test_write_to_stream() {
        let mut request = Request::get("http://localhost/").header("Host", "localhost");

        let mut buffer = Vec::new();
        request.write_to_stream(&mut buffer, false);
        assert!(buffer.starts_with(b"GET / HTTP/1.1\r\n"));
    }

    #[test]
    fn test_write_to_stream_preserves_connection_header() {
        let mut request = Request::get("http://localhost/").header("Connection", "Upgrade");
        let mut buffer = Vec::new();

        request.write_to_stream(&mut buffer, false);

        let request = String::from_utf8(buffer).unwrap();
        assert!(request.contains("\r\nConnection: Upgrade\r\n"));
        assert!(!request.contains("\r\nConnection: close\r\n"));
    }

    #[test]
    fn test_write_to_stream_with_body() {
        let mut request = Request::post("http://localhost/")
            .header("Host", "localhost")
            .body("Hello, world!");

        let mut buffer = Vec::new();
        request.write_to_stream(&mut buffer, false);
        assert!(buffer.starts_with(b"POST / HTTP/1.1\r\n"));
    }

    #[test]
    fn test_header_builder_uses_last_value() {
        let request = Request::get("http://localhost/")
            .header("X-Test", "first")
            .header("x-test", "second");

        assert_eq!(request.headers.get("X-Test"), Some("second"));
        assert_eq!(request.headers.get_all("x-test").count(), 1);
    }

    #[test]
    fn test_ip_ignores_untrusted_forwarding_headers() {
        let request = Request::new()
            .header("X-Forwarded-For", "203.0.113.10")
            .header("X-Real-IP", "203.0.113.11");

        assert_eq!(request.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
        assert_eq!(
            request.ip_from_trusted_proxies(&[]),
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }

    #[test]
    fn test_ip_uses_forwarding_header_from_trusted_proxy() {
        let request = Request::new().header("X-Forwarded-For", "203.0.113.10, 198.51.100.20");

        assert_eq!(
            request.ip_from_trusted_proxies(&[IpAddr::V4(Ipv4Addr::LOCALHOST)]),
            "203.0.113.10".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn test_write_to_stream_has_one_automatic_content_length() {
        let mut request = Request::post("http://localhost/")
            .header("content-length", "999")
            .body("test");
        let mut buffer = Vec::new();

        request.write_to_stream(&mut buffer, false);

        let request_text = String::from_utf8(buffer).unwrap();
        let content_lengths = request_text
            .lines()
            .filter(|line| line.to_ascii_lowercase().starts_with("content-length:"))
            .collect::<Vec<_>>();
        assert_eq!(content_lengths, ["Content-Length: 4"]);
    }

    #[test]
    fn test_fetch_http1_0() {
        let res = fetch_from_local_server(b"HTTP/1.0 200 OK\r\nContent-Length: 4\r\n\r\ntest");
        assert_eq!(res.status, Status::Ok);
        assert_eq!(res.body, "test".as_bytes());
    }

    #[test]
    fn test_fetch_http1_1() {
        let res = fetch_from_local_server(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntest",
        );
        assert_eq!(res.status, Status::Ok);
        assert_eq!(res.body, "test".as_bytes());
    }

    #[test]
    fn test_fetch_follows_redirects() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::Builder::new()
            .name("test-http-server".to_string())
            .spawn(move || {
                let responses = [
                    "HTTP/1.1 303 See Other\r\nLocation: next\r\nContent-Length: 0\r\n\r\n".to_string(),
                    format!(
                        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://localhost:{port}/final\r\nContent-Length: 0\r\n\r\n"
                    ),
                    "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndone".to_string(),
                ];
                responses
                    .map(|response| {
                        let (stream, client_addr) = listener.accept().unwrap();
                        let mut reader = io::BufReader::new(stream);
                        let req = Request::read_from_reader(&mut reader, client_addr).unwrap();
                        reader.into_inner().write_all(response.as_bytes()).unwrap();
                        req
                    })
            })
            .expect("Failed to spawn test HTTP server thread");

        let res = Request::post(format!("http://127.0.0.1:{port}/start"))
            .header("Authorization", "Bearer secret")
            .body("data")
            .fetch()
            .unwrap();
        assert_eq!(res.body, b"done");

        let [start, next, last] = server.join().unwrap();
        assert_eq!((start.method, start.url.path()), (Method::Post, "/start"));
        assert_eq!((next.method, next.url.path()), (Method::Get, "/next"));
        assert_eq!(next.body, None);
        assert_eq!(next.headers.get("Authorization"), Some("Bearer secret"));
        assert_eq!(last.url.path(), "/final");
        assert_eq!(last.headers.get("Authorization"), None);
    }
}
