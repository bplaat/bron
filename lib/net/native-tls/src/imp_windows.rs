/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Windows TLS implementation using SChannel (SSPI)

use std::ffi::c_void;
use std::io::{self, Read, Write};
use std::slice;

use crate::{Error, HandshakeError};

// MARK: SSPI types
#[repr(C)]
struct SecHandle {
    lower: usize,
    upper: usize,
}

impl SecHandle {
    const INVALID: Self = Self {
        lower: usize::MAX,
        upper: usize::MAX,
    };

    const fn is_invalid(&self) -> bool {
        self.lower == usize::MAX && self.upper == usize::MAX
    }
}

type CredHandle = SecHandle;
type CtxtHandle = SecHandle;

#[repr(C)]
struct TimeStamp {
    low: u32,
    high: u32, // FILETIME uses DWORD (u32) for both halves
}

#[repr(C)]
struct SecBuffer {
    cb_buffer: u32,
    buffer_type: u32,
    pv_buffer: *mut c_void,
}

#[repr(C)]
struct SecBufferDesc {
    ul_version: u32,
    c_buffers: u32,
    p_buffers: *mut SecBuffer,
}

#[repr(C)]
#[derive(Default)]
struct SecPkgContextStreamSizes {
    cb_header: u32,
    cb_trailer: u32,
    cb_max_message: u32,
    c_buffers: u32,
    cb_block_size: u32,
}

// SCHANNEL_CRED (version 4 for compatibility with Windows 7+)
#[repr(C)]
struct SchannelCred {
    dw_version: u32,
    c_creds: u32,
    pa_cred: *mut *mut c_void,
    h_root_store: *mut c_void,
    c_mappers: u32,
    aph_mappers: *mut *mut c_void,
    c_supported_algs: u32,
    palg_supported_algs: *mut u32,
    grbit_enabled_protocols: u32,
    dw_minimum_cipher_strength: u32,
    dw_maximum_cipher_strength: u32,
    dw_session_lifespan: u32,
    dw_flags: u32,
    dw_credentials_format: u32,
}

// MARK: Constants
const SCHANNEL_CRED_VERSION: u32 = 4;
const SECPKG_CRED_OUTBOUND: u32 = 2;
const SECBUFFER_EMPTY: u32 = 0;
const SECBUFFER_DATA: u32 = 1;
const SECBUFFER_TOKEN: u32 = 2;
const SECBUFFER_EXTRA: u32 = 5;
const SECBUFFER_STREAM_TRAILER: u32 = 6;
const SECBUFFER_STREAM_HEADER: u32 = 7;
const SECBUFFER_VERSION: u32 = 0;
const SECPKG_ATTR_STREAM_SIZES: u32 = 4;
const SCH_CRED_AUTO_CRED_VALIDATION: u32 = 0x00000020;
const SCH_CRED_MANUAL_CRED_VALIDATION: u32 = 0x00000008;
const SCH_CRED_NO_DEFAULT_CREDS: u32 = 0x00000010;
const SP_PROT_TLS1_2_CLIENT: u32 = 0x00000800;
const SP_PROT_TLS1_3_CLIENT: u32 = 0x00002000;

const ISC_REQ_SEQUENCE_DETECT: u32 = 0x00000008;
const ISC_REQ_REPLAY_DETECT: u32 = 0x00000004;
const ISC_REQ_CONFIDENTIALITY: u32 = 0x00000010;
const ISC_REQ_EXTENDED_ERROR: u32 = 0x00004000;
const ISC_REQ_ALLOCATE_MEMORY: u32 = 0x00000100;
const ISC_REQ_STREAM: u32 = 0x00008000;
const ISC_FLAGS: u32 = ISC_REQ_SEQUENCE_DETECT
    | ISC_REQ_REPLAY_DETECT
    | ISC_REQ_CONFIDENTIALITY
    | ISC_REQ_EXTENDED_ERROR
    | ISC_REQ_ALLOCATE_MEMORY
    | ISC_REQ_STREAM;

const SEC_E_OK: i32 = 0;
const SEC_I_CONTINUE_NEEDED: i32 = 0x00090312u32 as i32;
const SEC_E_INCOMPLETE_MESSAGE: i32 = 0x80090318u32 as i32;

/// Maximum ciphertext bytes read from the stream at once, one maximum size TLS record
const READ_CHUNK_SIZE: usize = 16384;

/// Encode a string as a null-terminated UTF-16 string.
pub(crate) fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// Encode a string literal as a null-terminated UTF-16 string at compile time.
macro_rules! w {
    ($text:literal) => {{
        const TEXT: &str = $text;
        const LEN: usize = {
            let bytes = TEXT.as_bytes();
            let mut len = 1;
            let mut i = 0;
            while i < bytes.len() {
                if bytes[i] & 0xc0 != 0x80 {
                    len += if bytes[i] >= 0xf0 { 2 } else { 1 };
                }
                i += 1;
            }
            len
        };
        const WIDE: &[u16] = &{
            let bytes = TEXT.as_bytes();
            let mut result = [0; LEN];
            let mut i = 0;
            let mut j = 0;
            while i < bytes.len() {
                let first = bytes[i];
                let (mut code, extra) = match first {
                    0..=0x7f => (first as u32, 0),
                    0xc0..=0xdf => ((first & 0x1f) as u32, 1),
                    0xe0..=0xef => ((first & 0x0f) as u32, 2),
                    _ => ((first & 0x07) as u32, 3),
                };
                let end = i + extra + 1;
                i += 1;
                while i < end {
                    code = (code << 6) | (bytes[i] & 0x3f) as u32;
                    i += 1;
                }
                assert!(code != 0, "wide string literal contains a null character");
                if code > 0xffff {
                    code -= 0x10000;
                    result[j] = 0xd800 | (code >> 10) as u16;
                    j += 1;
                    result[j] = 0xdc00 | (code & 0x3ff) as u16;
                } else {
                    result[j] = code as u16;
                }
                j += 1;
            }
            result
        };
        WIDE
    }};
}

const SCHANNEL_NAME_W: &[u16] = w!("Schannel");

// MARK: SSPI FFI
#[link(name = "Secur32")]
#[link(name = "Crypt32")]
unsafe extern "system" {
    fn AcquireCredentialsHandleW(
        principal: *const u16,
        package: *const u16,
        credential_use: u32,
        logon_id: *const c_void,
        auth_data: *mut c_void,
        get_key_fn: *const c_void,
        get_key_arg: *const c_void,
        credential: *mut CredHandle,
        expiry: *mut TimeStamp,
    ) -> i32;
    fn InitializeSecurityContextW(
        credential: *mut CredHandle,
        context: *mut CtxtHandle,
        target_name: *const u16,
        context_req: u32,
        reserved1: u32,
        target_data_rep: u32,
        input: *mut SecBufferDesc,
        reserved2: u32,
        new_context: *mut CtxtHandle,
        output: *mut SecBufferDesc,
        context_attr: *mut u32,
        expiry: *mut TimeStamp,
    ) -> i32;
    fn FreeContextBuffer(ctx_buf: *mut c_void) -> i32;
    fn DeleteSecurityContext(context: *mut CtxtHandle) -> i32;
    fn FreeCredentialsHandle(credential: *mut CredHandle) -> i32;
    fn EncryptMessage(context: *mut CtxtHandle, qop: u32, msg: *mut SecBufferDesc, seq: u32)
    -> i32;
    fn DecryptMessage(
        context: *mut CtxtHandle,
        msg: *mut SecBufferDesc,
        seq: u32,
        qop: *mut u32,
    ) -> i32;
    fn QueryContextAttributesW(context: *mut CtxtHandle, attr: u32, buf: *mut c_void) -> i32;
}

struct SecurityHandles {
    cred: CredHandle,
    ctx: CtxtHandle,
}

impl Drop for SecurityHandles {
    fn drop(&mut self) {
        // SAFETY: These handles are uniquely owned. A partially established context must also
        // be deleted, before releasing the credentials it uses.
        unsafe {
            if !self.ctx.is_invalid() {
                DeleteSecurityContext(&mut self.ctx);
            }
            if !self.cred.is_invalid() {
                FreeCredentialsHandle(&mut self.cred);
            }
        }
    }
}

struct ContextBuffer(*mut c_void);

impl Drop for ContextBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: SChannel transfers ownership of each output token to the caller.
            unsafe { FreeContextBuffer(self.0) };
        }
    }
}

fn acquire_cred(accept_invalid_certs: bool) -> Result<CredHandle, Error> {
    let mut cred = CredHandle::INVALID;
    let mut cred_data = SchannelCred {
        dw_version: SCHANNEL_CRED_VERSION,
        c_creds: 0,
        pa_cred: std::ptr::null_mut(),
        h_root_store: std::ptr::null_mut(),
        c_mappers: 0,
        aph_mappers: std::ptr::null_mut(),
        c_supported_algs: 0,
        palg_supported_algs: std::ptr::null_mut(),
        grbit_enabled_protocols: SP_PROT_TLS1_2_CLIENT | SP_PROT_TLS1_3_CLIENT,
        dw_minimum_cipher_strength: 0,
        dw_maximum_cipher_strength: 0,
        dw_session_lifespan: 0,
        dw_flags: if accept_invalid_certs {
            SCH_CRED_MANUAL_CRED_VALIDATION | SCH_CRED_NO_DEFAULT_CREDS
        } else {
            SCH_CRED_AUTO_CRED_VALIDATION | SCH_CRED_NO_DEFAULT_CREDS
        },
        dw_credentials_format: 0,
    };
    let mut expiry = TimeStamp { low: 0, high: 0 };
    // SAFETY: all pointer arguments are either null (meaning "default") or point to valid local variables.
    let status = unsafe {
        AcquireCredentialsHandleW(
            std::ptr::null(),
            SCHANNEL_NAME_W.as_ptr(),
            SECPKG_CRED_OUTBOUND,
            std::ptr::null(),
            &mut cred_data as *mut SchannelCred as *mut c_void,
            std::ptr::null(),
            std::ptr::null(),
            &mut cred,
            &mut expiry,
        )
    };
    if status != SEC_E_OK {
        return Err(Error(format!(
            "AcquireCredentialsHandleW failed: 0x{:08x}",
            status as u32
        )));
    }
    Ok(cred)
}

fn do_handshake<S: Read + Write>(
    handles: &mut SecurityHandles,
    domain: &[u16],
    stream: &mut S,
) -> Result<Vec<u8>, Error> {
    let SecurityHandles { cred, ctx } = handles;
    let mut in_data: Vec<u8> = Vec::new();
    let mut first_call = true;

    loop {
        // Prepare output buffers (ISC_REQ_ALLOCATE_MEMORY = SChannel allocates)
        let mut out_buf = SecBuffer {
            cb_buffer: 0,
            buffer_type: SECBUFFER_TOKEN,
            pv_buffer: std::ptr::null_mut(),
        };
        let mut out_desc = SecBufferDesc {
            ul_version: SECBUFFER_VERSION,
            c_buffers: 1,
            p_buffers: &mut out_buf,
        };
        let mut ctx_attrs: u32 = 0;
        let mut expiry = TimeStamp { low: 0, high: 0 };

        let status = if first_call {
            // SAFETY: cred is a valid credential handle; null context on first call is per-spec.
            // out_desc, ctx_attrs, expiry are valid output locations. ISC_FLAGS are valid flags.
            unsafe {
                InitializeSecurityContextW(
                    cred,
                    std::ptr::null_mut(),
                    domain.as_ptr(),
                    ISC_FLAGS,
                    0,
                    0,
                    std::ptr::null_mut(),
                    0,
                    ctx,
                    &mut out_desc,
                    &mut ctx_attrs,
                    &mut expiry,
                )
            }
        } else {
            let mut in_bufs = [
                SecBuffer {
                    cb_buffer: in_data.len() as u32,
                    buffer_type: SECBUFFER_TOKEN,
                    pv_buffer: in_data.as_mut_ptr() as *mut c_void,
                },
                SecBuffer {
                    cb_buffer: 0,
                    buffer_type: SECBUFFER_EMPTY,
                    pv_buffer: std::ptr::null_mut(),
                },
            ];
            let mut in_desc = SecBufferDesc {
                ul_version: SECBUFFER_VERSION,
                c_buffers: 2,
                p_buffers: in_bufs.as_mut_ptr(),
            };
            // SAFETY: cred and ctx are valid handles; in_desc/out_desc/ctx_attrs/expiry are valid output locations.
            let status = unsafe {
                InitializeSecurityContextW(
                    cred,
                    ctx,
                    domain.as_ptr(),
                    ISC_FLAGS,
                    0,
                    0,
                    &mut in_desc,
                    0,
                    ctx,
                    &mut out_desc,
                    &mut ctx_attrs,
                    &mut expiry,
                )
            };
            // Retain unconsumed input bytes (SECBUFFER_EXTRA) for the next ISC call.
            // This is critical in TLS 1.3 where the server bundles multiple records
            // into one flight and SChannel may only process one record per call.
            // For SEC_E_INCOMPLETE_MESSAGE keep all of in_data so we can append more.
            if status != SEC_E_INCOMPLETE_MESSAGE {
                if in_bufs[1].buffer_type == SECBUFFER_EXTRA && in_bufs[1].cb_buffer > 0 {
                    let extra_len = in_bufs[1].cb_buffer as usize;
                    let total_len = in_data.len();
                    in_data.drain(..total_len - extra_len);
                } else {
                    in_data.clear();
                }
            }
            status
        };
        first_call = false;

        // Release the token on every path, including an I/O error or an empty output.
        let token = ContextBuffer(out_buf.pv_buffer);
        if !token.0.is_null() && out_buf.cb_buffer > 0 {
            // SAFETY: SChannel allocated out_buf.pv_buffer (ISC_REQ_ALLOCATE_MEMORY) and
            // cb_buffer bytes are valid; FreeContextBuffer releases SChannel-allocated memory.
            let data =
                unsafe { slice::from_raw_parts(token.0 as *const u8, out_buf.cb_buffer as usize) };
            stream.write_all(data).map_err(|e| Error(e.to_string()))?;
        }
        drop(token);

        match status {
            SEC_E_OK => return Ok(in_data),
            SEC_I_CONTINUE_NEEDED => {
                // Only read from the network when there is no leftover data from
                // SECBUFFER_EXTRA; if there is, loop immediately with that data.
                if in_data.is_empty() {
                    read_handshake_data(stream, &mut in_data)?;
                }
            }
            // Need more data - append to existing buffer
            SEC_E_INCOMPLETE_MESSAGE => read_handshake_data(stream, &mut in_data)?,
            _ => {
                return Err(Error(format!(
                    "TLS handshake error: 0x{:08x}",
                    status as u32
                )));
            }
        }
    }
}

/// Reads up to `max` more bytes from the stream directly into the end of `buffer`
fn read_append<S: Read>(stream: &mut S, buffer: &mut Vec<u8>, max: usize) -> io::Result<usize> {
    let len = buffer.len();
    buffer.resize(len + max, 0);
    let read = stream.read(&mut buffer[len..]);
    buffer.truncate(len + read.as_ref().map_or(0, |n| *n));
    read
}

fn read_handshake_data<S: Read>(stream: &mut S, in_data: &mut Vec<u8>) -> Result<(), Error> {
    match read_append(stream, in_data, READ_CHUNK_SIZE) {
        Ok(0) => Err(Error("Connection closed during TLS handshake".to_string())),
        Ok(_) => Ok(()),
        Err(err) => Err(Error(err.to_string())),
    }
}

fn query_stream_sizes(ctx: &mut CtxtHandle) -> Result<SecPkgContextStreamSizes, Error> {
    let mut sizes = SecPkgContextStreamSizes::default();
    // SAFETY: ctx is a valid context handle; &mut sizes is a valid output location for the attribute.
    let status = unsafe {
        QueryContextAttributesW(
            ctx,
            SECPKG_ATTR_STREAM_SIZES,
            &mut sizes as *mut SecPkgContextStreamSizes as *mut c_void,
        )
    };
    if status != SEC_E_OK {
        return Err(Error(format!(
            "QueryContextAttributesW failed: 0x{:08x}",
            status as u32
        )));
    }
    Ok(sizes)
}

// MARK: TlsConnector
/// A TLS connector using SChannel
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
        let mut handles = SecurityHandles {
            cred: acquire_cred(self.accept_invalid_certs)?,
            ctx: CtxtHandle::INVALID,
        };
        let domain = wide(domain);
        let mut stream = stream;
        let enc_buf = do_handshake(&mut handles, &domain, &mut stream)?;
        let sizes = query_stream_sizes(&mut handles.ctx)?;
        Ok(TlsStream {
            handles,
            stream,
            sizes,
            enc_buf,
            dec_buf: Vec::new(),
        })
    }
}

// MARK: TlsStream
/// A TLS stream backed by SChannel
pub struct TlsStream<S> {
    handles: SecurityHandles,
    stream: S,
    sizes: SecPkgContextStreamSizes,
    enc_buf: Vec<u8>, // buffered ciphertext read from stream
    dec_buf: Vec<u8>, // leftover plaintext after a decrypt
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
        // Drain any leftover plaintext
        if !self.dec_buf.is_empty() {
            let n = buf.len().min(self.dec_buf.len());
            buf[..n].copy_from_slice(&self.dec_buf[..n]);
            self.dec_buf.drain(..n);
            return Ok(n);
        }

        loop {
            // Try to decrypt what we have buffered
            if !self.enc_buf.is_empty() {
                let mut bufs = [
                    SecBuffer {
                        cb_buffer: self.enc_buf.len() as u32,
                        buffer_type: SECBUFFER_DATA,
                        pv_buffer: self.enc_buf.as_mut_ptr() as *mut c_void,
                    },
                    SecBuffer {
                        cb_buffer: 0,
                        buffer_type: SECBUFFER_EMPTY,
                        pv_buffer: std::ptr::null_mut(),
                    },
                    SecBuffer {
                        cb_buffer: 0,
                        buffer_type: SECBUFFER_EMPTY,
                        pv_buffer: std::ptr::null_mut(),
                    },
                    SecBuffer {
                        cb_buffer: 0,
                        buffer_type: SECBUFFER_EMPTY,
                        pv_buffer: std::ptr::null_mut(),
                    },
                ];
                let mut desc = SecBufferDesc {
                    ul_version: SECBUFFER_VERSION,
                    c_buffers: 4,
                    p_buffers: bufs.as_mut_ptr(),
                };
                // SAFETY: self.ctx is a valid context handle; bufs[0].pv_buffer points into
                // self.enc_buf which is alive for this call; the other buffers are empty outputs.
                let status = unsafe {
                    DecryptMessage(&mut self.handles.ctx, &mut desc, 0, std::ptr::null_mut())
                };

                match status {
                    SEC_E_OK => {
                        // DecryptMessage operates in-place, so the SECBUFFER_DATA plaintext and
                        // SECBUFFER_EXTRA leftover ciphertext are ranges of self.enc_buf.
                        let base = self.enc_buf.as_ptr().addr();
                        let range = |buffer_type| {
                            bufs.iter()
                                .find(|b| b.buffer_type == buffer_type && !b.pv_buffer.is_null())
                                .map(|b| {
                                    let start = b.pv_buffer.addr() - base;
                                    start..start + b.cb_buffer as usize
                                })
                        };
                        let data = range(SECBUFFER_DATA).unwrap_or_default();
                        let n = buf.len().min(data.len());
                        buf[..n].copy_from_slice(&self.enc_buf[data.start..data.start + n]);
                        self.dec_buf
                            .extend_from_slice(&self.enc_buf[data.start + n..data.end]);
                        match range(SECBUFFER_EXTRA) {
                            Some(extra) => {
                                let len = extra.len();
                                self.enc_buf.copy_within(extra, 0);
                                self.enc_buf.truncate(len);
                            }
                            None => self.enc_buf.clear(),
                        }
                        return Ok(n);
                    }
                    SEC_E_INCOMPLETE_MESSAGE => {
                        // Need more ciphertext - fall through to read more
                    }
                    _ => {
                        return Err(io::Error::other(format!(
                            "DecryptMessage failed: 0x{:08x}",
                            status as u32
                        )));
                    }
                }
            }

            // Read more ciphertext from the underlying stream
            if read_append(&mut self.stream, &mut self.enc_buf, READ_CHUNK_SIZE)? == 0 {
                return Ok(0);
            }
        }
    }
}

impl<S: Read + Write> Write for TlsStream<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Write at most max_message bytes per call
        let chunk_len = buf.len().min(self.sizes.cb_max_message as usize);
        let chunk = &buf[..chunk_len];

        let header_sz = self.sizes.cb_header as usize;
        let trailer_sz = self.sizes.cb_trailer as usize;
        let total = header_sz + chunk_len + trailer_sz;
        let mut enc_buf = vec![0u8; total];

        // Copy plaintext after the header area
        enc_buf[header_sz..header_sz + chunk_len].copy_from_slice(chunk);

        let mut bufs = [
            SecBuffer {
                cb_buffer: self.sizes.cb_header,
                buffer_type: SECBUFFER_STREAM_HEADER,
                pv_buffer: enc_buf.as_mut_ptr() as *mut c_void,
            },
            SecBuffer {
                cb_buffer: chunk_len as u32,
                buffer_type: SECBUFFER_DATA,
                pv_buffer: enc_buf[header_sz..].as_mut_ptr() as *mut c_void,
            },
            SecBuffer {
                cb_buffer: self.sizes.cb_trailer,
                buffer_type: SECBUFFER_STREAM_TRAILER,
                pv_buffer: enc_buf[header_sz + chunk_len..].as_mut_ptr() as *mut c_void,
            },
            SecBuffer {
                cb_buffer: 0,
                buffer_type: SECBUFFER_EMPTY,
                pv_buffer: std::ptr::null_mut(),
            },
        ];
        let mut desc = SecBufferDesc {
            ul_version: SECBUFFER_VERSION,
            c_buffers: 4,
            p_buffers: bufs.as_mut_ptr(),
        };

        // SAFETY: self.ctx is a valid context handle; all buffer pointers are into enc_buf which
        // is alive for the duration of the call; EncryptMessage operates in-place.
        let status = unsafe { EncryptMessage(&mut self.handles.ctx, 0, &mut desc, 0) };
        if status != SEC_E_OK {
            return Err(io::Error::other(format!(
                "EncryptMessage failed: 0x{:08x}",
                status as u32
            )));
        }

        // Write header + data + trailer (all in enc_buf, EncryptMessage modifies in-place)
        let enc_size =
            bufs[0].cb_buffer as usize + bufs[1].cb_buffer as usize + bufs[2].cb_buffer as usize;
        self.stream.write_all(&enc_buf[..enc_size])?;
        Ok(chunk_len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingStream {
        fail_write: bool,
    }

    impl Read for FailingStream {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read failed"))
        }
    }

    impl Write for FailingStream {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            if self.fail_write {
                Err(io::Error::other("write failed"))
            } else {
                Ok(data.len())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn handshake_propagates_transport_errors() {
        let connector = TlsConnector::new().expect("TLS connector");
        for fail_write in [false, true] {
            let Err(error) = connector.connect_inner("localhost", FailingStream { fail_write })
            else {
                panic!("failing stream completed its handshake");
            };
            assert_eq!(
                error.0,
                if fail_write {
                    "write failed"
                } else {
                    "read failed"
                }
            );
        }
    }
}
