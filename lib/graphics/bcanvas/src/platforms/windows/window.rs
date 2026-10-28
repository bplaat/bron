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
use crate::platforms::windows::context::{PlatformCanvasContext, Resources};
use crate::platforms::windows::headers::*;
use crate::platforms::windows::hwnd::{HwndTarget, client_size};

// Draws the client area of the top-level window with a Direct2D HWND render target in the
// paint pass of its WM_PAINT. The window delivers the interaction.
struct CanvasData {
    hwnd: Cell<Option<HWND>>,
    sender: WindowEventSender,
    resources: Rc<Resources>,
    target: RefCell<HwndTarget>,
    frame: RefCell<Option<CanvasRenderingContext2d<'static>>>,
    painting: Cell<bool>,
}

impl CanvasData {
    fn request_redraw(&self) {
        let Some(hwnd) = self.hwnd.get() else {
            return;
        };
        // Redraws requested while painting wait for the next display frame.
        if self.painting.get() {
            self.sender.request_animation_frame();
        } else {
            let _ = unsafe { InvalidateRect(hwnd, null(), FALSE) };
        }
    }

    fn close(&self) {
        self.hwnd.take();
        self.frame.borrow_mut().take();
        self.target.borrow_mut().lose();
    }

    fn lose_target(&self) {
        // The brush belongs to the lost render target and is recreated with it.
        self.target.borrow_mut().lose();
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
        let (width, height, scale) = client_size(hwnd);
        if width == 0 || height == 0 {
            self.painting.set(false);
            return;
        }
        let target =
            self.target
                .borrow_mut()
                .get(&self.resources, hwnd, width, height, scale * 96.0);
        let Some(target) = target else {
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
        if !self.sender.redraw(|| self.end_draw(&target.native)) {
            self.end_draw(&target.native);
            self.request_redraw();
        }
        self.painting.set(false);
    }

    fn end_draw(&self, target: &ComPtr<ID2D1HwndRenderTarget>) {
        self.frame.borrow_mut().take();
        if unsafe { target.EndDraw(null_mut(), null_mut()) }.is_err() {
            self.lose_target();
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
            target: RefCell::new(HwndTarget::default()),
            frame: RefCell::new(None),
            painting: Cell::new(false),
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
        let close = data.clone();
        attachment.on_close(move || close.close());
        data.request_redraw();
        Self { data, attachment }
    }

    pub(crate) fn request_redraw(&self) {
        self.data.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.attachment.request_animation_frame();
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

#[cfg(test)]
mod tests {
    use bwindow::{Event, EventLoop, WindowBuilder, WindowEvent};

    use super::*;
    use crate::CanvasBuilder;

    #[test]
    fn startup_redraw_has_a_frame_and_draw_is_consumed_once() {
        // EventLoop::run exits the process, so exercise the complete native loop in a child.
        const CHILD: &str = "BCANVAS_FRAME_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let result = std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    "platforms::windows::window::tests::startup_redraw_has_a_frame_and_draw_is_consumed_once",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .expect("run native frame test");
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            return;
        }
        let event_loop = EventLoop::new();
        let watchdog = event_loop.create_proxy();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(5));
            let _ = watchdog.exit();
        });
        let mut window = WindowBuilder::new().title("Canvas frame test").build();
        let id = window.id();
        let mut canvas = CanvasBuilder::new(&window).build();
        assert!(!canvas.draw(|_| panic!("draw outside paint")));
        let painted = Rc::new(Cell::new(false));
        event_loop.run(move |event| {
            if let Event::Window(window_id, WindowEvent::RedrawRequested) = event
                && window_id == id
            {
                assert!(canvas.draw(|context| {
                    assert!(context.width() > 0.0 && context.height() > 0.0);
                    painted.set(true);
                }));
                assert!(!canvas.draw(|_| panic!("frame drawn twice")));
                window.close();
                assert!(!canvas.draw(|_| panic!("draw after close")));
            }
            if matches!(event, Event::LoopExiting) {
                assert!(painted.get(), "startup never produced a drawable frame");
            }
        });
    }
}
