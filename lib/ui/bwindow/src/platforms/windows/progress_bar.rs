/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::null_mut;

use super::com::ComPtr;
use super::headers::*;
use crate::WindowsProgressBarState;

/// The progress a window asked for, applied once its taskbar button exists
#[derive(Default)]
pub(super) struct ProgressBar {
    progress: Option<f32>,
    state: WindowsProgressBarState,
    button_ready: bool,
}

impl ProgressBar {
    /// Stores the requested progress and shows it when the taskbar button is ready
    pub(super) fn set(
        &mut self,
        hwnd: HWND,
        progress: Option<f32>,
        state: WindowsProgressBarState,
    ) {
        self.progress = progress;
        self.state = state;
        if self.button_ready {
            set_progress_bar(hwnd, progress, state);
        }
    }

    /// Shows the stored progress now that Explorer has created the taskbar button
    pub(super) fn button_created(&mut self, hwnd: HWND) {
        self.button_ready = true;
        set_progress_bar(hwnd, self.progress, self.state);
    }
}

// Shows, updates or hides the taskbar button progress of a window
fn set_progress_bar(hwnd: HWND, progress: Option<f32>, state: WindowsProgressBarState) {
    let Ok(taskbar) = (unsafe {
        ComPtr::create(|out: *mut *mut ITaskbarList3| {
            CoCreateInstance(
                &CLSID_TASKBAR_LIST,
                null_mut(),
                CLSCTX_INPROC_SERVER,
                &IID_TASKBAR_LIST3,
                out.cast(),
            )
        })
    }) else {
        return;
    };
    unsafe {
        if taskbar.init().is_ok() {
            let taskbar_state = match (progress, state) {
                (None, _) => 0,
                (Some(_), WindowsProgressBarState::Normal) => 2,
                (Some(_), WindowsProgressBarState::Error) => 4,
                (Some(_), WindowsProgressBarState::Paused) => 8,
                (Some(_), WindowsProgressBarState::Indeterminate) => 1,
            };
            if let (Some(progress), false) = (
                progress,
                matches!(state, WindowsProgressBarState::Indeterminate),
            ) {
                let progress = progress.clamp(0.0, 1.0);
                taskbar.set_progress_value(hwnd, (progress * 1000.0) as u64, 1000);
            }
            taskbar.set_progress_state(hwnd, taskbar_state);
        }
    }
}
