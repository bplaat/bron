/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::null;

use crate::CanvasRenderingContext2d;
#[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
use crate::platforms::windows::gdi;
use crate::platforms::windows::headers::*;
use crate::platforms::windows::{Backend, PlatformCanvasContext, backend, direct2d};

/// The DPI of a window, the normal Windows targets need Windows 10
#[cfg(not(any(target_vendor = "winxp", target_vendor = "win7")))]
unsafe fn window_dpi(hwnd: HWND) -> u32 {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDpiForWindow(hwnd: HWND) -> u32;
    }
    unsafe { GetDpiForWindow(hwnd) }
}

/// The DPI of a window, GetDpiForWindow came with Windows 10
#[cfg(any(target_vendor = "winxp", target_vendor = "win7"))]
unsafe fn window_dpi(hwnd: HWND) -> u32 {
    type GetDpi = unsafe extern "system" fn(HWND) -> u32;
    static GET_DPI: std::sync::OnceLock<Option<GetDpi>> = std::sync::OnceLock::new();
    let get_dpi = GET_DPI.get_or_init(|| unsafe {
        // user32 is held by the imports for the process lifetime.
        let module = GetModuleHandleA(c"user32.dll".as_ptr());
        if module.is_null() {
            return None;
        }
        let address = GetProcAddress(module, c"GetDpiForWindow".as_ptr());
        (!address.is_null())
            .then(|| std::mem::transmute::<*const std::ffi::c_void, GetDpi>(address))
    });
    if let Some(get_dpi) = get_dpi {
        return unsafe { get_dpi(hwnd) };
    }
    // Without per-window DPI, use the system DPI.
    let dc = unsafe { GetDC(hwnd) };
    if dc.is_null() {
        return 96;
    }
    let dpi = unsafe { GetDeviceCaps(dc, LOGPIXELSX) };
    unsafe { ReleaseDC(hwnd, dc) };
    dpi.max(1) as u32
}

/// The client area of a window in physical pixels and its scale factor.
pub(crate) fn client_size(hwnd: HWND) -> (u32, u32, f32) {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    let dpi = unsafe { window_dpi(hwnd) }.max(96) as f32;
    (
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
        dpi / 96.0,
    )
}

/// The drawing surface of the client area of a window, of the process backend.
pub(crate) enum WindowTarget {
    Direct2D(direct2d::WindowTarget),
    #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
    Gdi(gdi::WindowTarget),
}

impl WindowTarget {
    pub(crate) fn new() -> Self {
        match backend() {
            Backend::Direct2D => Self::Direct2D(direct2d::WindowTarget::new()),
            #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
            Backend::Gdi => Self::Gdi(gdi::WindowTarget::new()),
        }
    }

    /// Begin a frame of the client area in physical pixels, `None` when the surface is
    /// unavailable, after which the caller loses it.
    pub(crate) fn begin(
        &mut self,
        hwnd: HWND,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Option<PlatformCanvasContext> {
        match self {
            Self::Direct2D(target) => target
                .begin(hwnd, width, height, scale)
                .map(PlatformCanvasContext::Direct2D),
            #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
            Self::Gdi(target) => target
                .begin(hwnd, width, height, scale)
                .map(|context| PlatformCanvasContext::Gdi(Box::new(context))),
        }
    }

    /// Present the frame once its context is dropped, false when the surface was lost.
    pub(crate) fn end(&mut self, hwnd: HWND) -> bool {
        match self {
            Self::Direct2D(target) => target.end(hwnd),
            #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
            Self::Gdi(target) => target.end(hwnd),
        }
    }

    /// Forget a lost surface, the next frame creates a new one.
    pub(crate) fn lose(&mut self) {
        match self {
            Self::Direct2D(target) => target.lose(),
            #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
            Self::Gdi(target) => target.lose(),
        }
    }
}

pub(crate) struct PlatformHwndCanvas {
    hwnd: HWND,
    target: WindowTarget,
}

impl PlatformHwndCanvas {
    pub(crate) fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            target: WindowTarget::new(),
        }
    }

    pub(crate) fn paint(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) {
        let _ = unsafe { ValidateRect(self.hwnd, null()) };
        let (width, height, scale) = client_size(self.hwnd);
        if width == 0 || height == 0 {
            return;
        }
        let Some(platform) = self.target.begin(self.hwnd, width, height, scale) else {
            // A target that failed to resize is recreated by the next paint.
            self.target.lose();
            return;
        };
        let mut context = CanvasRenderingContext2d::new(
            platform,
            width as f32 / scale,
            height as f32 / scale,
            scale,
        );
        draw(&mut context);
        drop(context);
        if !self.target.end(self.hwnd) {
            self.target.lose();
            let _ = unsafe { InvalidateRect(self.hwnd, null(), FALSE) };
        }
    }
}
