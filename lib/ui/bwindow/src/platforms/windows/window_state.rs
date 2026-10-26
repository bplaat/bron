/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs::{self, File};
use std::io::Read;
use std::mem;

use super::headers::*;
use super::window::config_dir;

/// Restores the window placement saved by [`save_window_state`], returns whether one was applied
pub(super) unsafe fn restore_window_state(hwnd: HWND) -> bool {
    let Ok(mut file) = File::open(config_dir().join("window.bin")) else {
        return false;
    };
    let size = size_of::<WINDOWPLACEMENT>();
    let mut buffer = vec![0u8; size];
    if file.read_exact(&mut buffer).is_err() {
        return false;
    }
    unsafe {
        let mut window_placement: WINDOWPLACEMENT = std::ptr::read(buffer.as_ptr() as *const _);
        window_placement.length = size as u32;
        SetWindowPlacement(hwnd, &window_placement);
    }
    true
}

/// Writes the window's current placement to the config directory
pub(super) fn save_window_state(hwnd: HWND) {
    let directory = config_dir();
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    unsafe {
        use std::io::Write;
        let mut window_placement: WINDOWPLACEMENT = mem::zeroed();
        window_placement.length = size_of::<WINDOWPLACEMENT>() as u32;
        if GetWindowPlacement(hwnd, &mut window_placement) == 0 {
            return;
        }
        if let Ok(mut file) = File::create(directory.join("window.bin")) {
            _ = file.write_all(std::slice::from_raw_parts(
                &window_placement as *const _ as *const u8,
                size_of::<WINDOWPLACEMENT>(),
            ));
        }
    }
}
