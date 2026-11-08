/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::ptr::null;

use crate::headers::*;

pub(crate) fn is_process_elevated() -> bool {
    let mut token = null::<c_void>().cast_mut();
    // SAFETY: `token` points to writable storage for the returned process-token
    // handle, and GetCurrentProcess returns a valid pseudo-handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }

    let mut elevation = TokenElevation {
        token_is_elevated: 0,
    };
    let mut returned = 0;
    // SAFETY: `token` was opened successfully, and `elevation` is writable storage
    // with the exact size passed to GetTokenInformation.
    let success = unsafe {
        GetTokenInformation(
            token,
            TOKEN_ELEVATION_CLASS,
            (&raw mut elevation).cast(),
            size_of::<TokenElevation>() as u32,
            &mut returned,
        )
    } != 0;
    // SAFETY: `token` is an owned handle returned by OpenProcessToken and is closed once.
    _ = unsafe { CloseHandle(token) };
    success && elevation.token_is_elevated != 0
}

pub(crate) fn wait_for_parent_if_requested() {
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--elevated-from" {
            let Some(process_id) = arguments
                .next()
                .and_then(|value| value.to_string_lossy().parse::<u32>().ok())
            else {
                return;
            };

            // SAFETY: OpenProcess returns either a valid owned handle or null. Waiting with
            // SYNCHRONIZE access is valid, and the handle is closed exactly once afterward.
            let parent = unsafe { OpenProcess(SYNCHRONIZE, 0, process_id) };
            if !parent.is_null() {
                // SAFETY: `parent` is a valid process handle opened with SYNCHRONIZE access.
                _ = unsafe { WaitForSingleObject(parent, INFINITE) };
                // SAFETY: `parent` is an owned handle and is closed exactly once.
                _ = unsafe { CloseHandle(parent) };
            }
            return;
        }
    }
}

pub(crate) fn restart() -> io::Result<()> {
    let executable = std::env::current_exe()?;
    let operation = w!("runas");
    let executable = wide(&executable.to_string_lossy());
    // SAFETY: GetCurrentProcessId has no preconditions.
    let parameters = format!("--elevated-from {}", unsafe { GetCurrentProcessId() });
    let parameters = wide(&parameters);

    let mut info = ShellExecuteInfoW {
        size: size_of::<ShellExecuteInfoW>() as u32,
        mask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: null::<c_void>().cast_mut(),
        verb: operation.as_ptr(),
        file: executable.as_ptr(),
        parameters: parameters.as_ptr(),
        directory: null(),
        show: SW_SHOWNORMAL,
        instance: null::<c_void>().cast_mut(),
        id_list: null::<c_void>().cast_mut(),
        class: null(),
        class_key: null::<c_void>().cast_mut(),
        hot_key: 0,
        icon_or_monitor: null::<c_void>().cast_mut(),
        process: null::<c_void>().cast_mut(),
    };

    // SAFETY: `info` has the documented size and layout, and its UTF-16 buffers
    // are null-terminated and remain alive for the duration of the call.
    if unsafe { ShellExecuteExW(&raw mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.process.is_null() {
        return Err(io::Error::other(
            "Windows did not return an elevated process handle",
        ));
    }

    // The elevated child waits for this process to exit before initializing its WebView2
    // environment, so release the launch handle and let the caller close the window now.
    // SAFETY: `process` is an owned process handle returned by ShellExecuteExW.
    _ = unsafe { CloseHandle(info.process) };
    Ok(())
}
