/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_void};
use std::ptr::{null, null_mut};

use crate::{Error, Result, platform_failure};

const SECRET_SCHEMA_NONE: i32 = 0;
const SECRET_SCHEMA_ATTRIBUTE_STRING: i32 = 0;

#[repr(C)]
struct SecretSchema([u8; 0]);

#[repr(C)]
struct SecretValue([u8; 0]);

#[repr(C)]
struct GError {
    domain: u32,
    code: i32,
    message: *mut c_char,
}

unsafe extern "C" {
    fn secret_schema_new(name: *const c_char, flags: i32, ...) -> *mut SecretSchema;
    fn secret_schema_unref(schema: *mut SecretSchema);
    fn secret_password_store_binary_sync(
        schema: *const SecretSchema,
        collection: *const c_char,
        label: *const c_char,
        value: *mut SecretValue,
        cancellable: *mut c_void,
        error: *mut *mut GError,
        ...
    ) -> i32;
    fn secret_password_lookup_binary_sync(
        schema: *const SecretSchema,
        cancellable: *mut c_void,
        error: *mut *mut GError,
        ...
    ) -> *mut SecretValue;
    fn secret_password_clear_sync(
        schema: *const SecretSchema,
        cancellable: *mut c_void,
        error: *mut *mut GError,
        ...
    ) -> i32;
    fn secret_value_new(
        secret: *const c_char,
        length: isize,
        content_type: *const c_char,
    ) -> *mut SecretValue;
    fn secret_value_get(value: *mut SecretValue, length: *mut usize) -> *const c_char;
    fn secret_value_unref(value: *mut SecretValue);
    fn g_error_free(error: *mut GError);
}

struct Schema(*mut SecretSchema);

impl Schema {
    fn new() -> Result<Self> {
        // SAFETY: all arguments are correctly typed C varargs and the list is NULL-terminated.
        let schema = unsafe {
            secret_schema_new(
                c"nl.bplaat.keyring.Password".as_ptr(),
                SECRET_SCHEMA_NONE,
                c"service".as_ptr(),
                SECRET_SCHEMA_ATTRIBUTE_STRING,
                c"account".as_ptr(),
                SECRET_SCHEMA_ATTRIBUTE_STRING,
                null::<c_char>(),
            )
        };
        if schema.is_null() {
            Err(platform_failure(
                "failed to create libsecret schema".to_string(),
            ))
        } else {
            Ok(Self(schema))
        }
    }
}

impl Drop for Schema {
    fn drop(&mut self) {
        // SAFETY: Schema owns the reference returned by secret_schema_new.
        unsafe { secret_schema_unref(self.0) };
    }
}

struct OwnedSecretValue(*mut SecretValue);

impl Drop for OwnedSecretValue {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the reference returned by libsecret.
        unsafe { secret_value_unref(self.0) };
    }
}

pub(crate) fn set_secret(service: &str, account: &str, secret: &[u8]) -> Result<()> {
    let schema = Schema::new()?;
    let service = c_string(service)?;
    let account = c_string(account)?;
    // SAFETY: libsecret copies `secret.len()` bytes and the content type is NUL-terminated.
    let value = unsafe {
        secret_value_new(
            secret.as_ptr().cast(),
            secret.len() as isize,
            c"application/octet-stream".as_ptr(),
        )
    };
    if value.is_null() {
        return Err(platform_failure(
            "failed to create libsecret value".to_string(),
        ));
    }
    let value = OwnedSecretValue(value);
    let mut error = null_mut();
    // SAFETY: pointers remain valid for the call, attributes match the schema, and varargs end in NULL.
    let succeeded = unsafe {
        secret_password_store_binary_sync(
            schema.0,
            null(),
            service.as_ptr(),
            value.0,
            null_mut(),
            &mut error,
            c"service".as_ptr(),
            service.as_ptr(),
            c"account".as_ptr(),
            account.as_ptr(),
            null::<c_char>(),
        )
    };
    if !error.is_null() {
        return Err(take_error(error, "store"));
    }
    if succeeded == 0 {
        Err(platform_failure(
            "libsecret failed to store credential".to_string(),
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn get_secret(service: &str, account: &str) -> Result<Vec<u8>> {
    let schema = Schema::new()?;
    let service = c_string(service)?;
    let account = c_string(account)?;
    let mut error = null_mut();
    // SAFETY: pointers remain valid for the call, attributes match the schema, and varargs end in NULL.
    let value = unsafe {
        secret_password_lookup_binary_sync(
            schema.0,
            null_mut(),
            &mut error,
            c"service".as_ptr(),
            service.as_ptr(),
            c"account".as_ptr(),
            account.as_ptr(),
            null::<c_char>(),
        )
    };
    let value = (!value.is_null()).then(|| OwnedSecretValue(value));
    if !error.is_null() {
        return Err(take_error(error, "load"));
    }
    let Some(value) = value else {
        return Err(Error::NoEntry);
    };
    let mut length = 0;
    // SAFETY: value is a valid SecretValue and length points to writable storage.
    let bytes = unsafe { secret_value_get(value.0, &mut length) };
    if length == 0 {
        return Ok(Vec::new());
    }
    if bytes.is_null() {
        return Err(platform_failure(
            "libsecret returned an invalid secret".to_string(),
        ));
    }
    // SAFETY: libsecret reports that bytes points to `length` bytes owned by value.
    Ok(unsafe { std::slice::from_raw_parts(bytes.cast(), length) }.to_vec())
}

pub(crate) fn delete_credential(service: &str, account: &str) -> Result<()> {
    let schema = Schema::new()?;
    let service = c_string(service)?;
    let account = c_string(account)?;
    let mut error = null_mut();
    // SAFETY: pointers remain valid for the call, attributes match the schema, and varargs end in NULL.
    let removed = unsafe {
        secret_password_clear_sync(
            schema.0,
            null_mut(),
            &mut error,
            c"service".as_ptr(),
            service.as_ptr(),
            c"account".as_ptr(),
            account.as_ptr(),
            null::<c_char>(),
        )
    };
    if !error.is_null() {
        return Err(take_error(error, "delete"));
    }
    if removed == 0 {
        Err(Error::NoEntry)
    } else {
        Ok(())
    }
}

fn c_string(value: &str) -> Result<CString> {
    CString::new(value).map_err(platform_failure)
}

fn take_error(error: *mut GError, operation: &str) -> Error {
    // SAFETY: a non-NULL GError from GLib has a valid NUL-terminated message until freed.
    let message = unsafe { CStr::from_ptr((*error).message) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: ownership of this GError was transferred to the caller.
    unsafe { g_error_free(error) };
    platform_failure(format!(
        "failed to {operation} libsecret credential: {message}"
    ))
}
