/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

use std::ffi::c_void;

pub(super) const TOKEN_QUERY: u32 = 0x0008;
pub(super) const TOKEN_ELEVATION_CLASS: u32 = 20;
pub(super) const SYNCHRONIZE: u32 = 0x0010_0000;
pub(super) const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
pub(super) const SW_SHOWNORMAL: i32 = 1;
pub(super) const INFINITE: u32 = 0xffff_ffff;
pub(super) const DELETE: u32 = 0x0001_0000;
pub(super) const FILE_READ_ATTRIBUTES: u32 = 0x0080;
pub(super) const FILE_SHARE_READ: u32 = 0x0001;
pub(super) const FILE_SHARE_WRITE: u32 = 0x0002;
pub(super) const FILE_SHARE_DELETE: u32 = 0x0004;
pub(super) const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
pub(super) const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
pub(super) const FILE_DISPOSITION_INFO_CLASS: i32 = 4;
pub(super) const FILE_DISPOSITION_INFO_EX_CLASS: i32 = 21;
pub(super) const FILE_DISPOSITION_FLAG_DELETE: u32 = 0x0001;
pub(super) const FILE_DISPOSITION_FLAG_POSIX_SEMANTICS: u32 = 0x0002;
pub(super) const FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE: u32 = 0x0010;

#[repr(C)]
pub(super) struct TokenElevation {
    pub(super) token_is_elevated: u32,
}

#[repr(C)]
pub(super) struct ShellExecuteInfoW {
    pub(super) size: u32,
    pub(super) mask: u32,
    pub(super) hwnd: *mut c_void,
    pub(super) verb: *const u16,
    pub(super) file: *const u16,
    pub(super) parameters: *const u16,
    pub(super) directory: *const u16,
    pub(super) show: i32,
    pub(super) instance: *mut c_void,
    pub(super) id_list: *mut c_void,
    pub(super) class: *const u16,
    pub(super) class_key: *mut c_void,
    pub(super) hot_key: u32,
    pub(super) icon_or_monitor: *mut c_void,
    pub(super) process: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub(super) fn CloseHandle(object: *mut c_void) -> i32;
    pub(super) fn GetCurrentProcess() -> *mut c_void;
    pub(super) fn GetCurrentProcessId() -> u32;
    pub(super) fn GetFinalPathNameByHandleW(
        file: *mut c_void,
        path: *mut u16,
        path_length: u32,
        flags: u32,
    ) -> u32;
    pub(super) fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    pub(super) fn OpenProcess(
        desired_access: u32,
        inherit_handle: i32,
        process_id: u32,
    ) -> *mut c_void;
    pub(super) fn SetFileInformationByHandle(
        file: *mut c_void,
        information_class: i32,
        information: *const c_void,
        size: u32,
    ) -> i32;
    pub(super) fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
}

#[link(name = "advapi32")]
unsafe extern "system" {
    pub(super) fn GetTokenInformation(
        token: *mut c_void,
        information_class: u32,
        information: *mut c_void,
        information_length: u32,
        return_length: *mut u32,
    ) -> i32;
    pub(super) fn OpenProcessToken(
        process: *mut c_void,
        access: u32,
        token: *mut *mut c_void,
    ) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    pub(super) fn ShellExecuteExW(info: *mut ShellExecuteInfoW) -> i32;
}
