/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::{null, null_mut};
use std::rc::Rc;

use crate::CanvasRenderingContext2d;
use crate::platforms::windows::context::{DrawTarget, PlatformCanvasContext, Resources};
use crate::platforms::windows::headers::*;

/// A Direct2D render target for the client area of a window, kept sized to it.
#[derive(Default)]
pub(super) struct HwndTarget {
    target: Option<DrawTarget<ID2D1HwndRenderTarget>>,
    size: (u32, u32),
    dpi: u32,
}

impl HwndTarget {
    /// The cached render target resized to the client area, or a new one.
    pub(super) fn get(
        &mut self,
        resources: &Resources,
        hwnd: HWND,
        width: u32,
        height: u32,
        dpi: f32,
    ) -> Option<DrawTarget<ID2D1HwndRenderTarget>> {
        let size = D2D_SIZE_U { width, height };
        if let Some(target) = &self.target {
            if self.size != (width, height) {
                unsafe { target.native.Resize(&size) }.ok()?;
                self.size = (width, height);
            }
            if self.dpi != dpi as u32 {
                unsafe { target.native.SetDpi(dpi, dpi) };
                self.dpi = dpi as u32;
            }
            return Some(target.clone());
        }
        let target = unsafe {
            resources
                .factory
                .CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_UNKNOWN,
                            alphaMode: D2D1_ALPHA_MODE_UNKNOWN,
                        },
                        dpiX: dpi,
                        dpiY: dpi,
                        ..Default::default()
                    },
                    // The window paces the frames, so presenting never waits for the display.
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd,
                        pixelSize: size,
                        presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
                    },
                )
                .and_then(DrawTarget::new)
                .ok()?
        };
        self.size = (width, height);
        self.dpi = dpi as u32;
        self.target = Some(target.clone());
        Some(target)
    }

    /// Forget a lost render target, the next frame creates a new one.
    pub(super) fn lose(&mut self) {
        *self = Self::default();
    }
}

/// The client area of a window in physical pixels and its scale factor.
pub(super) fn client_size(hwnd: HWND) -> (u32, u32, f32) {
    let mut rect = RECT::default();
    let _ = unsafe { GetClientRect(hwnd, &mut rect) };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
    (
        (rect.right - rect.left).max(0) as u32,
        (rect.bottom - rect.top).max(0) as u32,
        dpi / 96.0,
    )
}

pub(crate) struct PlatformHwndCanvas {
    hwnd: HWND,
    resources: Rc<Resources>,
    target: HwndTarget,
}

impl PlatformHwndCanvas {
    pub(crate) fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            resources: Resources::new(),
            target: HwndTarget::default(),
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
        let Some(target) = self
            .target
            .get(&self.resources, self.hwnd, width, height, scale * 96.0)
        else {
            // A target that failed to resize is recreated by the next paint.
            self.target.lose();
            return;
        };
        unsafe { target.native.BeginDraw() };
        let mut context = CanvasRenderingContext2d::new(
            PlatformCanvasContext::new(&target, self.resources.clone()),
            width as f32 / scale,
            height as f32 / scale,
            scale,
        );
        draw(&mut context);
        drop(context);
        if unsafe { target.native.EndDraw(null_mut(), null_mut()) }.is_err() {
            self.target.lose();
            let _ = unsafe { InvalidateRect(self.hwnd, null(), FALSE) };
        }
    }
}
