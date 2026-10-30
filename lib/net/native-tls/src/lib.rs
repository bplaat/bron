/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(unsafe_code)]

use std::any::Any;
use std::convert::Infallible;
use std::fmt::{self, Display, Formatter};
use std::io::{Read, Write};
use std::marker::PhantomData;

cfg_select! {
    // The vendored feature uses rustls on every platform.
    feature = "vendored" => {
        use crate::imp_rustls as imp;
        mod imp_rustls;
    }
    target_vendor = "apple" => {
        use crate::imp_macos as imp;
        mod imp_macos;
    }
    windows => {
        use crate::imp_windows as imp;
        mod imp_windows;
    }
    _ => {
        use crate::imp_openssl as imp;
        mod imp_openssl;
    }
}

// MARK: Error
/// TLS error
#[derive(Debug)]
pub struct Error(String);

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

// MARK: HandshakeError
/// An error returned from `TlsConnector::connect`.
#[derive(Debug)]
pub enum HandshakeError<S> {
    /// A fatal error.
    Failure(Error),

    /// A stream interrupted midway through the handshake process due to a `WouldBlock` error.
    ///
    /// The handshake is always driven to completion, so this variant is never returned.
    WouldBlock(MidHandshakeTlsStream<S>),
}

impl<S: Any + fmt::Debug> std::error::Error for HandshakeError<S> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Failure(error) => Some(error),
            Self::WouldBlock(_) => None,
        }
    }
}

impl<S: Any + fmt::Debug> Display for HandshakeError<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Failure(error) => Display::fmt(error, f),
            Self::WouldBlock(_) => f.write_str("the handshake process was interrupted"),
        }
    }
}

// MARK: MidHandshakeTlsStream
/// A TLS stream which has been interrupted midway through the handshake process.
///
/// The handshake is always driven to completion, so this type is never constructed.
pub struct MidHandshakeTlsStream<S>(Infallible, PhantomData<S>);

impl<S: fmt::Debug> fmt::Debug for MidHandshakeTlsStream<S> {
    fn fmt(&self, _f: &mut Formatter<'_>) -> fmt::Result {
        match self.0 {}
    }
}

impl<S> MidHandshakeTlsStream<S> {
    /// Returns a shared reference to the inner stream.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn get_ref(&self) -> &S {
        match self.0 {}
    }

    /// Returns a mutable reference to the inner stream.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn get_mut(&mut self) -> &mut S {
        match self.0 {}
    }
}

impl<S: Read + Write> MidHandshakeTlsStream<S> {
    /// Restarts the handshake process.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn handshake(self) -> Result<TlsStream<S>, HandshakeError<S>> {
        match self.0 {}
    }
}

// MARK: TlsConnector
/// A TLS connector for creating TLS connections
pub use imp::TlsConnector;
// MARK: TlsStream
/// A TLS stream wrapping an underlying I/O stream
pub use imp::TlsStream;

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use rustls_pki_types::pem::PemObject;

    use super::*;

    const LOCALHOST_CERT_PEM: &[u8] = include_bytes!("../tests/certs/localhost.crt");
    const LOCALHOST_KEY_PEM: &[u8] = include_bytes!("../tests/certs/localhost.key");
    const WRONG_HOST_CERT_PEM: &[u8] = include_bytes!("../tests/certs/wrong-host.crt");
    const WRONG_HOST_KEY_PEM: &[u8] = include_bytes!("../tests/certs/wrong-host.key");
    const EXPIRED_LOCALHOST_CERT_PEM: &[u8] =
        include_bytes!("../tests/certs/expired-localhost.crt");
    const EXPIRED_LOCALHOST_KEY_PEM: &[u8] = include_bytes!("../tests/certs/expired-localhost.key");

    fn local_https_get(server_addr: std::net::SocketAddr, path: &str) -> String {
        let tcp = TcpStream::connect(server_addr).expect("TCP connect failed");
        let connector =
            TlsConnector::new_danger_accept_invalid_certs().expect("TlsConnector::new failed");
        let mut tls = connector
            .connect("localhost", tcp)
            .expect("TLS handshake failed");
        write!(tls, "GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n").expect("write failed");
        read_http_response(&mut tls)
    }

    fn read_http_response<S: Read>(stream: &mut S) -> String {
        let mut response = Vec::new();
        let mut buffer = [0; 1024];
        let header_end = loop {
            let bytes_read = stream.read(&mut buffer).expect("read response failed");
            assert!(bytes_read > 0, "connection closed before HTTP headers");
            response.extend_from_slice(&buffer[..bytes_read]);
            if let Some(index) = response.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };

        let headers =
            std::str::from_utf8(&response[..header_end]).expect("response headers should be UTF-8");
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().expect("valid Content-Length"))
            })
            .expect("response should include Content-Length");

        while response.len() < header_end + content_length {
            let bytes_read = stream.read(&mut buffer).expect("read response body failed");
            assert!(bytes_read > 0, "connection closed before HTTP body");
            response.extend_from_slice(&buffer[..bytes_read]);
        }

        response.truncate(header_end + content_length);
        String::from_utf8(response).expect("response should be UTF-8")
    }

    fn local_tls_server_config(cert_pem: &[u8], key_pem: &[u8]) -> rustls::ServerConfig {
        let certs = vec![
            rustls_pki_types::CertificateDer::from_pem_slice(cert_pem)
                .expect("test certificate should parse"),
        ];
        let key = rustls_pki_types::PrivateKeyDer::from_pem_slice(key_pem)
            .expect("test private key should parse");

        rustls::ServerConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("test TLS protocol versions should be supported")
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("test TLS server config should build")
    }

    fn spawn_local_https_server(
        response_body: &'static [u8],
        connection_count: usize,
    ) -> (std::net::SocketAddr, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind failed");
        let server_addr = listener.local_addr().expect("local_addr failed");
        let server = thread::Builder::new()
            .name("test-https-server".to_string())
            .spawn(move || {
                let config = Arc::new(local_tls_server_config(
                    LOCALHOST_CERT_PEM,
                    LOCALHOST_KEY_PEM,
                ));
                for _ in 0..connection_count {
                    let (mut tcp, _) = listener.accept().expect("accept failed");
                    tcp.set_read_timeout(Some(Duration::from_secs(5)))
                        .expect("set_read_timeout failed");
                    tcp.set_write_timeout(Some(Duration::from_secs(5)))
                        .expect("set_write_timeout failed");

                    let mut conn = rustls::ServerConnection::new(config.clone()).unwrap();
                    while conn.is_handshaking() {
                        conn.complete_io(&mut tcp).expect("TLS handshake failed");
                    }

                    {
                        let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
                        let mut request = Vec::new();
                        let mut buffer = [0; 1024];
                        loop {
                            let bytes_read = tls.read(&mut buffer).expect("read request failed");
                            assert!(bytes_read > 0, "connection closed before HTTP request");
                            request.extend_from_slice(&buffer[..bytes_read]);
                            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                                break;
                            }
                        }

                        let mut response = format!(
                            "HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n",
                            response_body.len()
                        )
                        .into_bytes();
                        response.extend_from_slice(response_body);
                        tls.write_all(&response).expect("write response failed");
                        tls.flush().expect("flush response failed");
                    }
                    conn.send_close_notify();
                    let _ = conn.complete_io(&mut tcp);
                }
            })
            .expect("Failed to spawn test HTTPS server thread");
        (server_addr, server)
    }

    fn assert_local_tls_server_rejected(
        domain: &str,
        cert_pem: &'static [u8],
        key_pem: &'static [u8],
    ) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind failed");
        let server_addr = listener.local_addr().expect("local_addr failed");
        let server = thread::Builder::new()
            .name("test-tls-fragment-server".to_string())
            .spawn(move || {
                let (mut tcp, _) = listener.accept().expect("accept failed");
                tcp.set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("set_read_timeout failed");
                tcp.set_write_timeout(Some(Duration::from_secs(5)))
                    .expect("set_write_timeout failed");

                let mut conn = rustls::ServerConnection::new(Arc::new(local_tls_server_config(
                    cert_pem, key_pem,
                )))
                .unwrap();
                let _ = conn.complete_io(&mut tcp);
            })
            .expect("Failed to spawn test TLS fragment server thread");

        let tcp = TcpStream::connect(server_addr).expect("TCP connect failed");
        let connector = TlsConnector::new().expect("TlsConnector::new failed");
        let result = connector.connect(domain, tcp);
        assert!(result.is_err(), "Expected TLS certificate to be rejected");
        server.join().expect("test TLS server thread panicked");
    }

    #[test]
    fn test_handshake_succeeds() {
        let (server_addr, server) = spawn_local_https_server(b"ok", 1);
        let response = local_https_get(server_addr, "/");
        assert!(response.starts_with("HTTP/"), "TLS request failed");
        server.join().expect("test TLS server thread panicked");
    }

    #[test]
    fn test_preserves_application_data_after_handshake() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind failed");
        let server_addr = listener.local_addr().expect("local_addr failed");
        let server = thread::Builder::new()
            .name("test-tls-application-data-server".to_string())
            .spawn(move || {
                let (mut tcp, _) = listener.accept().expect("accept failed");
                let config = Arc::new(local_tls_server_config(
                    LOCALHOST_CERT_PEM,
                    LOCALHOST_KEY_PEM,
                ));
                let mut conn = rustls::ServerConnection::new(config).unwrap();
                while conn.is_handshaking() {
                    conn.complete_io(&mut tcp).expect("TLS handshake failed");
                }
                let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
                tls.write_all(b"hello").expect("write greeting failed");
                tls.flush().expect("flush greeting failed");
            })
            .expect("Failed to spawn test TLS application data server thread");

        let tcp = TcpStream::connect(server_addr).expect("TCP connect failed");
        let connector =
            TlsConnector::new_danger_accept_invalid_certs().expect("TlsConnector::new failed");
        let mut tls = connector
            .connect("localhost", tcp)
            .expect("TLS handshake failed");
        let mut greeting = [0; 5];
        tls.read_exact(&mut greeting).expect("read greeting failed");
        assert_eq!(&greeting, b"hello");
        server.join().expect("test TLS server thread panicked");
    }

    #[test]
    fn test_https_get_returns_http_response() {
        let (server_addr, server) = spawn_local_https_server(b"test", 1);
        let response = local_https_get(server_addr, "/");
        assert!(
            response.starts_with("HTTP/"),
            "Expected HTTP response, got: {response}"
        );
        server.join().expect("test TLS server thread panicked");
    }

    #[test]
    fn test_https_get_response_contains_html() {
        let (server_addr, server) =
            spawn_local_https_server(b"<!doctype html><html><body>test</body></html>", 1);
        let response = local_https_get(server_addr, "/");
        let lower = response.to_lowercase();
        assert!(
            lower.contains("<html"),
            "Expected HTML in body, got: {response}"
        );
        server.join().expect("test TLS server thread panicked");
    }

    #[test]
    fn test_multiple_sequential_connections() {
        let (server_addr, server) = spawn_local_https_server(b"test", 3);
        for i in 0..3 {
            let response = local_https_get(server_addr, "/");
            assert!(
                response.starts_with("HTTP/"),
                "Connection {i}: expected HTTP response"
            );
        }
        server.join().expect("test TLS server thread panicked");
    }

    #[test]
    fn test_self_signed_wrong_hostname_cert_rejected() {
        assert_local_tls_server_rejected("localhost", WRONG_HOST_CERT_PEM, WRONG_HOST_KEY_PEM);
    }

    #[test]
    fn test_expired_self_signed_cert_rejected() {
        assert_local_tls_server_rejected(
            "localhost",
            EXPIRED_LOCALHOST_CERT_PEM,
            EXPIRED_LOCALHOST_KEY_PEM,
        );
    }

    #[test]
    fn test_self_signed_cert_rejected() {
        assert_local_tls_server_rejected("localhost", LOCALHOST_CERT_PEM, LOCALHOST_KEY_PEM);
    }

    #[test]
    fn test_different_connectors() {
        let (server_addr, server) = spawn_local_https_server(b"test", 2);
        let r1 = local_https_get(server_addr, "/first");
        let r2 = local_https_get(server_addr, "/second");
        assert!(r1.starts_with("HTTP/"), "first connector failed");
        assert!(r2.starts_with("HTTP/"), "second connector failed");
        server.join().expect("test TLS server thread panicked");
    }
}
