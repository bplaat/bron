/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! macOS TLS implementation using Security.framework SecureTransport

use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::os::raw::{c_char, c_int};
use std::slice;

use crate::{Error, HandshakeError};

// MARK: FFI types and constants
type SSLContextRef = *mut c_void;
type SSLConnectionRef = *const c_void;
type SecTrustRef = *mut c_void;
type CFErrorRef = *mut c_void;
type OSStatus = i32;
type SSLReadFunc = unsafe extern "C" fn(SSLConnectionRef, *mut c_void, *mut usize) -> OSStatus;
type SSLWriteFunc = unsafe extern "C" fn(SSLConnectionRef, *const c_void, *mut usize) -> OSStatus;

const SSL_CLIENT_SIDE: u32 = 1;
const SSL_STREAM_TYPE: u32 = 0;
// kSSLSessionOptionBreakOnServerAuth = 0: pause handshake after server cert received for manual trust eval
const SSL_OPT_BREAK_ON_SERVER_AUTH: i32 = 0;
// kTLSProtocol12 = 8 (SSLProtocol is c_int)
const K_TLS_PROTOCOL_12: c_int = 8;
const NO_ERR: OSStatus = 0;
const ERR_SSL_WOULD_BLOCK: OSStatus = -9803;
const ERR_SSL_CLOSED_GRACEFUL: OSStatus = -9805;
const ERR_SSL_CLOSED_ABORT: OSStatus = -9806;
const ERR_SSL_CLOSED_NO_NOTIFY: OSStatus = -9816;
// errSSLPeerAuthCompleted: returned when break-on-server-auth fires and the server cert is ready
const ERR_SSL_PEER_AUTH_COMPLETED: OSStatus = -9841;
const IO_ERR: OSStatus = -36;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SSLCreateContext(
        alloc: *const c_void,
        protocol_side: u32,
        connection_type: u32,
    ) -> SSLContextRef;
    fn SSLSetSessionOption(ctx: SSLContextRef, option: i32, value: u8) -> OSStatus;
    fn SSLSetIOFuncs(ctx: SSLContextRef, read: SSLReadFunc, write: SSLWriteFunc) -> OSStatus;
    fn SSLSetConnection(ctx: SSLContextRef, connection: SSLConnectionRef) -> OSStatus;
    fn SSLSetProtocolVersionMin(ctx: SSLContextRef, min_ver: c_int) -> OSStatus;
    fn SSLSetPeerDomainName(ctx: SSLContextRef, name: *const c_char, len: usize) -> OSStatus;
    fn SSLHandshake(ctx: SSLContextRef) -> OSStatus;
    fn SSLCopyPeerTrust(ctx: SSLContextRef, trust: *mut SecTrustRef) -> OSStatus;
    fn SecTrustEvaluateWithError(trust: SecTrustRef, error: *mut CFErrorRef) -> u8;
    fn SSLRead(
        ctx: SSLContextRef,
        data: *mut c_void,
        data_len: usize,
        processed: *mut usize,
    ) -> OSStatus;
    fn SSLWrite(
        ctx: SSLContextRef,
        data: *const c_void,
        data_len: usize,
        processed: *mut usize,
    ) -> OSStatus;
    fn SSLClose(ctx: SSLContextRef) -> OSStatus;
}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

// MARK: IoFuncs
// Type-erased I/O wrapper passed as the SSL connection reference
struct IoFuncs {
    stream_ptr: *mut c_void,
    read_fn: unsafe fn(*mut c_void, *mut u8, usize) -> io::Result<usize>,
    write_fn: unsafe fn(*mut c_void, *const u8, usize) -> io::Result<usize>,
}

unsafe extern "C" fn ssl_read_cb(
    conn: SSLConnectionRef,
    data: *mut c_void,
    data_len: *mut usize,
) -> OSStatus {
    if data.is_null() || data_len.is_null() || conn.is_null() {
        return IO_ERR;
    }
    // SAFETY: conn was set via SSLSetConnection with a &IoFuncs pointer that lives as long as the
    // SSL context; alignment and size are guaranteed by the Box<IoFuncs> in connect_inner.
    let io = unsafe { &mut *(conn as *mut IoFuncs) };
    // SAFETY: data_len is non-null (checked above).
    let total = unsafe { *data_len };
    // SAFETY: SecureTransport guarantees data is non-null and points to at least data_len bytes.
    let buf = unsafe { slice::from_raw_parts_mut(data as *mut u8, total) };
    let mut got = 0usize;
    // SecureTransport requires all `total` bytes to be read before returning noErr
    while got < total {
        // SAFETY: io.read_fn and io.stream_ptr are set in connect_inner and valid for SSL context lifetime.
        match unsafe { (io.read_fn)(io.stream_ptr, buf.as_mut_ptr().add(got), total - got) } {
            Ok(0) => {
                // SAFETY: data_len is non-null.
                unsafe { *data_len = got };
                return ERR_SSL_CLOSED_NO_NOTIFY;
            }
            Ok(n) => got += n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // SAFETY: data_len is non-null.
                unsafe { *data_len = got };
                return ERR_SSL_WOULD_BLOCK;
            }
            Err(_) => {
                // SAFETY: data_len is non-null.
                unsafe { *data_len = got };
                return IO_ERR;
            }
        }
    }
    // SAFETY: data_len is non-null.
    unsafe { *data_len = got };
    NO_ERR
}

unsafe extern "C" fn ssl_write_cb(
    conn: SSLConnectionRef,
    data: *const c_void,
    data_len: *mut usize,
) -> OSStatus {
    if data.is_null() || data_len.is_null() || conn.is_null() {
        return IO_ERR;
    }
    // SAFETY: conn was set via SSLSetConnection with a &IoFuncs pointer valid for SSL context lifetime.
    let io = unsafe { &mut *(conn as *mut IoFuncs) };
    // SAFETY: data_len is non-null (checked above).
    let total = unsafe { *data_len };
    // SAFETY: SecureTransport guarantees data is non-null and points to at least data_len bytes.
    let buf = unsafe { slice::from_raw_parts(data as *const u8, total) };
    let mut sent = 0usize;
    // SecureTransport requires all `total` bytes to be written before returning noErr
    while sent < total {
        // SAFETY: io.write_fn and io.stream_ptr are set in connect_inner and valid for SSL context lifetime.
        match unsafe { (io.write_fn)(io.stream_ptr, buf.as_ptr().add(sent), total - sent) } {
            Ok(0) => {
                // SAFETY: data_len is non-null.
                unsafe { *data_len = sent };
                return ERR_SSL_CLOSED_NO_NOTIFY;
            }
            Ok(n) => sent += n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // SAFETY: data_len is non-null.
                unsafe { *data_len = sent };
                return ERR_SSL_WOULD_BLOCK;
            }
            Err(_) => {
                // SAFETY: data_len is non-null.
                unsafe { *data_len = sent };
                return IO_ERR;
            }
        }
    }
    // SAFETY: data_len is non-null.
    unsafe { *data_len = sent };
    NO_ERR
}

unsafe fn do_read<S: Read>(ptr: *mut c_void, buf: *mut u8, len: usize) -> io::Result<usize> {
    // SAFETY: ptr derives from Box<S>.as_ref() in connect_inner; buf/len come from the SSL layer.
    let stream = unsafe { &mut *(ptr as *mut S) };
    // SAFETY: buf/len are provided by SecureTransport and guaranteed valid for len bytes.
    let slice = unsafe { slice::from_raw_parts_mut(buf, len) };
    stream.read(slice)
}

unsafe fn do_write<S: Write>(ptr: *mut c_void, buf: *const u8, len: usize) -> io::Result<usize> {
    // SAFETY: ptr derives from Box<S>.as_ref() in connect_inner; buf/len come from the SSL layer.
    let stream = unsafe { &mut *(ptr as *mut S) };
    // SAFETY: buf/len are provided by SecureTransport and guaranteed valid for len bytes.
    let slice = unsafe { slice::from_raw_parts(buf, len) };
    stream.write(slice)
}

// MARK: TlsConnector
/// A TLS connector using SecureTransport
pub struct TlsConnector {
    accept_invalid_certs: bool,
}

impl TlsConnector {
    /// Create a new TLS connector
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            accept_invalid_certs: false,
        })
    }

    #[cfg(test)]
    pub(crate) const fn new_danger_accept_invalid_certs() -> Result<Self, Error> {
        Ok(Self {
            accept_invalid_certs: true,
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
        // SAFETY: null allocator selects the default CF allocator; enum values are valid.
        let ctx = unsafe { SSLCreateContext(std::ptr::null(), SSL_CLIENT_SIDE, SSL_STREAM_TYPE) };
        if ctx.is_null() {
            return Err(Error("Failed to create SSL context".to_string()));
        }

        // Break handshake after server cert is received so we can do manual trust evaluation.
        // Without this, SSLHandshake tries implicit validation which is broken on macOS 26+.
        // SAFETY: ctx is non-null; option/value are valid SSLSessionOption values.
        let status = unsafe { SSLSetSessionOption(ctx, SSL_OPT_BREAK_ON_SERVER_AUTH, 1) };
        if status != NO_ERR {
            // SAFETY: ctx is non-null and was just created; not freed elsewhere.
            unsafe { CFRelease(ctx as *const c_void) };
            return Err(Error(format!("SSLSetSessionOption failed: {status}")));
        }

        // Only set a minimum: SecureTransport then negotiates its highest version, and setting
        // kTLSProtocol13 as maximum fails with errSSLIllegalParam.
        // SAFETY: ctx is non-null; K_TLS_PROTOCOL_12 is a valid SSLProtocol value.
        let status = unsafe { SSLSetProtocolVersionMin(ctx, K_TLS_PROTOCOL_12) };
        if status != NO_ERR {
            // SAFETY: ctx is non-null; not freed elsewhere.
            unsafe { CFRelease(ctx as *const c_void) };
            return Err(Error(format!("SSLSetProtocolVersionMin failed: {status}")));
        }

        // Store stream in a Box to ensure a stable heap address for the lifetime of the SSL context
        let stream = Box::new(stream);
        let io = Box::new(IoFuncs {
            stream_ptr: stream.as_ref() as *const S as *mut c_void,
            read_fn: do_read::<S>,
            write_fn: do_write::<S>,
        });

        // Register I/O callbacks - IOFuncs must be set before Connection
        // SAFETY: ctx is non-null; callback signatures match SSLReadFunc/SSLWriteFunc.
        let status = unsafe { SSLSetIOFuncs(ctx, ssl_read_cb, ssl_write_cb) };
        if status != NO_ERR {
            // SSLSetIOFuncs failed so IO funcs are not registered; CFRelease alone suffices.
            // SAFETY: ctx is non-null; not freed elsewhere.
            unsafe { CFRelease(ctx as *const c_void) };
            return Err(Error(format!("SSLSetIOFuncs failed: {status}")));
        }
        // SAFETY: ctx is non-null; io.as_ref() is a stable heap pointer for the SSL context lifetime.
        let status =
            unsafe { SSLSetConnection(ctx, io.as_ref() as *const IoFuncs as SSLConnectionRef) };
        if status != NO_ERR {
            // IO funcs are registered; Apple docs require SSLClose before CFRelease in this state.
            // SAFETY: ctx is non-null with registered IO funcs.
            unsafe {
                SSLClose(ctx);
                CFRelease(ctx as *const c_void);
            }
            return Err(Error(format!("SSLSetConnection failed: {status}")));
        }

        // Set SNI
        let domain_bytes = domain.as_bytes();
        // SAFETY: ctx is non-null; domain_bytes pointer/length are valid for the call duration.
        let status = unsafe {
            SSLSetPeerDomainName(
                ctx,
                domain_bytes.as_ptr() as *const c_char,
                domain_bytes.len(),
            )
        };
        if status != NO_ERR {
            // IO funcs and connection are registered; SSLClose is required before CFRelease.
            // SAFETY: ctx is non-null with registered IO funcs and connection.
            unsafe {
                SSLClose(ctx);
                CFRelease(ctx as *const c_void);
            }
            return Err(Error(format!("SSLSetPeerDomainName failed: {status}")));
        }

        // Perform handshake loop; handle trust evaluation break-point
        loop {
            // SAFETY: ctx is non-null and fully configured.
            let status = unsafe { SSLHandshake(ctx) };
            match status {
                NO_ERR => break,
                ERR_SSL_WOULD_BLOCK => continue,
                ERR_SSL_PEER_AUTH_COMPLETED => {
                    // Server cert is ready; evaluate trust manually using the modern API
                    let mut trust: SecTrustRef = std::ptr::null_mut();
                    // SAFETY: ctx is non-null; &mut trust is a valid output pointer.
                    let st = unsafe { SSLCopyPeerTrust(ctx, &mut trust) };
                    if st != NO_ERR || trust.is_null() {
                        // SAFETY: ctx is non-null with an active SSL connection.
                        unsafe {
                            SSLClose(ctx);
                            CFRelease(ctx as *const c_void);
                        }
                        return Err(Error(format!("SSLCopyPeerTrust failed: {st}")));
                    }
                    let mut cf_error: CFErrorRef = std::ptr::null_mut();
                    // SAFETY: trust is non-null (checked above); &mut cf_error is a valid output pointer.
                    let trusted = unsafe { SecTrustEvaluateWithError(trust, &mut cf_error) };
                    // SAFETY: trust is a non-null CF object returned by SSLCopyPeerTrust.
                    unsafe { CFRelease(trust) };
                    if !cf_error.is_null() {
                        // SAFETY: cf_error is non-null.
                        unsafe { CFRelease(cf_error) };
                    }
                    if trusted == 0 && !self.accept_invalid_certs {
                        // SAFETY: ctx is non-null with an active SSL connection.
                        unsafe {
                            SSLClose(ctx);
                            CFRelease(ctx as *const c_void);
                        }
                        return Err(Error("Certificate trust validation failed".to_string()));
                    }
                    // Trust OK; resume handshake
                    continue;
                }
                _ => {
                    // SAFETY: ctx is non-null with an active SSL connection.
                    unsafe {
                        SSLClose(ctx);
                        CFRelease(ctx as *const c_void);
                    }
                    return Err(Error(format!("TLS handshake failed: {status}")));
                }
            }
        }

        Ok(TlsStream { ctx, io, stream })
    }
}

// MARK: TlsStream
/// A TLS stream backed by SecureTransport
pub struct TlsStream<S> {
    ctx: SSLContextRef,
    // Kept alive as the SSL connection reference pointer; must not be moved or dropped before ctx
    #[expect(dead_code)]
    io: Box<IoFuncs>,
    stream: Box<S>,
}

// SAFETY: SSLContextRef is safe to transfer between threads after the handshake completes.
// IoFuncs aliases stream only through &mut self, so exclusive access is enforced by the borrow checker.
unsafe impl<S: Send> Send for TlsStream<S> {}

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

impl<S: Read + Write> Read for TlsStream<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut processed = 0usize;
        // SAFETY: self.ctx is non-null; buf is a valid mutable slice for its length.
        let status = unsafe {
            SSLRead(
                self.ctx,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                &mut processed,
            )
        };
        // SSLRead may return an error and data simultaneously; prioritize data
        if processed > 0 {
            return Ok(processed);
        }
        match status {
            NO_ERR => Ok(processed),
            ERR_SSL_CLOSED_GRACEFUL | ERR_SSL_CLOSED_ABORT | ERR_SSL_CLOSED_NO_NOTIFY => Ok(0),
            ERR_SSL_WOULD_BLOCK => {
                Err(io::Error::new(io::ErrorKind::WouldBlock, "TLS would block"))
            }
            _ => Err(io::Error::other(format!("SSLRead error: {status}"))),
        }
    }
}

impl<S: Read + Write> Write for TlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut processed = 0usize;
        // SAFETY: self.ctx is non-null; buf is a valid slice for its length.
        let status = unsafe {
            SSLWrite(
                self.ctx,
                buf.as_ptr() as *const c_void,
                buf.len(),
                &mut processed,
            )
        };
        match status {
            NO_ERR => Ok(processed),
            ERR_SSL_WOULD_BLOCK => {
                if processed > 0 {
                    Ok(processed)
                } else {
                    Err(io::Error::new(io::ErrorKind::WouldBlock, "TLS would block"))
                }
            }
            _ => Err(io::Error::other(format!("SSLWrite error: {status}"))),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<S> Drop for TlsStream<S> {
    fn drop(&mut self) {
        // SAFETY: self.ctx is non-null; Drop is called at most once.
        unsafe {
            SSLClose(self.ctx);
            CFRelease(self.ctx as *const c_void);
        }
    }
}
