/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The display frames of a window: a thread waits for the vertical blank of the monitor the
//! window is on while animation frames are requested, and posts each one to the window.

use std::ffi::c_void;
use std::ptr::null;
use std::rc::Weak;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::headers::*;
use crate::NativeWindowHandle;
use crate::content::ContentHost;

/// Posted to the window for every requested display frame.
pub(super) const WM_ANIMATION_FRAME: u32 = WM_USER + 16;

const MONITOR_DEFAULTTONEAREST: u32 = 2;

#[repr(C)]
#[derive(Default)]
struct D3dkmtOpenAdapterFromHdc {
    hdc: HDC,
    adapter: u32,
    adapter_luid: [u32; 2],
    vid_pn_source_id: u32,
}

#[repr(C)]
struct D3dkmtWaitForVerticalBlankEvent {
    adapter: u32,
    device: u32,
    vid_pn_source_id: u32,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateDCW(
        driver: *const w_char,
        device: *const w_char,
        port: *const w_char,
        mode: *const c_void,
    ) -> HDC;
    fn DeleteDC(hdc: HDC) -> BOOL;
    fn D3DKMTOpenAdapterFromHdc(open: *mut D3dkmtOpenAdapterFromHdc) -> i32;
    fn D3DKMTWaitForVerticalBlankEvent(wait: *const D3dkmtWaitForVerticalBlankEvent) -> i32;
    fn D3DKMTCloseAdapter(adapter: *const u32) -> i32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MonitorFromWindow(hwnd: HWND, flags: u32) -> HMONITOR;
}

#[derive(Default)]
struct State {
    requested: bool,
    stopped: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

pub(crate) struct FrameSource(Arc<Shared>);

impl FrameSource {
    pub(crate) fn new(handle: NativeWindowHandle, _: Weak<ContentHost>) -> Option<Self> {
        let NativeWindowHandle::Win32(hwnd) = handle else {
            return None;
        };
        let shared = Arc::new(Shared::default());
        let thread = shared.clone();
        let hwnd = hwnd as usize;
        std::thread::Builder::new()
            .name("bwindow-frames".into())
            .spawn(move || run(&thread, hwnd as HWND))
            .ok()?;
        Some(Self(shared))
    }

    pub(crate) fn request(&self) {
        self.update(|state| state.requested = true);
    }

    fn update(&self, f: impl FnOnce(&mut State)) {
        f(&mut self.0.state.lock().expect("frame state"));
        self.0.wake.notify_one();
    }
}

impl Drop for FrameSource {
    fn drop(&mut self) {
        self.update(|state| state.stopped = true);
    }
}

fn run(shared: &Shared, hwnd: HWND) {
    let mut vblank = VerticalBlank::default();
    loop {
        {
            let mut state = shared
                .wake
                .wait_while(shared.state.lock().expect("frame state"), |state| {
                    !state.requested && !state.stopped
                })
                .expect("frame state");
            if state.stopped {
                return;
            }
            state.requested = false;
        }
        vblank.wait(hwnd);
        if shared.state.lock().expect("frame state").stopped {
            return;
        }
        unsafe { PostMessageW(hwnd, WM_ANIMATION_FRAME, 0, 0) };
    }
}

// The display adapter output of the monitor the window is on.
#[derive(Default)]
struct VerticalBlank {
    monitor: HMONITOR,
    output: Option<D3dkmtWaitForVerticalBlankEvent>,
}

impl VerticalBlank {
    // Wait for the next vertical blank, or approximate a 60 Hz display without one.
    fn wait(&mut self, hwnd: HWND) {
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        if monitor != self.monitor {
            self.close();
            self.monitor = monitor;
            self.output = open_output(monitor);
        }
        let waited = self
            .output
            .as_ref()
            .is_some_and(|output| unsafe { D3DKMTWaitForVerticalBlankEvent(output) } == 0);
        if !waited {
            std::thread::sleep(Duration::from_millis(16));
        }
    }

    fn close(&mut self) {
        if let Some(output) = self.output.take() {
            unsafe { D3DKMTCloseAdapter(&output.adapter) };
        }
    }
}

impl Drop for VerticalBlank {
    fn drop(&mut self) {
        self.close();
    }
}

fn open_output(monitor: HMONITOR) -> Option<D3dkmtWaitForVerticalBlankEvent> {
    let mut info = MONITORINFOEXW {
        cbSize: size_of::<MONITORINFOEXW>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return None;
    }
    let hdc = unsafe { CreateDCW(null(), info.szDevice.as_ptr(), null(), null()) };
    if hdc.is_null() {
        return None;
    }
    let mut open = D3dkmtOpenAdapterFromHdc {
        hdc,
        ..Default::default()
    };
    let status = unsafe { D3DKMTOpenAdapterFromHdc(&mut open) };
    unsafe { DeleteDC(hdc) };
    (status == 0).then_some(D3dkmtWaitForVerticalBlankEvent {
        adapter: open.adapter,
        device: 0,
        vid_pn_source_id: open.vid_pn_source_id,
    })
}
