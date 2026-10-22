/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::rc::Rc;

use bwindow::{NativeContentHandle, WindowAttachment, WindowEventSender};

use crate::CanvasRenderingContext2d;
use crate::platforms::windows::com::ComPtr;
use crate::platforms::windows::context::{DrawTarget, PlatformCanvasContext, Resources};
use crate::platforms::windows::headers::*;

// Draws the client area of the top-level window with a Direct2D HWND render target in the
// paint pass of its WM_PAINT. The window delivers the interaction.
struct CanvasData {
    hwnd: Cell<Option<HWND>>,
    sender: WindowEventSender,
    resources: Rc<Resources>,
    target: RefCell<Option<DrawTarget<ID2D1HwndRenderTarget>>>,
    target_size: Cell<(u32, u32)>,
    target_dpi: Cell<u32>,
    frame: RefCell<Option<CanvasRenderingContext2d<'static>>>,
    painting: Cell<bool>,
    scheduled: Cell<bool>,
}

impl CanvasData {
    fn request_redraw(&self) {
        if let Some(hwnd) = self.hwnd.get() {
            self.scheduled.set(false);
            let _ = unsafe { InvalidateRect(hwnd, null(), FALSE) };
        }
    }

    fn request_animation_frame(&self) {
        if self.painting.get() {
            self.scheduled.set(true);
        } else {
            self.request_redraw();
        }
    }

    fn close(&self) {
        self.hwnd.take();
        self.frame.borrow_mut().take();
        self.target.borrow_mut().take();
        self.target_size.set((0, 0));
        self.target_dpi.set(0);
    }

    fn lose_target(&self) {
        // The brush belongs to the lost render target and is recreated with it.
        self.target.borrow_mut().take();
        self.target_size.set((0, 0));
        self.target_dpi.set(0);
        self.request_redraw();
    }

    // The window validated the update region before this paint pass.
    fn paint(&self) {
        let Some(hwnd) = self.hwnd.get() else {
            return;
        };
        if self.painting.replace(true) {
            self.request_redraw();
            return;
        }
        let mut rect = RECT::default();
        let _ = unsafe { GetClientRect(hwnd, &mut rect) };
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        if width == 0 || height == 0 {
            self.painting.set(false);
            return;
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
        let scale = dpi / 96.0;
        let Some(target) = self.target(hwnd, width, height, dpi) else {
            self.painting.set(false);
            self.lose_target();
            return;
        };
        unsafe { target.native.BeginDraw() }
        *self.frame.borrow_mut() = Some(CanvasRenderingContext2d::new(
            PlatformCanvasContext::new(&target, self.resources.clone()),
            width as f32 / scale,
            height as f32 / scale,
            scale,
        ));
        if !self.sender.redraw(|| self.end_draw(hwnd, &target.native)) {
            self.end_draw(hwnd, &target.native);
            self.request_redraw();
        }
        self.painting.set(false);
    }

    // The cached render target resized to the client area, or a new one.
    fn target(
        &self,
        hwnd: HWND,
        width: u32,
        height: u32,
        dpi: f32,
    ) -> Option<DrawTarget<ID2D1HwndRenderTarget>> {
        let size = D2D_SIZE_U { width, height };
        let cached = self.target.borrow().clone();
        if let Some(target) = cached {
            if self.target_size.get() != (width, height) {
                unsafe { target.native.Resize(&size) }.ok()?;
                self.target_size.set((width, height));
            }
            if self.target_dpi.replace(dpi as u32) != dpi as u32 {
                unsafe { target.native.SetDpi(dpi, dpi) };
            }
            return Some(target);
        }
        let target = unsafe {
            self.resources
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
                    &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                        hwnd,
                        pixelSize: size,
                        ..Default::default()
                    },
                )
                .and_then(DrawTarget::new)
                .ok()?
        };
        self.target_size.set((width, height));
        self.target_dpi.set(dpi as u32);
        *self.target.borrow_mut() = Some(target.clone());
        Some(target)
    }

    fn end_draw(&self, hwnd: HWND, target: &ComPtr<ID2D1HwndRenderTarget>) {
        self.frame.borrow_mut().take();
        if unsafe { target.EndDraw(null_mut(), null_mut()) }.is_err() {
            self.lose_target();
        } else if self.scheduled.replace(false) {
            // The HWND render target's default presentation mode already waits for
            // display refresh. Queue the next coalesced WM_PAINT after that present.
            let _ = unsafe { InvalidateRect(hwnd, null(), FALSE) };
        }
    }
}

pub(crate) struct PlatformCanvas {
    data: Rc<CanvasData>,
    attachment: WindowAttachment,
}

impl PlatformCanvas {
    pub(crate) fn new(attachment: WindowAttachment) -> Self {
        let Some(NativeContentHandle::Win32(hwnd)) = (unsafe { attachment.content_handle() })
        else {
            panic!("expected an open Win32 window");
        };
        let data = Rc::new(CanvasData {
            hwnd: Cell::new(Some(hwnd)),
            sender: attachment.event_sender(),
            resources: Resources::new(),
            target: RefCell::new(None),
            target_size: Cell::new((0, 0)),
            target_dpi: Cell::new(0),
            frame: RefCell::new(None),
            painting: Cell::new(false),
            scheduled: Cell::new(false),
        });
        let paint = Rc::downgrade(&data);
        attachment.windows_on_paint(move || {
            if let Some(data) = paint.upgrade() {
                data.paint();
            }
        });
        let resize = Rc::downgrade(&data);
        attachment.on_resize(move |_, _| {
            if let Some(data) = resize.upgrade() {
                data.request_redraw();
            }
        });
        let redraw = Rc::downgrade(&data);
        attachment.on_redraw(move || {
            if let Some(data) = redraw.upgrade() {
                data.request_redraw();
            }
        });
        let animation_frame = Rc::downgrade(&data);
        attachment.on_animation_frame(move || {
            if let Some(data) = animation_frame.upgrade() {
                data.request_animation_frame();
            }
        });
        let close = data.clone();
        attachment.on_close(move || close.close());
        data.request_redraw();
        Self { data, attachment }
    }

    pub(crate) fn request_redraw(&self) {
        self.data.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.data.request_animation_frame();
    }

    pub(crate) fn draw(
        &mut self,
        draw: impl for<'frame> FnOnce(&mut CanvasRenderingContext2d<'frame>),
    ) -> bool {
        if self.attachment.is_closed() {
            return false;
        }
        let frame = self.data.frame.borrow_mut().take();
        let Some(mut frame) = frame else { return false };
        draw(&mut frame);
        true
    }
}

impl Drop for PlatformCanvas {
    fn drop(&mut self) {
        // Erase the canvas, the window paints its background again.
        if let Some(hwnd) = self.data.hwnd.get() {
            let _ = unsafe { InvalidateRect(hwnd, null(), TRUE) };
        }
        self.attachment.disconnect();
        self.data.close();
    }
}
