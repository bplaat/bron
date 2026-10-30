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

// "Schannel\0" as UTF-16
const SCHANNEL_NAME_W: &[u16] = &[
    b'S' as u16,
    b'c' as u16,
    b'h' as u16,
    b'a' as u16,
    b'n' as u16,
    b'n' as u16,
    b'e' as u16,
    b'l' as u16,
    0,
];

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

fn to_utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
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
    cred: &mut CredHandle,
    domain: &[u16],
    stream: &mut S,
) -> Result<(CtxtHandle, Vec<u8>), Error> {
    let mut ctx = CtxtHandle::INVALID;
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
                    &mut ctx,
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
                    &mut ctx,
                    domain.as_ptr(),
                    ISC_FLAGS,
                    0,
                    0,
                    &mut in_desc,
                    0,
                    &mut ctx,
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

        // Send output token if any
        if !out_buf.pv_buffer.is_null() && out_buf.cb_buffer > 0 {
            // SAFETY: SChannel allocated out_buf.pv_buffer (ISC_REQ_ALLOCATE_MEMORY) and
            // cb_buffer bytes are valid; FreeContextBuffer releases SChannel-allocated memory.
            let data = unsafe {
                slice::from_raw_parts(out_buf.pv_buffer as *const u8, out_buf.cb_buffer as usize)
            };
            stream.write_all(data).map_err(|e| Error(e.to_string()))?;
            // SAFETY: out_buf.pv_buffer is non-null and was allocated by SChannel.
            unsafe { FreeContextBuffer(out_buf.pv_buffer) };
        }

        match status {
            SEC_E_OK => return Ok((ctx, in_data)),
            SEC_I_CONTINUE_NEEDED => {
                // Only read from the network when there is no leftover data from
                // SECBUFFER_EXTRA; if there is, loop immediately with that data.
                if in_data.is_empty() {
                    let mut chunk = vec![0u8; 16384];
                    let n = stream.read(&mut chunk).map_err(|e| Error(e.to_string()))?;
                    if n == 0 {
                        return Err(Error("Connection closed during TLS handshake".to_string()));
                    }
                    in_data.extend_from_slice(&chunk[..n]);
                }
            }
            SEC_E_INCOMPLETE_MESSAGE => {
                // Need more data - append to existing buffer
                let mut chunk = vec![0u8; 4096];
                let n = stream.read(&mut chunk).map_err(|e| Error(e.to_string()))?;
                if n == 0 {
                    return Err(Error("Connection closed during TLS handshake".to_string()));
                }
                in_data.extend_from_slice(&chunk[..n]);
            }
            _ => {
                if !ctx.is_invalid() {
                    // SAFETY: ctx is a valid (non-invalid) context handle.
                    unsafe { DeleteSecurityContext(&mut ctx) };
                }
                return Err(Error(format!(
                    "TLS handshake error: 0x{:08x}",
                    status as u32
                )));
            }
        }
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
        let mut cred = acquire_cred(self.accept_invalid_certs)?;
        let domain = to_utf16(domain);
        let mut stream = stream;
        let (ctx, enc_buf) = do_handshake(&mut cred, &domain, &mut stream)?;
        let mut ctx = ctx;
        let sizes = query_stream_sizes(&mut ctx)?;
        Ok(TlsStream {
            cred,
            ctx,
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
    cred: CredHandle,
    ctx: CtxtHandle,
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
                let status =
                    unsafe { DecryptMessage(&mut self.ctx, &mut desc, 0, std::ptr::null_mut()) };

                match status {
                    SEC_E_OK => {
                        // Extract decrypted data and leftover extra data.
                        // SECBUFFER_DATA and SECBUFFER_EXTRA pv_buffers point into self.enc_buf
                        // (DecryptMessage operates in-place), so we copy before replacing enc_buf.
                        let mut dec_data = Vec::new();
                        let mut extra_data = Vec::new();
                        for b in &bufs {
                            if b.buffer_type == SECBUFFER_DATA && !b.pv_buffer.is_null() {
                                // SAFETY: pv_buffer points into enc_buf (in-place decryption); cb_buffer bytes are valid.
                                dec_data.extend_from_slice(unsafe {
                                    slice::from_raw_parts(
                                        b.pv_buffer as *const u8,
                                        b.cb_buffer as usize,
                                    )
                                });
                            }
                            if b.buffer_type == SECBUFFER_EXTRA && !b.pv_buffer.is_null() {
                                // SAFETY: pv_buffer points into enc_buf; cb_buffer bytes are unconsumed ciphertext.
                                extra_data.extend_from_slice(unsafe {
                                    slice::from_raw_parts(
                                        b.pv_buffer as *const u8,
                                        b.cb_buffer as usize,
                                    )
                                });
                            }
                        }
                        self.enc_buf = extra_data;

                        let n = buf.len().min(dec_data.len());
                        buf[..n].copy_from_slice(&dec_data[..n]);
                        if dec_data.len() > n {
                            self.dec_buf.extend_from_slice(&dec_data[n..]);
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
            let mut chunk = vec![0u8; 16384];
            let n = self.stream.read(&mut chunk)?;
            if n == 0 {
                return Ok(0);
            }
            self.enc_buf.extend_from_slice(&chunk[..n]);
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
        let status = unsafe { EncryptMessage(&mut self.ctx, 0, &mut desc, 0) };
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

impl<S> Drop for TlsStream<S> {
    fn drop(&mut self) {
        // SAFETY: ctx and cred are valid handles; Drop is called at most once.
        unsafe {
            DeleteSecurityContext(&mut self.ctx);
            FreeCredentialsHandle(&mut self.cred);
        }
    }
}
