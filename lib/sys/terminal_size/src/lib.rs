/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [terminal_size](https://crates.io/crates/terminal_size) crate

#![allow(non_camel_case_types, non_snake_case, clippy::upper_case_acronyms)]

use std::mem::MaybeUninit;

/// Represents the width of a terminal in characters.
pub struct Width(pub u16);

/// Represents the height of a terminal in characters.
pub struct Height(pub u16);

/// Returns the terminal size as a tuple.
///
/// Checks stdout, stderr, and stdin (in that order) and returns the size of the first one that is
/// a terminal.
pub fn terminal_size() -> Option<(Width, Height)> {
    STREAMS.into_iter().find_map(stream_size)
}

cfg_select! {
    unix => {
        #[repr(C)]
        struct winsize {
            ws_row: u16,
            ws_col: u16,
            ws_xpixel: u16,
            ws_ypixel: u16,
        }
        const TIOCGWINSZ: std::ffi::c_ulong = if cfg!(any(
            target_os = "macos",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        )) {
            0x40087468
        } else {
            0x5413
        };
        #[allow(unsafe_code)]
        unsafe extern "C" {
            fn ioctl(fd: i32, op: std::ffi::c_ulong, ...) -> i32;
        }

        /// The stdout, stderr, and stdin file descriptors.
        const STREAMS: [i32; 3] = [1, 2, 0];

        #[allow(unsafe_code)]
        fn stream_size(fd: i32) -> Option<(Width, Height)> {
            let mut size = MaybeUninit::<winsize>::uninit();
            // SAFETY: size is a valid MaybeUninit<winsize> pointer; ioctl with TIOCGWINSZ fills it on success.
            if unsafe { ioctl(fd, TIOCGWINSZ, size.as_mut_ptr()) } == -1 {
                return None;
            }
            // SAFETY: ioctl returned success, so size was fully initialized by the kernel.
            let size = unsafe { size.assume_init() };
            (size.ws_row > 0 && size.ws_col > 0)
                .then_some((Width(size.ws_col), Height(size.ws_row)))
        }
    }
    windows => {
        #[repr(C)]
        struct COORD {
            X: i16,
            Y: i16,
        }
        #[repr(C)]
        struct SMALL_RECT {
            Left: i16,
            Top: i16,
            Right: i16,
            Bottom: i16,
        }
        #[repr(C)]
        struct CONSOLE_SCREEN_BUFFER_INFO {
            dwSize: COORD,
            dwCursorPosition: COORD,
            wAttributes: u16,
            srWindow: SMALL_RECT,
            dwMaximumWindowSize: COORD,
        }
        const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = (-1isize) as *mut std::ffi::c_void;
        #[allow(unsafe_code)]
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
            fn GetConsoleScreenBufferInfo(
                hConsoleOutput: *mut std::ffi::c_void,
                lpConsoleScreenBufferInfo: *mut CONSOLE_SCREEN_BUFFER_INFO,
            ) -> i32;
        }

        /// The STD_OUTPUT_HANDLE, STD_ERROR_HANDLE, and STD_INPUT_HANDLE constants.
        const STREAMS: [u32; 3] = [-11i32 as u32, -12i32 as u32, -10i32 as u32];

        #[allow(unsafe_code)]
        fn stream_size(std_handle: u32) -> Option<(Width, Height)> {
            // SAFETY: std_handle is a valid standard handle constant for GetStdHandle.
            let handle = unsafe { GetStdHandle(std_handle) };
            if handle == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut csbi = MaybeUninit::<CONSOLE_SCREEN_BUFFER_INFO>::uninit();
            // SAFETY: GetConsoleScreenBufferInfo fails for non-console handles; csbi.as_mut_ptr() is a valid output pointer.
            if unsafe { GetConsoleScreenBufferInfo(handle, csbi.as_mut_ptr()) } == 0 {
                return None;
            }
            // SAFETY: GetConsoleScreenBufferInfo returned success, so csbi was fully initialized by the OS.
            let window = unsafe { csbi.assume_init() }.srWindow;
            Some((
                Width((window.Right - window.Left + 1) as u16),
                Height((window.Bottom - window.Top + 1) as u16),
            ))
        }
    }
    _ => {
        compile_error!("Unsupported platform");
    }
}
