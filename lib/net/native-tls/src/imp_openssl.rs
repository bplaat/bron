/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! OpenSSL TLS implementation for Linux and other Unix platforms.
//!
//! Supports OpenSSL 1.0.2+ (Ubuntu 16.04, old RHEL/CentOS), 1.1.x
//! (Ubuntu 18.04/20.04, RHEL 7/8), 3.x (Ubuntu 22.04+, Debian 12,
//! Fedora, Arch) and 4.x.
//!
//! The build script detects the system OpenSSL version and sets cfg flags:
//! - `openssl_v10x` for 1.0.x releases
//! - `openssl_v4xx` for 4.x and later
//!
//! Both code paths below implement the same public API.

#![allow(unused)]

use std::ffi::{CString, c_char, c_int, c_long, c_ulong, c_void};
use std::io::{self, Read, Write};

use crate::{Error, HandshakeError};

// MARK: FFI

// All supported OpenSSL versions
unsafe extern "C" {
    fn SSL_CTX_new(method: *const c_void) -> *mut c_void;
    fn SSL_CTX_free(ctx: *mut c_void);
    fn SSL_CTX_set_verify(ctx: *mut c_void, mode: c_int, callback: *const c_void);
    fn SSL_CTX_set_default_verify_paths(ctx: *mut c_void) -> c_int;
    fn SSL_CTX_ctrl(ctx: *mut c_void, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
    fn SSL_new(ctx: *mut c_void) -> *mut c_void;
    fn SSL_free(ssl: *mut c_void);
    fn SSL_set_bio(ssl: *mut c_void, rbio: *mut c_void, wbio: *mut c_void);
    fn SSL_connect(ssl: *mut c_void) -> c_int;
    fn SSL_read(ssl: *mut c_void, buf: *mut c_void, num: c_int) -> c_int;
    fn SSL_write(ssl: *mut c_void, buf: *const c_void, num: c_int) -> c_int;
    fn SSL_shutdown(ssl: *mut c_void) -> c_int;
    fn SSL_get_error(ssl: *const c_void, ret: c_int) -> c_int;
    fn SSL_ctrl(ssl: *mut c_void, cmd: c_int, larg: c_long, parg: *mut c_void) -> c_long;
    // OpenSSL error queue - available in all versions
    fn ERR_get_error() -> c_ulong;
    fn ERR_error_string(err: c_ulong, buf: *mut c_char) -> *mut c_char;
}

// OpenSSL 1.0.x
unsafe extern "C" {
    fn SSLv23_client_method() -> *const c_void;
    fn SSL_library_init() -> c_int;
    fn SSL_load_error_strings();
    fn OPENSSL_add_all_algorithms_noconf();
    fn BIO_new_bio_pair(
        bio1: *mut *mut c_void,
        writebuf1: usize,
        bio2: *mut *mut c_void,
        writebuf2: usize,
    ) -> c_int;
    fn BIO_read(bio: *mut c_void, buf: *mut c_void, len: c_int) -> c_int;
    fn BIO_write(bio: *mut c_void, buf: *const c_void, len: c_int) -> c_int;
    fn BIO_free(bio: *mut c_void) -> c_int;

    // OpenSSL 1.0.2+
    fn SSL_get_peer_certificate(ssl: *const c_void) -> *mut c_void;
    fn X509_free(cert: *mut c_void);
    fn X509_check_host(
        cert: *mut c_void,
        chk: *const c_char,
        chklen: usize,
        flags: u32,
        peername: *mut *mut c_char,
    ) -> c_int;
}

// OpenSSL 1.1.x / 3.x / 4.x
unsafe extern "C" {
    fn TLS_client_method() -> *const c_void;
    fn BIO_meth_new(type_: c_int, name: *const c_char) -> *mut c_void;
    fn BIO_meth_free(biom: *mut c_void);
    fn BIO_meth_set_read(
        biom: *mut c_void,
        read: unsafe extern "C" fn(*mut c_void, *mut c_char, c_int) -> c_int,
    ) -> c_int;
    fn BIO_meth_set_write(
        biom: *mut c_void,
        write: unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_int,
    ) -> c_int;
    fn BIO_meth_set_ctrl(
        biom: *mut c_void,
        ctrl: unsafe extern "C" fn(*mut c_void, c_int, c_long, *mut c_void) -> c_long,
    ) -> c_int;
    fn BIO_meth_set_create(
        biom: *mut c_void,
        create: unsafe extern "C" fn(*mut c_void) -> c_int,
    ) -> c_int;
    fn BIO_meth_set_destroy(
        biom: *mut c_void,
        destroy: unsafe extern "C" fn(*mut c_void) -> c_int,
    ) -> c_int;
    fn BIO_new(biom: *const c_void) -> *mut c_void;
    fn BIO_set_data(bio: *mut c_void, data: *mut c_void);
    fn BIO_get_data(bio: *const c_void) -> *mut c_void;
    fn BIO_set_init(bio: *mut c_void, init: c_int);
    fn BIO_set_flags(bio: *mut c_void, flags: c_int);
    fn BIO_clear_flags(bio: *mut c_void, flags: c_int);

    // OpenSSL 1.1.x / 3.x
    fn SSL_set1_host(ssl: *mut c_void, hostname: *const c_char) -> c_int;

    // OpenSSL 4.x
    fn SSL_set1_dnsname(ssl: *mut c_void, dnsname: *const c_char) -> c_int;
}

// Common constants
const SSL_VERIFY_NONE: c_int = 0x00;
const SSL_VERIFY_PEER: c_int = 0x01;
const SSL_ERROR_NONE: c_int = 0;
const SSL_ERROR_ZERO_RETURN: c_int = 6;
const SSL_ERROR_WANT_READ: c_int = 2;
const SSL_ERROR_WANT_WRITE: c_int = 3;
// SSL_set_tlsext_host_name macro equivalent (SSL_ctrl cmd)
const SSL_CTRL_SET_TLSEXT_HOSTNAME: c_int = 55;
const TLSEXT_NAMETYPE_HOST_NAME: c_long = 0;

// Drain the OpenSSL per-thread error queue into a human-readable string.
fn openssl_error_string() -> String {
    let mut parts = Vec::new();
    loop {
        // SAFETY: ERR_get_error is thread-safe; it pops one entry from the per-thread error queue.
        let code = unsafe { ERR_get_error() };
        if code == 0 {
            break;
        }
        let mut buf = [0u8; 256];
        // SAFETY: code is a valid OpenSSL error code; buf is a valid writable buffer of 256 bytes.
        unsafe { ERR_error_string(code, buf.as_mut_ptr() as *mut c_char) };
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        parts.push(String::from_utf8_lossy(&buf[..end]).into_owned());
    }
    parts.join(": ")
}

fn ssl_error(context: &str, code: c_int) -> Error {
    let detail = openssl_error_string();
    if detail.is_empty() {
        Error(format!("{context} (SSL_get_error={code})"))
    } else {
        Error(format!("{context}: {detail}"))
    }
}

// ---------------------------------------------------------------------------
// MARK: OpenSSL 1.0.x path
// Uses BIO_new_bio_pair + explicit I/O pumping because the BIO method API
// (BIO_meth_new, BIO_set_data, etc.) does not exist in 1.0.x.
// ---------------------------------------------------------------------------

// SSL_CTX_ctrl cmd to set option flags (SSL_CTX_set_options macro equivalent)
#[cfg(openssl_v10x)]
const SSL_CTRL_OPTIONS: c_int = 32;
#[cfg(openssl_v10x)]
const SSL_OP_NO_SSLV2: c_long = 0x01000000;
#[cfg(openssl_v10x)]
const SSL_OP_NO_SSLV3: c_long = 0x02000000;
#[cfg(openssl_v10x)]
const SSL_OP_NO_TLSV1: c_long = 0x04000000;
#[cfg(openssl_v10x)]
const SSL_OP_NO_TLSV1_1: c_long = 0x10000000;

// Drain all pending encrypted output from network_bio to the TCP stream.
#[cfg(openssl_v10x)]
fn flush_network_bio(bio: *mut c_void, stream: &mut impl Write) -> io::Result<()> {
    let mut buf = [0u8; 4096];
    loop {
        // SAFETY: bio is a non-null BIO pair handle; buf is a valid writable buffer.
        let n = unsafe { BIO_read(bio, buf.as_mut_ptr() as *mut c_void, buf.len() as c_int) };
        if n <= 0 {
            break;
        }
        stream.write_all(&buf[..n as usize])?;
    }
    Ok(())
}

// Read one chunk from the TCP stream and feed it into network_bio for SSL.
#[cfg(openssl_v10x)]
fn feed_network_bio(bio: *mut c_void, stream: &mut impl Read) -> io::Result<usize> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    if n > 0 {
        // SAFETY: bio is a non-null BIO pair handle; buf[..n] is valid readable data.
        let written = unsafe { BIO_write(bio, buf.as_ptr() as *const c_void, n as c_int) };
        if written < 0 {
            return Err(io::Error::other("BIO_write to BIO pair failed"));
        }
    }
    Ok(n)
}

// MARK: TlsConnector (1.0.x)
#[cfg(openssl_v10x)]
/// A TLS connector using OpenSSL 1.0.x
pub struct TlsConnector {
    ctx: *mut c_void,
    accept_invalid_certs: bool,
}

#[cfg(openssl_v10x)]
// SAFETY: SSL_CTX is reference-counted by OpenSSL and is safe to share across threads.
unsafe impl Send for TlsConnector {}
#[cfg(openssl_v10x)]
// SAFETY: SSL_CTX is reference-counted by OpenSSL and is safe to use from multiple threads.
unsafe impl Sync for TlsConnector {}

#[cfg(openssl_v10x)]
impl TlsConnector {
    /// Create a new TLS connector
    pub fn new() -> Result<Self, Error> {
        Self::new_with_accept_invalid_certs(false)
    }

    #[cfg(test)]
    pub(crate) fn new_danger_accept_invalid_certs() -> Result<Self, Error> {
        Self::new_with_accept_invalid_certs(true)
    }

    fn new_with_accept_invalid_certs(accept_invalid_certs: bool) -> Result<Self, Error> {
        // OpenSSL 1.0.x requires explicit one-time initialization; 1.1+ does this automatically.
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            // SAFETY: these init functions are idempotent; Once ensures single execution.
            unsafe {
                SSL_library_init();
                SSL_load_error_strings();
                OPENSSL_add_all_algorithms_noconf();
            }
        });

        // SAFETY: SSLv23_client_method returns a valid method pointer valid for the process lifetime.
        let ctx = unsafe { SSL_CTX_new(SSLv23_client_method()) };
        if ctx.is_null() {
            return Err(Error(format!(
                "Failed to create SSL_CTX: {}",
                openssl_error_string()
            )));
        }
        // SAFETY: ctx is non-null; constants and null callback are valid for SSL_CTX_ctrl/set_verify.
        unsafe {
            // Disable SSLv2, SSLv3, TLS 1.0 and TLS 1.1 to require TLS 1.2+
            let opts = SSL_OP_NO_SSLV2 | SSL_OP_NO_SSLV3 | SSL_OP_NO_TLSV1 | SSL_OP_NO_TLSV1_1;
            SSL_CTX_ctrl(ctx, SSL_CTRL_OPTIONS, opts, std::ptr::null_mut());
            SSL_CTX_set_verify(
                ctx,
                if accept_invalid_certs {
                    SSL_VERIFY_NONE
                } else {
                    SSL_VERIFY_PEER
                },
                std::ptr::null(),
            );
            if !accept_invalid_certs && SSL_CTX_set_default_verify_paths(ctx) != 1 {
                SSL_CTX_free(ctx);
                return Err(Error("Failed to load CA certificates".to_string()));
            }
        }
        Ok(Self {
            ctx,
            accept_invalid_certs,
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
        stream: S,
    ) -> Result<TlsStream<S>, Error> {
        // SAFETY: self.ctx is a non-null SSL_CTX valid for the connector's lifetime.
        let ssl = unsafe { SSL_new(self.ctx) };
        if ssl.is_null() {
            return Err(Error(format!(
                "Failed to create SSL: {}",
                openssl_error_string()
            )));
        }

        let domain_c =
            CString::new(domain).map_err(|_| Error("Invalid domain name".to_string()))?;

        // Set SNI hostname
        // SAFETY: ssl is non-null; domain_c is a valid C string valid for the call duration.
        unsafe {
            SSL_ctrl(
                ssl,
                SSL_CTRL_SET_TLSEXT_HOSTNAME,
                TLSEXT_NAMETYPE_HOST_NAME,
                domain_c.as_ptr() as *mut c_void,
            );
        }

        // Create a BIO pair: SSL holds internal_bio; we pump via network_bio
        let mut internal_bio: *mut c_void = std::ptr::null_mut();
        let mut network_bio: *mut c_void = std::ptr::null_mut();
        // SAFETY: &mut internal_bio and &mut network_bio are valid output pointers; buf sizes of 0
        // request the default OpenSSL buffer size.
        if unsafe { BIO_new_bio_pair(&mut internal_bio, 0, &mut network_bio, 0) } != 1 {
            // SAFETY: ssl is non-null; not freed elsewhere.
            unsafe { SSL_free(ssl) };
            return Err(Error(format!(
                "Failed to create BIO pair: {}",
                openssl_error_string()
            )));
        }
        // SSL_set_bio takes ownership of internal_bio; SSL_free will release it
        // SAFETY: ssl, internal_bio are non-null; ownership of internal_bio transfers to ssl.
        unsafe { SSL_set_bio(ssl, internal_bio, internal_bio) };

        let mut stream = Box::new(stream);

        // Handshake + hostname verification inside a closure for clean cleanup
        let result: Result<(), Error> = (|| {
            loop {
                // SAFETY: ssl is non-null and configured with a BIO pair.
                let ret = unsafe { SSL_connect(ssl) };
                if ret == 1 {
                    break;
                }
                // SAFETY: ssl is non-null.
                let err = unsafe { SSL_get_error(ssl, ret) };
                match err {
                    SSL_ERROR_WANT_READ => {
                        flush_network_bio(network_bio, &mut *stream)
                            .map_err(|e| Error(e.to_string()))?;
                        let n = feed_network_bio(network_bio, &mut *stream)
                            .map_err(|e| Error(e.to_string()))?;
                        if n == 0 {
                            return Err(Error("TLS handshake: unexpected EOF".to_string()));
                        }
                    }
                    SSL_ERROR_WANT_WRITE => {
                        flush_network_bio(network_bio, &mut *stream)
                            .map_err(|e| Error(e.to_string()))?;
                    }
                    _ => {
                        return Err(ssl_error("TLS handshake failed", err));
                    }
                }
            }
            // Flush any final handshake messages (e.g. Finished)
            flush_network_bio(network_bio, &mut *stream).map_err(|e| Error(e.to_string()))?;

            // Verify hostname via X509_check_host (OpenSSL 1.0.2+)
            // SAFETY: ssl is non-null; the returned cert pointer must be freed with X509_free.
            let cert = unsafe { SSL_get_peer_certificate(ssl) };
            if cert.is_null() {
                return Err(Error("No peer certificate".to_string()));
            }
            // SAFETY: cert is non-null; domain_c and domain.len() describe a valid hostname.
            let ok = unsafe {
                X509_check_host(
                    cert,
                    domain_c.as_ptr(),
                    domain.len(),
                    0,
                    std::ptr::null_mut(),
                )
            };
            // SAFETY: cert is a non-null X509 object returned by SSL_get_peer_certificate.
            unsafe { X509_free(cert) };
            if ok != 1 && !self.accept_invalid_certs {
                return Err(Error(format!(
                    "Hostname verification failed for '{domain}'"
                )));
            }
            Ok(())
        })();

        if let Err(e) = result {
            // SAFETY: ssl and network_bio are non-null; not freed elsewhere.
            unsafe {
                SSL_free(ssl);
                BIO_free(network_bio);
            }
            return Err(e);
        }

        Ok(TlsStream {
            ssl,
            network_bio,
            stream,
        })
    }
}

#[cfg(openssl_v10x)]
impl Drop for TlsConnector {
    fn drop(&mut self) {
        // SAFETY: self.ctx is non-null; Drop is called at most once.
        unsafe { SSL_CTX_free(self.ctx) };
    }
}

// MARK: TlsStream (1.0.x)
#[cfg(openssl_v10x)]
/// A TLS stream backed by OpenSSL 1.0.x (BIO pair I/O pump)
pub struct TlsStream<S> {
    ssl: *mut c_void,
    network_bio: *mut c_void,
    stream: Box<S>,
}

#[cfg(openssl_v10x)]
// SAFETY: SSL* objects are not thread-safe for concurrent use, but transferring ownership
// to another thread is safe since we only access ssl/network_bio under &mut self.
unsafe impl<S: Send> Send for TlsStream<S> {}

#[cfg(openssl_v10x)]
impl<S> TlsStream<S> {
    /// Returns a reference to the underlying stream
    pub fn get_ref(&self) -> &S {
        &self.stream
    }

    /// Returns a mutable reference to the underlying stream
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }
}

#[cfg(openssl_v10x)]
impl<S: Read + Write> Read for TlsStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // SAFETY: self.ssl is non-null; buf is valid for buf.len() bytes.
            let ret = unsafe {
                SSL_read(
                    self.ssl,
                    buf.as_mut_ptr() as *mut c_void,
                    buf.len().min(c_int::MAX as usize) as c_int,
                )
            };
            if ret > 0 {
                flush_network_bio(self.network_bio, &mut *self.stream)?;
                return Ok(ret as usize);
            }
            // SAFETY: self.ssl is non-null.
            let err = unsafe { SSL_get_error(self.ssl, ret) };
            match err {
                SSL_ERROR_ZERO_RETURN | SSL_ERROR_NONE => return Ok(0),
                SSL_ERROR_WANT_READ => {
                    flush_network_bio(self.network_bio, &mut *self.stream)?;
                    let n = feed_network_bio(self.network_bio, &mut *self.stream)?;
                    if n == 0 {
                        return Ok(0);
                    }
                }
                SSL_ERROR_WANT_WRITE => {
                    flush_network_bio(self.network_bio, &mut *self.stream)?;
                }
                _ => return Err(io::Error::other(ssl_error("SSL_read error", err).0)),
            }
        }
    }
}

#[cfg(openssl_v10x)]
impl<S: Read + Write> Write for TlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            // SAFETY: self.ssl is non-null; buf is valid for buf.len() bytes.
            let ret = unsafe {
                SSL_write(
                    self.ssl,
                    buf.as_ptr() as *const c_void,
                    buf.len().min(c_int::MAX as usize) as c_int,
                )
            };
            if ret > 0 {
                flush_network_bio(self.network_bio, &mut *self.stream)?;
                return Ok(ret as usize);
            }
            // SAFETY: self.ssl is non-null.
            let err = unsafe { SSL_get_error(self.ssl, ret) };
            match err {
                SSL_ERROR_WANT_WRITE => {
                    flush_network_bio(self.network_bio, &mut *self.stream)?;
                }
                SSL_ERROR_WANT_READ => {
                    // TLS renegotiation: must read before retrying write
                    flush_network_bio(self.network_bio, &mut *self.stream)?;
                    feed_network_bio(self.network_bio, &mut *self.stream)?;
                }
                _ => return Err(io::Error::other(ssl_error("SSL_write error", err).0)),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        flush_network_bio(self.network_bio, &mut *self.stream)?;
        self.stream.flush()
    }
}

#[cfg(openssl_v10x)]
impl<S> Drop for TlsStream<S> {
    fn drop(&mut self) {
        // Two-phase shutdown: first call sends our close_notify; second waits for the peer's.
        // SAFETY: self.ssl is non-null; Drop is called at most once.
        unsafe {
            if SSL_shutdown(self.ssl) == 0 {
                SSL_shutdown(self.ssl);
            }
            SSL_free(self.ssl); // releases internal_bio (the SSL side of the pair)
            BIO_free(self.network_bio);
        }
    }
}

// ---------------------------------------------------------------------------
// MARK: OpenSSL 1.1.x / 3.x / 4.x path
// Uses TLS_client_method, a custom BIO method wrapping the Rust stream
// (callbacks directly read/write the stream), and SSL_set1_host for
// hostname verification (SSL_set1_dnsname on OpenSSL 4.x where SSL_set1_host
// is deprecated).
// ---------------------------------------------------------------------------
#[cfg(not(openssl_v10x))]
const SSL_CTRL_SET_MIN_PROTO_VERSION: c_int = 123;
#[cfg(not(openssl_v10x))]
const SSL_CTRL_SET_MAX_PROTO_VERSION: c_int = 124;
#[cfg(not(openssl_v10x))]
const TLS1_2_VERSION: c_long = 0x0303;
#[cfg(not(openssl_v10x))]
const TLS1_3_VERSION: c_long = 0x0304;
// Custom BIO type: SOURCE_SINK bit (0x0400) | custom ID
#[cfg(not(openssl_v10x))]
const CUSTOM_BIO_TYPE: c_int = 0x0400 | 100;
#[cfg(not(openssl_v10x))]
const BIO_CTRL_FLUSH: c_int = 11;

// BIO retry flag constants (bio.h)
#[cfg(not(openssl_v10x))]
const BIO_FLAGS_READ: c_int = 0x01;
#[cfg(not(openssl_v10x))]
const BIO_FLAGS_WRITE: c_int = 0x02;
#[cfg(not(openssl_v10x))]
const BIO_FLAGS_IO_SPECIAL: c_int = 0x04;
#[cfg(not(openssl_v10x))]
const BIO_FLAGS_SHOULD_RETRY: c_int = 0x08;

#[cfg(not(openssl_v10x))]
unsafe fn bio_clear_retry_flags(bio: *mut c_void) {
    // SAFETY: bio is non-null; flags are the standard retry-flag bitmask.
    unsafe {
        BIO_clear_flags(
            bio,
            BIO_FLAGS_READ | BIO_FLAGS_WRITE | BIO_FLAGS_IO_SPECIAL | BIO_FLAGS_SHOULD_RETRY,
        )
    };
}

#[cfg(not(openssl_v10x))]
unsafe fn bio_set_retry_read(bio: *mut c_void) {
    // SAFETY: bio is non-null.
    unsafe { BIO_set_flags(bio, BIO_FLAGS_READ | BIO_FLAGS_SHOULD_RETRY) };
}

#[cfg(not(openssl_v10x))]
unsafe fn bio_set_retry_write(bio: *mut c_void) {
    // SAFETY: bio is non-null.
    unsafe { BIO_set_flags(bio, BIO_FLAGS_WRITE | BIO_FLAGS_SHOULD_RETRY) };
}

// MARK: BIO callbacks (1.1.x / 3.x)
// Type-erased stream wrapper stored in the BIO's data pointer
#[cfg(not(openssl_v10x))]
struct IoFuncs {
    stream_ptr: *mut c_void,
    read_fn: unsafe fn(*mut c_void, *mut u8, usize) -> io::Result<usize>,
    write_fn: unsafe fn(*mut c_void, *const u8, usize) -> io::Result<usize>,
}

#[cfg(not(openssl_v10x))]
unsafe extern "C" fn bio_read(bio: *mut c_void, buf: *mut c_char, len: c_int) -> c_int {
    if len <= 0 {
        return 0;
    }
    // SAFETY: bio is non-null; clearing retry flags before each operation is required by OpenSSL.
    unsafe { bio_clear_retry_flags(bio) };
    // SAFETY: bio is non-null; BIO_get_data returns the IoFuncs pointer set via BIO_set_data.
    let io = unsafe { &mut *(BIO_get_data(bio) as *mut IoFuncs) };
    // SAFETY: io.read_fn and io.stream_ptr are set in connect_inner; buf/len come from OpenSSL.
    match unsafe { (io.read_fn)(io.stream_ptr, buf.cast::<u8>(), len as usize) } {
        Ok(0) => 0,
        Ok(n) => n as c_int,
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
            // SAFETY: bio is non-null.
            unsafe { bio_set_retry_read(bio) };
            -1
        }
        Err(_) => -1,
    }
}

#[cfg(not(openssl_v10x))]
unsafe extern "C" fn bio_write(bio: *mut c_void, buf: *const c_char, len: c_int) -> c_int {
    if len <= 0 {
        return 0;
    }
    // SAFETY: bio is non-null; clearing retry flags before each operation is required by OpenSSL.
    unsafe { bio_clear_retry_flags(bio) };
    // SAFETY: bio is non-null; BIO_get_data returns the IoFuncs pointer set via BIO_set_data.
    let io = unsafe { &mut *(BIO_get_data(bio) as *mut IoFuncs) };
    // SAFETY: io.write_fn and io.stream_ptr are set in connect_inner; buf/len come from OpenSSL.
    match unsafe { (io.write_fn)(io.stream_ptr, buf.cast::<u8>(), len as usize) } {
        Ok(n) => n as c_int,
        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
            // SAFETY: bio is non-null.
            unsafe { bio_set_retry_write(bio) };
            -1
        }
        Err(_) => -1,
    }
}

#[cfg(not(openssl_v10x))]
const unsafe extern "C" fn bio_ctrl(
    _bio: *mut c_void,
    cmd: c_int,
    _larg: c_long,
    _parg: *mut c_void,
) -> c_long {
    if cmd == BIO_CTRL_FLUSH { 1 } else { 0 }
}

#[cfg(not(openssl_v10x))]
unsafe extern "C" fn bio_create(bio: *mut c_void) -> c_int {
    // SAFETY: bio is non-null; setting init=1 marks the BIO as ready.
    unsafe { BIO_set_init(bio, 1) };
    1
}

#[cfg(not(openssl_v10x))]
const unsafe extern "C" fn bio_destroy(_bio: *mut c_void) -> c_int {
    // IoFuncs is owned by TlsStream; do not free here
    1
}

#[cfg(not(openssl_v10x))]
unsafe fn do_read<S: Read>(ptr: *mut c_void, buf: *mut u8, len: usize) -> io::Result<usize> {
    use std::slice;
    // SAFETY: ptr derives from Box<S>.as_ref() in connect_inner; buf/len come from OpenSSL.
    unsafe { (*(ptr as *mut S)).read(slice::from_raw_parts_mut(buf, len)) }
}

#[cfg(not(openssl_v10x))]
unsafe fn do_write<S: Write>(ptr: *mut c_void, buf: *const u8, len: usize) -> io::Result<usize> {
    use std::slice;
    // SAFETY: ptr derives from Box<S>.as_ref() in connect_inner; buf/len come from OpenSSL.
    unsafe { (*(ptr as *mut S)).write(slice::from_raw_parts(buf, len)) }
}

#[cfg(not(openssl_v10x))]
fn make_bio_method() -> *mut c_void {
    // SAFETY: the name is a valid C string; all callback functions have the correct signatures.
    unsafe {
        let m = BIO_meth_new(CUSTOM_BIO_TYPE, c"rust-stream".as_ptr());
        BIO_meth_set_read(m, bio_read);
        BIO_meth_set_write(m, bio_write);
        BIO_meth_set_ctrl(m, bio_ctrl);
        BIO_meth_set_create(m, bio_create);
        BIO_meth_set_destroy(m, bio_destroy);
        m
    }
}

#[cfg(not(openssl_v10x))]
struct BioMethodPtr(*mut c_void);
// SAFETY: BIO_METHOD is immutable after creation; OpenSSL guarantees thread safety for reads.
#[cfg(not(openssl_v10x))]
unsafe impl Send for BioMethodPtr {}
// SAFETY: see the Send impl above.
#[cfg(not(openssl_v10x))]
unsafe impl Sync for BioMethodPtr {}

#[cfg(not(openssl_v10x))]
impl Drop for BioMethodPtr {
    fn drop(&mut self) {
        // SAFETY: self.0 is a non-null BIO_METHOD allocated by BIO_meth_new; Drop once.
        unsafe { BIO_meth_free(self.0) };
    }
}

#[cfg(not(openssl_v10x))]
static BIO_METHOD: std::sync::LazyLock<BioMethodPtr> =
    std::sync::LazyLock::new(|| BioMethodPtr(make_bio_method()));

// MARK: TlsConnector (1.1.x / 3.x / 4.x)
#[cfg(not(openssl_v10x))]
/// A TLS connector using OpenSSL 1.1.x, 3.x or 4.x
pub struct TlsConnector {
    ctx: *mut c_void,
    accept_invalid_certs: bool,
}

#[cfg(not(openssl_v10x))]
// SAFETY: SSL_CTX is reference-counted and thread-safe for concurrent use.
unsafe impl Send for TlsConnector {}
#[cfg(not(openssl_v10x))]
// SAFETY: SSL_CTX is reference-counted and thread-safe for concurrent use.
unsafe impl Sync for TlsConnector {}

#[cfg(not(openssl_v10x))]
impl TlsConnector {
    /// Create a new TLS connector
    pub fn new() -> Result<Self, Error> {
        Self::new_with_accept_invalid_certs(false)
    }

    #[cfg(test)]
    pub(crate) fn new_danger_accept_invalid_certs() -> Result<Self, Error> {
        Self::new_with_accept_invalid_certs(true)
    }

    fn new_with_accept_invalid_certs(accept_invalid_certs: bool) -> Result<Self, Error> {
        // SAFETY: TLS_client_method returns a valid method pointer for the process lifetime.
        let ctx = unsafe { SSL_CTX_new(TLS_client_method()) };
        if ctx.is_null() {
            return Err(Error(format!(
                "Failed to create SSL_CTX: {}",
                openssl_error_string()
            )));
        }
        // SAFETY: ctx is non-null; version constants and null callback are valid arguments.
        unsafe {
            // Require TLS 1.2 minimum, allow up to TLS 1.3
            SSL_CTX_ctrl(
                ctx,
                SSL_CTRL_SET_MIN_PROTO_VERSION,
                TLS1_2_VERSION,
                std::ptr::null_mut(),
            );
            SSL_CTX_ctrl(
                ctx,
                SSL_CTRL_SET_MAX_PROTO_VERSION,
                TLS1_3_VERSION,
                std::ptr::null_mut(),
            );
            SSL_CTX_set_verify(
                ctx,
                if accept_invalid_certs {
                    SSL_VERIFY_NONE
                } else {
                    SSL_VERIFY_PEER
                },
                std::ptr::null(),
            );
            if !accept_invalid_certs && SSL_CTX_set_default_verify_paths(ctx) != 1 {
                SSL_CTX_free(ctx);
                return Err(Error("Failed to load CA certificates".to_string()));
            }
        }
        Ok(Self {
            ctx,
            accept_invalid_certs,
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
        stream: S,
    ) -> Result<TlsStream<S>, Error> {
        // SAFETY: self.ctx is a non-null SSL_CTX valid for the connector's lifetime.
        let ssl = unsafe { SSL_new(self.ctx) };
        if ssl.is_null() {
            return Err(Error(format!(
                "Failed to create SSL: {}",
                openssl_error_string()
            )));
        }

        let domain_c =
            CString::new(domain).map_err(|_| Error("Invalid domain name".to_string()))?;
        // SAFETY: ssl is non-null; domain_c is a valid C string for the call duration.
        unsafe {
            // Set SNI hostname
            SSL_ctrl(
                ssl,
                SSL_CTRL_SET_TLSEXT_HOSTNAME,
                TLSEXT_NAMETYPE_HOST_NAME,
                domain_c.as_ptr() as *mut c_void,
            );
            if !self.accept_invalid_certs {
                // Enable built-in hostname verification against the certificate.
                // SSL_set1_dnsname is the new name in OpenSSL 4.x; older versions use SSL_set1_host.
                let ok = cfg_select! {
                    openssl_v4xx => SSL_set1_dnsname(ssl, domain_c.as_ptr()),
                    _ => SSL_set1_host(ssl, domain_c.as_ptr()),
                };
                if ok != 1 {
                    SSL_free(ssl);
                    return Err(Error("Failed to set hostname for verification".to_string()));
                }
            }
        }

        let stream = Box::new(stream);
        let io = Box::new(IoFuncs {
            stream_ptr: stream.as_ref() as *const S as *mut c_void,
            read_fn: do_read::<S>,
            write_fn: do_write::<S>,
        });

        // SAFETY: BIO_METHOD.0 is non-null (initialized by LazyLock).
        let bio = unsafe { BIO_new(BIO_METHOD.0) };
        if bio.is_null() {
            // SAFETY: ssl is non-null; not freed elsewhere.
            unsafe { SSL_free(ssl) };
            return Err(Error(format!(
                "Failed to create BIO: {}",
                openssl_error_string()
            )));
        }
        // SAFETY: bio and ssl are non-null; ownership of bio (rbio=wbio) transfers to ssl.
        unsafe {
            BIO_set_data(bio, io.as_ref() as *const IoFuncs as *mut c_void);
            SSL_set_bio(ssl, bio, bio);
        }

        loop {
            // SAFETY: ssl is non-null and configured with a BIO.
            let ret = unsafe { SSL_connect(ssl) };
            if ret == 1 {
                break;
            }
            // SAFETY: ssl is non-null.
            let err = unsafe { SSL_get_error(ssl, ret) };
            match err {
                SSL_ERROR_WANT_READ | SSL_ERROR_WANT_WRITE => continue,
                _ => {
                    // SAFETY: ssl is non-null; BIO is owned by ssl and freed via SSL_free.
                    unsafe { SSL_free(ssl) };
                    return Err(ssl_error("TLS handshake failed", err));
                }
            }
        }

        Ok(TlsStream {
            ssl,
            _io: io,
            stream,
        })
    }
}

#[cfg(not(openssl_v10x))]
impl Drop for TlsConnector {
    fn drop(&mut self) {
        // SAFETY: self.ctx is non-null; Drop is called at most once.
        unsafe { SSL_CTX_free(self.ctx) };
    }
}

// MARK: TlsStream (1.1.x / 3.x / 4.x)
#[cfg(not(openssl_v10x))]
/// A TLS stream backed by OpenSSL 1.1.x, 3.x or 4.x
pub struct TlsStream<S> {
    ssl: *mut c_void,
    _io: Box<IoFuncs>,
    stream: Box<S>,
}

#[cfg(not(openssl_v10x))]
// SAFETY: SSL* objects are not thread-safe for concurrent use, but transferring ownership
// to another thread is safe since we only access ssl under &mut self.
unsafe impl<S: Send> Send for TlsStream<S> {}

#[cfg(not(openssl_v10x))]
impl<S> TlsStream<S> {
    /// Returns a reference to the underlying stream
    pub fn get_ref(&self) -> &S {
        &self.stream
    }

    /// Returns a mutable reference to the underlying stream
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.stream
    }
}

#[cfg(not(openssl_v10x))]
impl<S: Read + Write> Read for TlsStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // SAFETY: self.ssl is non-null; buf is valid for buf.len() bytes.
        let ret = unsafe {
            SSL_read(
                self.ssl,
                buf.as_mut_ptr() as *mut c_void,
                buf.len().min(c_int::MAX as usize) as c_int,
            )
        };
        if ret > 0 {
            return Ok(ret as usize);
        }
        // SAFETY: self.ssl is non-null.
        let err = unsafe { SSL_get_error(self.ssl, ret) };
        match err {
            SSL_ERROR_ZERO_RETURN | SSL_ERROR_NONE => Ok(0),
            SSL_ERROR_WANT_READ | SSL_ERROR_WANT_WRITE => {
                Err(io::Error::new(io::ErrorKind::WouldBlock, "TLS would block"))
            }
            _ => Err(io::Error::other(ssl_error("SSL_read error", err).0)),
        }
    }
}

#[cfg(not(openssl_v10x))]
impl<S: Read + Write> Write for TlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        // SAFETY: self.ssl is non-null; buf is valid for buf.len() bytes.
        let ret = unsafe {
            SSL_write(
                self.ssl,
                buf.as_ptr() as *const c_void,
                buf.len().min(c_int::MAX as usize) as c_int,
            )
        };
        if ret > 0 {
            return Ok(ret as usize);
        }
        // SAFETY: self.ssl is non-null.
        let err = unsafe { SSL_get_error(self.ssl, ret) };
        match err {
            SSL_ERROR_WANT_READ | SSL_ERROR_WANT_WRITE => {
                Err(io::Error::new(io::ErrorKind::WouldBlock, "TLS would block"))
            }
            _ => Err(io::Error::other(ssl_error("SSL_write error", err).0)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(not(openssl_v10x))]
impl<S> Drop for TlsStream<S> {
    fn drop(&mut self) {
        // Two-phase shutdown: first call sends our close_notify; second waits for the peer's.
        // SAFETY: self.ssl is non-null; Drop is called at most once.
        unsafe {
            if SSL_shutdown(self.ssl) == 0 {
                SSL_shutdown(self.ssl);
            }
            SSL_free(self.ssl); // also frees the BIO
        }
    }
}
