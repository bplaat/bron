/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! rustls TLS implementation for the `vendored` feature.
//!
//! Uses rustls with the Graviola backend and webpki-roots for embedded CA certificates.
//! Fully self-contained: no system OpenSSL or C compilation required.

use std::io::{self, Read, Write};
use std::sync::Arc;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore};

use crate::{Error, HandshakeError};

// MARK: TlsConnector
/// A TLS connector using rustls with embedded CA roots
pub struct TlsConnector {
    config: Arc<ClientConfig>,
}

impl TlsConnector {
    /// Create a new TLS connector
    pub fn new() -> Result<Self, Error> {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| Error(e.to_string()))?
                .with_root_certificates(roots)
                .with_no_client_auth();
        Ok(Self {
            config: Arc::new(config),
        })
    }

    #[cfg(test)]
    pub(crate) fn new_danger_accept_invalid_certs() -> Result<Self, Error> {
        #[derive(Debug)]
        struct NoCertificateVerification;

        impl rustls::client::danger::ServerCertVerifier for NoCertificateVerification {
            fn verify_server_cert(
                &self,
                _end_entity: &rustls::pki_types::CertificateDer<'_>,
                _intermediates: &[rustls::pki_types::CertificateDer<'_>],
                _server_name: &ServerName<'_>,
                _ocsp_response: &[u8],
                _now: rustls::pki_types::UnixTime,
            ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
                Ok(rustls::client::danger::ServerCertVerified::assertion())
            }

            fn verify_tls12_signature(
                &self,
                _message: &[u8],
                _cert: &rustls::pki_types::CertificateDer<'_>,
                _dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
            {
                Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
            }

            fn verify_tls13_signature(
                &self,
                _message: &[u8],
                _cert: &rustls::pki_types::CertificateDer<'_>,
                _dss: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
            {
                Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
            }

            fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
                rustls_graviola::default_provider()
                    .signature_verification_algorithms
                    .supported_schemes()
            }
        }

        let config =
            ClientConfig::builder_with_provider(Arc::new(rustls_graviola::default_provider()))
                .with_safe_default_protocol_versions()
                .map_err(|e| Error(e.to_string()))?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoCertificateVerification))
                .with_no_client_auth();
        Ok(Self {
            config: Arc::new(config),
        })
    }

    /// Perform a TLS handshake over the given stream
    pub fn connect<S: Read + Write>(
        &self,
        domain: &str,
        stream: S,
    ) -> Result<TlsStream<S>, HandshakeError<S>> {
        self.connect_inner(domain, stream)
            .map_err(HandshakeError::Failure)
    }

    fn connect_inner<S: Read + Write>(
        &self,
        domain: &str,
        mut stream: S,
    ) -> Result<TlsStream<S>, Error> {
        let server_name = ServerName::try_from(domain.to_string())
            .map_err(|e| Error(format!("Invalid domain name: {e}")))?;
        let mut conn = ClientConnection::new(self.config.clone(), server_name)
            .map_err(|e| Error(e.to_string()))?;
        while conn.is_handshaking() {
            conn.complete_io(&mut stream)
                .map_err(|e| Error(e.to_string()))?;
        }
        Ok(TlsStream { conn, stream })
    }
}

// MARK: TlsStream
/// A TLS stream backed by rustls
pub struct TlsStream<S> {
    conn: ClientConnection,
    stream: S,
}

impl<S> TlsStream<S> {
    /// Returns a reference to the underlying stream
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn get_ref(&self) -> &S {
        &self.stream
    }

    /// Returns a mutable reference to the underlying stream
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }
}

impl<S: Read + Write> Read for TlsStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        rustls::Stream::new(&mut self.conn, &mut self.stream).read(buf)
    }
}

impl<S: Read + Write> Write for TlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        rustls::Stream::new(&mut self.conn, &mut self.stream).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        rustls::Stream::new(&mut self.conn, &mut self.stream).flush()
    }
}
