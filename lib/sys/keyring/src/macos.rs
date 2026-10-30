/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use zeroize::Zeroize;

use crate::{Error, Result, platform_failure};

type CfIndex = isize;
type CfTypeRef = *const c_void;
type CfDictionaryRef = *const c_void;
type OsStatus = i32;

const UTF8_ENCODING: u32 = 0x0800_0100;
const ERR_SEC_SUCCESS: OsStatus = 0;
const ERR_SEC_DUPLICATE_ITEM: OsStatus = -25_299;
const ERR_SEC_ITEM_NOT_FOUND: OsStatus = -25_300;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CfTypeRef;

    fn CFStringCreateWithBytes(
        allocator: *const c_void,
        bytes: *const u8,
        length: CfIndex,
        encoding: u32,
        is_external_representation: u8,
    ) -> CfTypeRef;
    fn CFDataCreateMutable(allocator: *const c_void, capacity: CfIndex) -> CfTypeRef;
    fn CFDataAppendBytes(data: CfTypeRef, bytes: *const u8, length: CfIndex);
    fn CFDataGetLength(data: CfTypeRef) -> CfIndex;
    fn CFDataGetBytePtr(data: CfTypeRef) -> *const u8;
    fn CFDataGetMutableBytePtr(data: CfTypeRef) -> *mut u8;
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const CfTypeRef,
        values: *const CfTypeRef,
        count: CfIndex,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> CfDictionaryRef;
    fn CFRelease(object: CfTypeRef);
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecClass: CfTypeRef;
    static kSecClassGenericPassword: CfTypeRef;
    static kSecAttrService: CfTypeRef;
    static kSecAttrAccount: CfTypeRef;
    static kSecValueData: CfTypeRef;
    static kSecReturnData: CfTypeRef;
    static kSecMatchLimit: CfTypeRef;
    static kSecMatchLimitOne: CfTypeRef;

    fn SecItemAdd(attributes: CfDictionaryRef, result: *mut CfTypeRef) -> OsStatus;
    fn SecItemCopyMatching(query: CfDictionaryRef, result: *mut CfTypeRef) -> OsStatus;
    fn SecItemUpdate(query: CfDictionaryRef, attributes: CfDictionaryRef) -> OsStatus;
    fn SecItemDelete(query: CfDictionaryRef) -> OsStatus;
}

struct OwnedCf(CfTypeRef);

impl OwnedCf {
    fn string(value: &str) -> Result<Self> {
        // SAFETY: value points to `value.len()` initialized bytes and Core Foundation copies them.
        let object = unsafe {
            CFStringCreateWithBytes(
                null(),
                value.as_ptr(),
                value.len() as CfIndex,
                UTF8_ENCODING,
                0,
            )
        };
        Self::from_created(object, "create Keychain string")
    }

    fn dictionary(entries: &[(CfTypeRef, CfTypeRef)]) -> Result<Self> {
        let (keys, values): (Vec<_>, Vec<_>) = entries.iter().copied().unzip();
        // SAFETY: both arrays contain `entries.len()` valid object pointers and are copied by Core Foundation.
        let object = unsafe {
            CFDictionaryCreate(
                null(),
                keys.as_ptr(),
                values.as_ptr(),
                entries.len() as CfIndex,
                null(),
                null(),
            )
        };
        Self::from_created(object, "create Keychain query")
    }

    fn from_created(object: CfTypeRef, operation: &str) -> Result<Self> {
        if object.is_null() {
            Err(platform_failure(format!("failed to {operation}")))
        } else {
            Ok(Self(object))
        }
    }
}

struct SensitiveData {
    object: OwnedCf,
    len: usize,
}

impl SensitiveData {
    fn new(value: &[u8]) -> Result<Self> {
        // SAFETY: Core Foundation creates an owned mutable data object.
        let object = unsafe { CFDataCreateMutable(null(), value.len() as CfIndex) };
        let object = OwnedCf::from_created(object, "create Keychain data")?;
        if !value.is_empty() {
            // SAFETY: value contains `value.len()` initialized bytes and the mutable data object is
            // valid for the duration of the call.
            unsafe { CFDataAppendBytes(object.0, value.as_ptr(), value.len() as CfIndex) };
        }
        Ok(Self {
            object,
            len: value.len(),
        })
    }
}

impl Drop for SensitiveData {
    fn drop(&mut self) {
        if self.len == 0 {
            return;
        }
        // SAFETY: this object was created as mutable data and contains `len` initialized bytes.
        let bytes = unsafe {
            std::slice::from_raw_parts_mut(CFDataGetMutableBytePtr(self.object.0), self.len)
        };
        bytes.zeroize();
    }
}

impl Drop for OwnedCf {
    fn drop(&mut self) {
        // SAFETY: OwnedCf is only constructed from create/copy functions returning an owned object.
        unsafe { CFRelease(self.0) };
    }
}

struct EntryQuery {
    service: OwnedCf,
    account: OwnedCf,
}

impl EntryQuery {
    fn new(service: &str, account: &str) -> Result<Self> {
        Ok(Self {
            service: OwnedCf::string(service)?,
            account: OwnedCf::string(account)?,
        })
    }

    fn dictionary(&self) -> Result<OwnedCf> {
        // SAFETY: Security framework constants are valid process-lifetime Core Foundation objects.
        let entries = unsafe {
            [
                (kSecClass, kSecClassGenericPassword),
                (kSecAttrService, self.service.0),
                (kSecAttrAccount, self.account.0),
            ]
        };
        OwnedCf::dictionary(&entries)
    }
}

pub(crate) fn set_secret(service: &str, account: &str, secret: &[u8]) -> Result<()> {
    let entry = EntryQuery::new(service, account)?;
    let query = entry.dictionary()?;
    let data = SensitiveData::new(secret)?;
    // SAFETY: Security framework constants are valid and data remains alive for the call.
    let update = unsafe { OwnedCf::dictionary(&[(kSecValueData, data.object.0)])? };
    // SAFETY: query and update are valid dictionaries for SecItemUpdate.
    let status = unsafe { SecItemUpdate(query.0, update.0) };
    if status == ERR_SEC_SUCCESS {
        return Ok(());
    }
    if status != ERR_SEC_ITEM_NOT_FOUND {
        return Err(status_error("update", status));
    }

    // SAFETY: Security framework constants and all referenced values remain alive for the call.
    let add = unsafe {
        OwnedCf::dictionary(&[
            (kSecClass, kSecClassGenericPassword),
            (kSecAttrService, entry.service.0),
            (kSecAttrAccount, entry.account.0),
            (kSecValueData, data.object.0),
        ])?
    };
    // SAFETY: add is a valid Keychain item dictionary and no result is requested.
    let status = unsafe { SecItemAdd(add.0, null_mut()) };
    if status == ERR_SEC_SUCCESS || status == ERR_SEC_DUPLICATE_ITEM {
        Ok(())
    } else {
        Err(status_error("store", status))
    }
}

pub(crate) fn get_secret(service: &str, account: &str) -> Result<Vec<u8>> {
    let entry = EntryQuery::new(service, account)?;
    // SAFETY: Security and Core Foundation constants are valid process-lifetime objects.
    let query = unsafe {
        OwnedCf::dictionary(&[
            (kSecClass, kSecClassGenericPassword),
            (kSecAttrService, entry.service.0),
            (kSecAttrAccount, entry.account.0),
            (kSecReturnData, kCFBooleanTrue),
            (kSecMatchLimit, kSecMatchLimitOne),
        ])?
    };
    let mut result = null();
    // SAFETY: query is valid and result points to writable storage for an owned result object.
    let status = unsafe { SecItemCopyMatching(query.0, &mut result) };
    if status == ERR_SEC_ITEM_NOT_FOUND {
        return Err(Error::NoEntry);
    }
    if status != ERR_SEC_SUCCESS {
        return Err(status_error("load", status));
    }
    let data = OwnedCf::from_created(result, "load Keychain secret")?;
    // SAFETY: a successful data-returning Keychain query returns a valid CFData object.
    let length = unsafe { CFDataGetLength(data.0) };
    if length == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: the CFData remains alive and exposes `length` bytes.
    let bytes = unsafe { std::slice::from_raw_parts(CFDataGetBytePtr(data.0), length as usize) };
    Ok(bytes.to_vec())
}

pub(crate) fn delete_credential(service: &str, account: &str) -> Result<()> {
    let entry = EntryQuery::new(service, account)?;
    let query = entry.dictionary()?;
    // SAFETY: query is a valid Keychain item query dictionary.
    let status = unsafe { SecItemDelete(query.0) };
    match status {
        ERR_SEC_SUCCESS => Ok(()),
        ERR_SEC_ITEM_NOT_FOUND => Err(Error::NoEntry),
        _ => Err(status_error("delete", status)),
    }
}

fn status_error(operation: &str, status: OsStatus) -> Error {
    platform_failure(format!(
        "failed to {operation} Keychain credential (OSStatus {status})"
    ))
}
