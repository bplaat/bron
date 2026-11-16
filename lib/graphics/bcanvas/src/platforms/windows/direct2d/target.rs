/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ptr::null_mut;
use std::rc::Rc;

use super::context::{Context, DrawTarget, Resources};
use super::headers::*;
use crate::platforms::windows::dib::Dib;
use crate::platforms::windows::headers::*;

/// A Direct2D render target for the client area of a window, kept sized to it.
pub(crate) struct WindowTarget {
    resources: Rc<Resources>,
    target: Option<DrawTarget<ID2D1HwndRenderTarget>>,
    size: (u32, u32),
    dpi: u32,
    drawing: Option<DrawTarget<ID2D1HwndRenderTarget>>,
}

impl WindowTarget {
    pub(crate) fn new() -> Self {
        Self {
            resources: Resources::new(),
            target: None,
            size: (0, 0),
            dpi: 0,
            drawing: None,
        }
    }

    /// The cached render target resized to the client area, or a new one.
    fn get(
        &mut self,
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
            self.resources
                .factory
                .CreateHwndRenderTarget(
                    &D2D1_RENDER_TARGET_PROPERTIES {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
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

    /// Begin drawing the client area, or `None` when the render target is unavailable.
    pub(crate) fn begin(
        &mut self,
        hwnd: HWND,
        width: u32,
        height: u32,
        scale: f32,
    ) -> Option<Context> {
        let target = self.get(hwnd, width, height, scale * 96.0)?;
        unsafe { target.native.BeginDraw() };
        let context = Context::new(&target, self.resources.clone());
        self.drawing = Some(target);
        Some(context)
    }

    /// Present the frame after its context is dropped, false when the target was lost.
    pub(crate) fn end(&mut self, _hwnd: HWND) -> bool {
        let Some(target) = self.drawing.take() else {
            return true;
        };
        unsafe { target.native.EndDraw(null_mut(), null_mut()) }.is_ok()
    }

    /// Forget a lost render target, the next frame creates a new one.
    pub(crate) fn lose(&mut self) {
        self.target = None;
        self.drawing = None;
        self.size = (0, 0);
        self.dpi = 0;
    }
}

/// A software Direct2D render target bound to the device context of a bitmap.
pub(crate) struct OffscreenTarget {
    target: DrawTarget<ID2D1DCRenderTarget>,
    resources: Rc<Resources>,
}

impl OffscreenTarget {
    pub(crate) fn new(dib: &Dib) -> Self {
        let resources = Resources::new();
        unsafe {
            // Software rendering cannot lose its device, so drawing never fails.
            let target = resources
                .factory
                .CreateDCRenderTarget(&D2D1_RENDER_TARGET_PROPERTIES {
                    target_type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                    pixelFormat: D2D1_PIXEL_FORMAT {
                        format: DXGI_FORMAT_B8G8R8A8_UNORM,
                        alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                    },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    ..Default::default()
                })
                .expect("Direct2D bitmap target");
            target
                .BindDC(
                    dib.dc,
                    &RECT {
                        left: 0,
                        top: 0,
                        right: dib.width as i32,
                        bottom: dib.height as i32,
                    },
                )
                .expect("Bind Direct2D bitmap target");
            Self {
                target: DrawTarget::new(target).expect("Direct2D brush"),
                resources,
            }
        }
    }

    pub(crate) fn begin(&mut self, scale: f32) -> Context {
        unsafe {
            self.target.native.SetDpi(96.0 * scale, 96.0 * scale);
            self.target.native.BeginDraw();
        }
        Context::new(&self.target, self.resources.clone())
    }

    pub(crate) fn end(&mut self) {
        unsafe { self.target.native.EndDraw(null_mut(), null_mut()) }
            .expect("Draw offscreen canvas");
    }
}
