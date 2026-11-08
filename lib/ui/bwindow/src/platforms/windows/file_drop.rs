/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::null_mut;

use super::headers::*;
use crate::WindowEvent;
use crate::dispatch::send as send_event;

/// Reports the files of a WM_DROPFILES message dropped onto the window frame
pub(super) unsafe fn handle_file_drop(window_id: crate::WindowId, drop: HDROP) {
    let count = unsafe { DragQueryFileW(drop, u32::MAX, null_mut(), 0) };
    for index in 0..count {
        let length = unsafe { DragQueryFileW(drop, index, null_mut(), 0) };
        let mut buffer = vec![0; length as usize + 1];
        unsafe { DragQueryFileW(drop, index, buffer.as_mut_ptr(), buffer.len() as u32) };
        send_event(crate::Event::Window(
            window_id,
            WindowEvent::DroppedFile(String::from_utf16_lossy(&buffer[..length as usize]).into()),
        ));
    }
    unsafe { DragFinish(drop) };
}
