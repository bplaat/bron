/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]
#![allow(non_snake_case)]

use std::ffi::c_void;
use std::ptr::{null_mut, slice_from_raw_parts};

use zeroize::{Zeroize, Zeroizing};

use crate::{Error, Result, platform_failure};

const CRED_TYPE_GENERIC: u32 = 1;
const CRED_PERSIST_LOCAL_MACHINE: u32 = 2;
const ERROR_NOT_FOUND: u32 = 1168;

#[repr(C)]
struct FileTime {
    dwLowDateTime: u32,
    dwHighDateTime: u32,
}

#[repr(C)]
struct CredentialW {
    Flags: u32,
    Type: u32,
    TargetName: *mut u16,
    Comment: *mut u16,
    LastWritten: FileTime,
    CredentialBlobSize: u32,
    CredentialBlob: *mut u8,
    Persist: u32,
    AttributeCount: u32,
    Attributes: *mut c_void,
    TargetAlias: *mut u16,
    UserName: *mut u16,
}

struct OwnedCredential(*mut CredentialW);

impl Drop for OwnedCredential {
    fn drop(&mut self) {
        // SAFETY: this allocation is owned by CredReadW. Its blob is writable for the lifetime of
        // the credential and must be released with CredFree.
        unsafe {
            let credential = &mut *self.0;
            if credential.CredentialBlobSize != 0 {
                std::slice::from_raw_parts_mut(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .zeroize();
            }
            CredFree(self.0.cast());
        }
    }
}

#[link(name = "advapi32")]
unsafe extern "system" {
    fn CredWriteW(credential: *const CredentialW, flags: u32) -> i32;
    fn CredReadW(
        target: *const u16,
        type_: u32,
        flags: u32,
        credential: *mut *mut CredentialW,
    ) -> i32;
    fn CredDeleteW(target: *const u16, type_: u32, flags: u32) -> i32;
    fn CredFree(buffer: *mut c_void);
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetLastError() -> u32;
}

pub(crate) fn set_secret(service: &str, account: &str, secret: &[u8]) -> Result<()> {
    let mut target = wide(&format!("{service}/{account}"));
    let mut username = wide(account);
    let mut secret = Zeroizing::new(secret.to_vec());
    let credential = CredentialW {
        Flags: 0,
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        Comment: null_mut(),
        LastWritten: FileTime {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        },
        CredentialBlobSize: secret.len() as u32,
        CredentialBlob: secret.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        AttributeCount: 0,
        Attributes: null_mut(),
        TargetAlias: null_mut(),
        UserName: username.as_mut_ptr(),
    };
    // SAFETY: all pointers in credential reference live buffers for the duration of the call.
    let succeeded = unsafe { CredWriteW(&credential, 0) };
    if succeeded == 0 {
        Err(last_error("store"))
    } else {
        Ok(())
    }
}

pub(crate) fn get_secret(service: &str, account: &str) -> Result<Vec<u8>> {
    let target = wide(&format!("{service}/{account}"));
    let mut credential = null_mut();
    // SAFETY: target is NUL-terminated and credential points to writable pointer storage.
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
        // SAFETY: GetLastError is called immediately after the failed Win32 operation.
        return match unsafe { GetLastError() } {
            ERROR_NOT_FOUND => Err(Error::NoEntry),
            code => Err(platform_failure(format!(
                "failed to load Windows credential (error {code})"
            ))),
        };
    }
    let credential = OwnedCredential(credential);
    // Empty credentials may use a null blob pointer, which cannot be passed to Rust slice APIs.
    // SAFETY: CredReadW returned a valid credential allocation owned by credential.
    if unsafe { (*credential.0).CredentialBlobSize } == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: CredReadW returned a valid credential allocation with a blob of the declared size.
    let bytes = unsafe {
        &*slice_from_raw_parts(
            (*credential.0).CredentialBlob,
            (*credential.0).CredentialBlobSize as usize,
        )
    };
    Ok(bytes.to_vec())
}

pub(crate) fn delete_credential(service: &str, account: &str) -> Result<()> {
    let target = wide(&format!("{service}/{account}"));
    // SAFETY: target is a valid NUL-terminated UTF-16 string.
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
        return Ok(());
    }
    // SAFETY: GetLastError is called immediately after the failed Win32 operation.
    match unsafe { GetLastError() } {
        ERROR_NOT_FOUND => Err(Error::NoEntry),
        code => Err(platform_failure(format!(
            "failed to delete Windows credential (error {code})"
        ))),
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn last_error(operation: &str) -> Error {
    // SAFETY: GetLastError is called immediately after the failed Win32 operation.
    let code = unsafe { GetLastError() };
    platform_failure(format!(
        "failed to {operation} Windows credential (error {code})"
    ))
}
