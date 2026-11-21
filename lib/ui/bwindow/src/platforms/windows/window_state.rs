/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::{env, mem, slice};

use super::event_loop::APP_ID;
use super::headers::*;
use super::{registry_value, set_registry_value};

/// `HKEY_CURRENT_USER\Software\<organization>\<application>\Window`, next to the values of the
/// preferences crate, or the executable name without an app id
fn window_key() -> Vec<u16> {
    let application = unsafe {
        if let Some(ref app_id) = APP_ID {
            format!("{}\\{}", app_id.organization, app_id.application)
        } else {
            env::current_exe()
                .ok()
                .and_then(|path| Some(path.file_stem()?.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "bwindow".to_owned())
        }
    };
    format!("Software\\{application}\\Window\0")
        .encode_utf16()
        .collect()
}

/// Restores the window placement saved by [`save_window_state`], returns whether one was applied
pub(super) unsafe fn restore_window_state(hwnd: HWND) -> bool {
    let mut window_placement: WINDOWPLACEMENT = unsafe { mem::zeroed() };
    let data = unsafe {
        slice::from_raw_parts_mut(
            (&raw mut window_placement).cast::<u8>(),
            size_of::<WINDOWPLACEMENT>(),
        )
    };
    if !registry_value(
        HKEY_CURRENT_USER,
        &window_key(),
        w!("Placement"),
        REG_BINARY,
        data,
    ) {
        return false;
    }
    window_placement.length = size_of::<WINDOWPLACEMENT>() as u32;
    unsafe { SetWindowPlacement(hwnd, &window_placement) };
    true
}

/// Writes the window's current placement to the registry
pub(super) fn save_window_state(hwnd: HWND) {
    let mut window_placement: WINDOWPLACEMENT = unsafe { mem::zeroed() };
    window_placement.length = size_of::<WINDOWPLACEMENT>() as u32;
    if unsafe { GetWindowPlacement(hwnd, &mut window_placement) } == 0 {
        return;
    }
    let data = unsafe {
        slice::from_raw_parts(
            (&raw const window_placement).cast::<u8>(),
            size_of::<WINDOWPLACEMENT>(),
        )
    };
    set_registry_value(
        HKEY_CURRENT_USER,
        &window_key(),
        w!("Placement"),
        REG_BINARY,
        data,
    );
}
