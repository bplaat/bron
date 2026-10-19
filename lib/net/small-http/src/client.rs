/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(feature = "tls")]
use native_tls::TlsStream;
use url::Url;

use crate::header_map::HeaderMap;
use crate::request::{FetchError, Request};
use crate::response::Response;
use crate::{CONNECT_TIMEOUT, IO_TIMEOUT, KEEP_ALIVE_TIMEOUT};

// MARK: MaybeHttpsStream
#[allow(clippy::large_enum_variant)]
pub(crate) enum MaybeHttpsStream {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(TlsStream<TcpStream>),
}

impl MaybeHttpsStream {
    /// Connect to the host of an HTTP(S) URL, trying each resolved address with a timeout
    pub(crate) fn connect(url: &Url) -> Result<Self, FetchError> {
        let host = url
            .host()
            .ok_or_else(|| FetchError("URL has no host".to_string()))?;
        let is_https = match url.scheme() {
            "http" => false,
            "https" => true,
            scheme => return Err(FetchError(format!("Unsupported URL scheme: {scheme}"))),
        };
        let port = url.port().unwrap_or(if is_https { 443 } else { 80 });

        let mut result = Err(io::Error::other("Host has no addresses"));
        for address in (host, port).to_socket_addrs()? {
            result = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT);
            if result.is_ok() {
                break;
            }
        }
        let tcp = result?;
        tcp.set_read_timeout(Some(IO_TIMEOUT))?;
        tcp.set_write_timeout(Some(IO_TIMEOUT))?;

        if is_https {
            #[cfg(feature = "tls")]
            {
                let tls = native_tls::TlsConnector::new()
                    .map_err(|e| FetchError(e.to_string()))?
                    .connect(host, tcp)
                    .map_err(|e| FetchError(e.to_string()))?;
                return Ok(Self::Tls(tls));
            }
            #[cfg(not(feature = "tls"))]
            return Err(FetchError("HTTPS requires the tls feature".to_string()));
        }
        Ok(Self::Plain(tcp))
    }

    pub(crate) fn set_read_timeout(&self, dur: Option<Duration>) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.set_read_timeout(dur),
            #[cfg(feature = "tls")]
            Self::Tls(s) => s.get_ref().set_read_timeout(dur),
        }
    }
}

impl Read for MaybeHttpsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            #[cfg(feature = "tls")]
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for MaybeHttpsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            #[cfg(feature = "tls")]
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            #[cfg(feature = "tls")]
            Self::Tls(s) => s.flush(),
        }
    }
}

// MARK: HTTP Client
/// HTTP client
#[derive(Default, Clone)]
pub struct Client {
    connection_pool: Arc<Mutex<ConnectionPool>>,
    headers: HeaderMap,
}

impl Client {
    /// Create a new HTTP client
    pub fn new() -> Self {
        Self::default()
    }

    /// Set header
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    fn apply_default_headers(&self, request: &mut Request) {
        for (name, value) in &self.headers {
            if !request.headers.contains_key(name) {
                request.headers.insert(name.clone(), value.clone());
            }
        }
    }

    /// Fetch a request
    pub fn fetch(&mut self, mut request: Request) -> Result<Response, FetchError> {
        // Add client headers to request
        self.apply_default_headers(&mut request);

        // Build connection key
        let host = request
            .url
            .host()
            .ok_or_else(|| FetchError("URL has no host".to_string()))?;
        let is_https = request.url.scheme() == "https";
        let port = request
            .url
            .port()
            .unwrap_or(if is_https { 443 } else { 80 });
        let conn_key = format!("{}://{}", request.url.scheme(), socket_address(host, port));

        // Get or create connection
        let mut stream = self
            .connection_pool
            .lock()
            .expect("Can't lock connection pool")
            .take_connection(&conn_key, &request.url)?;
        stream
            .get_ref()
            .set_read_timeout(Some(KEEP_ALIVE_TIMEOUT))?;

        // Send request and read response
        request.write_to_stream(stream.get_mut(), true);
        let res = Response::read_from_buffered_stream(&mut stream)
            .map_err(|e| FetchError(e.to_string()))?;

        // Return connection
        self.connection_pool
            .lock()
            .expect("Can't lock connection pool")
            .return_connection(&conn_key, stream);
        Ok(res)
    }
}

fn socket_address(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

// MARK: ConnectionPool
#[derive(Default)]
struct ConnectionPool {
    connections: HashMap<String, Vec<BufReader<MaybeHttpsStream>>>,
}

impl ConnectionPool {
    fn take_connection(
        &mut self,
        key: &str,
        url: &Url,
    ) -> Result<BufReader<MaybeHttpsStream>, FetchError> {
        if let Some(conn) = self.connections.get_mut(key).and_then(Vec::pop) {
            return Ok(conn);
        }
        Ok(BufReader::new(MaybeHttpsStream::connect(url)?))
    }

    fn return_connection(&mut self, key: &str, conn: BufReader<MaybeHttpsStream>) {
        self.connections
            .entry(key.to_string())
            .or_default()
            .push(conn);
    }
}

// MARK: Tests
#[cfg(test)]
mod test {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpListener};
    use std::thread;

    use super::*;

    #[test]
    fn test_request_headers_override_client_defaults() {
        let client = Client::new().header("X-Test", "default");
        let mut request = Request::get("http://localhost/").header("x-test", "request");

        client.apply_default_headers(&mut request);

        assert_eq!(request.headers.get("X-Test"), Some("request"));
        assert_eq!(request.headers.get_all("x-test").count(), 1);
    }

    #[test]
    fn test_ipv6_connection_keys_are_unambiguous() {
        let first = format!("https://{}", socket_address("::1", 443));
        let second = format!("https://{}", socket_address("::1:443", 443));

        assert_eq!(first, "https://[::1]:443");
        assert_eq!(second, "https://[::1:443]:443");
        assert_ne!(first, second);
    }

    #[test]
    fn test_client_multiple_requests() {
        // Start test server
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let server_addr = listener.local_addr().unwrap();
        thread::Builder::new()
            .name("test-http-server".to_string())
            .spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                loop {
                    let mut buf = [0; 512];
                    _ = stream.read(&mut buf);
                    stream
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: closed\r\n\r\ntest",
                    )
                    .unwrap();
                }
            })
            .expect("Failed to spawn test HTTP server thread");

        // Create client and fetch multiple requests
        let mut client = Client::new();
        for _ in 0..10 {
            client
                .fetch(Request::get(format!("http://{server_addr}/")))
                .unwrap();
        }
    }
}
