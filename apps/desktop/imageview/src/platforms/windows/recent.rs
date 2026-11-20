/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Opened images in the system-managed Recent category of the taskbar Jump List.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;

use super::headers::ERROR_INSUFFICIENT_BUFFER;
use super::winrt::{GUID, HRESULT, out};

const IID_ISHELL_ITEM: GUID = GUID::from_u128(0x43826d1e_e718_42ee_bc55_a1e261c37bfe);
const SHARD_PATHW: u32 = 3;
const SHARD_APPIDINFO: u32 = 4;

#[repr(C)]
struct AppIdInfo {
    item: *mut c_void,
    app_id: *const u16,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentApplicationUserModelId(length: *mut u32, id: *mut u16) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn SHAddToRecentDocs(flags: u32, data: *const c_void);
    fn SHCreateItemFromParsingName(
        path: *const u16,
        context: *mut c_void,
        iid: *const GUID,
        item: *mut *mut c_void,
    ) -> HRESULT;
}

// Use the package's actual application ID so the document belongs to the same taskbar button
// and file associations as ImageView. An unpackaged build lets the shell infer its identity.
fn application_id() -> Option<Vec<u16>> {
    let mut length = 0;
    // SAFETY: This call only asks for the buffer length and fails for an unpackaged process.
    if unsafe { GetCurrentApplicationUserModelId(&mut length, null_mut()) }
        != ERROR_INSUFFICIENT_BUFFER
    {
        return None;
    }
    let mut id = vec![0; length as usize];
    // SAFETY: The buffer has the requested number of UTF-16 code units, including the null.
    (unsafe { GetCurrentApplicationUserModelId(&mut length, id.as_mut_ptr()) } == 0).then_some(id)
}

/// Records a successfully opened or saved image in Windows' recent documents and Jump List.
/// Windows manages ordering, pinning, removals and whether history is enabled.
pub(crate) fn note_recent_file(path: &Path) {
    let Ok(path) = std::path::absolute(path) else {
        return;
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let Some(app_id) = application_id() else {
        // SAFETY: SHARD_PATHW takes a null-terminated path and copies it during this call.
        unsafe { SHAddToRecentDocs(SHARD_PATHW, path.as_ptr().cast()) };
        return;
    };
    // SAFETY: The path is null terminated, COM is initialized by the UI event loop, and the
    // returned IShellItem reference is owned by the ComPtr.
    let Ok(item) = out(|raw| unsafe {
        SHCreateItemFromParsingName(path.as_ptr(), null_mut(), &IID_ISHELL_ITEM, raw)
    }) else {
        return;
    };
    let info = AppIdInfo {
        item: item.as_raw(),
        app_id: app_id.as_ptr(),
    };
    // SAFETY: SHARD_APPIDINFO takes this structure; both the item and ID live through the call.
    unsafe { SHAddToRecentDocs(SHARD_APPIDINFO, (&raw const info).cast()) };
}
