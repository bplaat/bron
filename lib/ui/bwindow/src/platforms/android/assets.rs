/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CString, c_char, c_int, c_void};
use std::io;

#[link(name = "android")]
unsafe extern "C" {
    fn AAssetManager_open(manager: *mut c_void, name: *const c_char, mode: c_int) -> *mut c_void;
    fn AAsset_getLength64(asset: *mut c_void) -> i64;
    fn AAsset_read(asset: *mut c_void, buffer: *mut c_void, count: usize) -> c_int;
    fn AAsset_close(asset: *mut c_void);
}

struct Asset(*mut c_void);

impl Drop for Asset {
    fn drop(&mut self) {
        // SAFETY: Asset owns a non-null handle returned by AAssetManager_open.
        unsafe { AAsset_close(self.0) };
    }
}

pub(super) fn read(manager: *mut c_void, name: &str) -> io::Result<Vec<u8>> {
    if manager.is_null() {
        return Err(io::Error::other("The Android activity is not initialized"));
    }
    let name =
        CString::new(name).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    // SAFETY: manager belongs to the live NativeActivity; name is a null-terminated C string.
    let handle = unsafe { AAssetManager_open(manager, name.as_ptr(), 2) };
    if handle.is_null() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "APK asset not found",
        ));
    }
    let asset = Asset(handle);
    // SAFETY: asset owns a live Android asset handle.
    let length = usize::try_from(unsafe { AAsset_getLength64(asset.0) })
        .map_err(|error| io::Error::other(format!("Invalid APK asset length: {error}")))?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(io::Error::other)?;
    bytes.resize(length, 0);
    let mut offset = 0;
    while offset < length {
        let count = (length - offset).min(c_int::MAX as usize);
        // SAFETY: the handle is live and the buffer has count writable bytes at offset.
        let read = unsafe { AAsset_read(asset.0, bytes.as_mut_ptr().add(offset).cast(), count) };
        if read <= 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Failed to read APK asset",
            ));
        }
        offset += read as usize;
    }
    Ok(bytes)
}
