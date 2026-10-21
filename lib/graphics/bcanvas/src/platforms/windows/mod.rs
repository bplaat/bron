/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::rc::Rc;

use bwindow::ffi::*;
use bwindow::{
    ButtonState, LogicalPoint, MouseButton, NativeWindowHandle, ScrollDelta, WindowAttachment,
    WindowEvent, WindowEventSender,
};

use crate::CanvasRenderingContext2d;
use crate::platforms::windows::com::ComPtr;
pub(crate) use crate::platforms::windows::context::PlatformCanvasContext;
use crate::platforms::windows::context::{DrawTarget, Resources};
use crate::platforms::windows::headers::*;
pub(crate) use crate::platforms::windows::offscreen::PlatformOffscreenCanvas;

mod com;
mod context;
mod headers;
mod offscreen;

struct CanvasData {
    cursor: Cell<crate::CursorIcon>,
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
    fn apply_cursor(&self) {
        let name = match self.cursor.get() {
            crate::CursorIcon::Default => IDC_ARROW,
            crate::CursorIcon::Pointer => IDC_HAND,
            crate::CursorIcon::Progress => IDC_APPSTARTING,
        };
        unsafe { SetCursor(LoadCursorW(null(), name)) };
    }

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
        if let Some(hwnd) = self.hwnd.take() {
            let _ = unsafe { DestroyWindow(hwnd) };
        }
        self.frame.borrow_mut().take();
        self.target.borrow_mut().take();
        self.target_size.set((0, 0));
        self.target_dpi.set(0);
    }

    fn paint(&self, hwnd: HWND) {
        let mut paint = PAINTSTRUCT::default();
        unsafe { BeginPaint(hwnd, &mut paint) };
        if self.painting.replace(true) {
            let _ = unsafe { EndPaint(hwnd, &paint) };
            self.request_redraw();
            return;
        }
        let mut rect = RECT::default();
        let _ = unsafe { GetClientRect(hwnd, &mut rect) };
        let width = (rect.right - rect.left).max(0) as u32;
        let height = (rect.bottom - rect.top).max(0) as u32;
        if width == 0 || height == 0 {
            self.painting.set(false);
            let _ = unsafe { EndPaint(hwnd, &paint) };
            return;
        }
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
        let scale = dpi / 96.0;
        let size = D2D_SIZE_U { width, height };
        let cached = self.target.borrow().clone();
        let target = if let Some(target) = cached {
            if self.target_size.get() != (width, height) {
                if unsafe { target.native.Resize(&size) }.is_err() {
                    self.target.borrow_mut().take();
                    self.target_size.set((0, 0));
                    self.target_dpi.set(0);
                    let _ = unsafe { EndPaint(hwnd, &paint) };
                    self.request_redraw();
                    self.painting.set(false);
                    return;
                }
                self.target_size.set((width, height));
            }
            if self.target_dpi.replace(dpi as u32) != dpi as u32 {
                unsafe { target.native.SetDpi(dpi, dpi) };
            }
            Some(target)
        } else {
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
                    .ok()
            };
            if target.is_some() {
                self.target_size.set((width, height));
                self.target_dpi.set(dpi as u32);
            }
            target
        };
        let Some(target) = target else {
            let _ = unsafe { EndPaint(hwnd, &paint) };
            self.request_redraw();
            self.painting.set(false);
            return;
        };
        *self.target.borrow_mut() = Some(target.clone());
        unsafe { target.native.BeginDraw() }
        *self.frame.borrow_mut() = Some(CanvasRenderingContext2d::new(
            PlatformCanvasContext::new(&target, self.resources.clone()),
            width as f32 / scale,
            height as f32 / scale,
            scale,
        ));
        if !self
            .sender
            .redraw(|| self.end_paint(hwnd, &paint, &target.native))
        {
            self.end_paint(hwnd, &paint, &target.native);
            self.request_redraw();
        }
        self.painting.set(false);
    }

    fn end_paint(&self, hwnd: HWND, paint: &PAINTSTRUCT, target: &ComPtr<ID2D1HwndRenderTarget>) {
        self.frame.borrow_mut().take();
        let failed = unsafe { target.EndDraw(null_mut(), null_mut()) }.is_err();
        let _ = unsafe { EndPaint(hwnd, paint) };
        if failed {
            // The brush belongs to the lost render target and is recreated with it.
            self.target.borrow_mut().take();
            self.target_size.set((0, 0));
            self.target_dpi.set(0);
            self.request_redraw();
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
        let Some(NativeWindowHandle::Win32(parent)) = (unsafe { attachment.native_handle() })
        else {
            panic!("expected an open Win32 window");
        };
        unsafe {
            let instance = GetModuleHandleW(null());
            assert!(!instance.is_null(), "application module");
            let class = wide!("BCanvas").as_ptr();
            let mut existing = WNDCLASSEXW::default();
            if GetClassInfoExW(instance, class, &mut existing) == FALSE {
                assert_ne!(
                    RegisterClassExW(&WNDCLASSEXW {
                        cbSize: size_of::<WNDCLASSEXW>() as u32,
                        hInstance: instance,
                        lpszClassName: class,
                        lpfnWndProc: Some(window_proc),
                        hCursor: LoadCursorW(null(), IDC_ARROW),
                        style: CS_HREDRAW | CS_VREDRAW,
                        ..Default::default()
                    }),
                    0,
                    "register canvas class"
                );
            }
            let data = Rc::new(CanvasData {
                cursor: Cell::new(crate::CursorIcon::Default),
                hwnd: Cell::new(None),
                sender: attachment.event_sender(),
                resources: Resources::new(),
                target: RefCell::new(None),
                target_size: Cell::new((0, 0)),
                target_dpi: Cell::new(0),
                frame: RefCell::new(None),
                painting: Cell::new(false),
                scheduled: Cell::new(false),
            });
            let mut rect = RECT::default();
            assert_ne!(
                GetClientRect(parent, &mut rect),
                FALSE,
                "window client bounds"
            );
            let hwnd = CreateWindowExW(
                0,
                class,
                wide!("").as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                0,
                0,
                rect.right - rect.left,
                rect.bottom - rect.top,
                parent,
                null_mut(),
                instance,
                Rc::as_ptr(&data) as isize,
            );
            assert!(!hwnd.is_null(), "create canvas child window");
            data.hwnd.set(Some(hwnd));
            #[cfg(feature = "file_drop")]
            if attachment.allow_file_drop() {
                DragAcceptFiles(hwnd, TRUE);
            }
            let resize = Rc::downgrade(&data);
            attachment.on_resize(move |width, height| {
                if let Some(data) = resize.upgrade()
                    && let Some(hwnd) = data.hwnd.get()
                {
                    let _ = SetWindowPos(
                        hwnd,
                        null_mut(),
                        0,
                        0,
                        width as i32,
                        height as i32,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
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
            let _ = SetFocus(hwnd);
            data.request_redraw();
            Self { data, attachment }
        }
    }

    pub(crate) fn request_redraw(&self) {
        self.data.request_redraw();
    }

    pub(crate) fn request_animation_frame(&self) {
        self.data.request_animation_frame();
    }

    pub(crate) fn set_cursor(&mut self, cursor: crate::CursorIcon) {
        let Some(hwnd) = self.data.hwnd.get() else {
            return;
        };
        if self.data.cursor.replace(cursor) == cursor {
            return;
        }
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } != FALSE && unsafe { WindowFromPoint(point) } == hwnd
        {
            self.data.apply_cursor();
        }
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
        self.attachment.disconnect();
        self.data.close();
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            let data = create.lpCreateParams.cast::<CanvasData>();
            Rc::increment_strong_count(data);
            SetWindowLong(hwnd, GWL_USERDATA, data as isize);
        }
        let pointer = GetWindowLong(hwnd, GWL_USERDATA) as *const CanvasData;
        if pointer.is_null() {
            return DefWindowProcW(hwnd, message, wparam, lparam);
        }
        Rc::increment_strong_count(pointer);
        let data = Rc::from_raw(pointer);
        match message {
            WM_SETCURSOR if lparam as u16 == HTCLIENT => {
                data.apply_cursor();
                return 1;
            }
            WM_NCDESTROY => {
                SetWindowLong(hwnd, GWL_USERDATA, 0);
                data.hwnd.set(None);
                drop(Rc::from_raw(pointer));
            }
            WM_PAINT => {
                data.paint(hwnd);
                return 0;
            }
            WM_ERASEBKGND => return 1,
            WM_SIZE => data.request_redraw(),
            WM_SETFOCUS => data.sender.send(WindowEvent::Focus),
            WM_KILLFOCUS => data.sender.send(WindowEvent::Blur),
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    ..Default::default()
                };
                let _ = TrackMouseEvent(&mut track);
                data.sender.send(WindowEvent::MouseMove {
                    position: pointer_position(hwnd, lparam),
                    modifiers: windows_modifiers(),
                });
            }
            WM_MOUSELEAVE => data.sender.send(WindowEvent::MouseLeave),
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
            | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP => {
                let pressed = matches!(
                    message,
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                );
                if pressed {
                    let _ = SetFocus(hwnd);
                    SetCapture(hwnd);
                } else if wparam & 0x73 == 0 {
                    let _ = ReleaseCapture();
                }
                let button = mouse_button(message, wparam);
                let event = bwindow::MouseEvent {
                    button,
                    position: pointer_position(hwnd, lparam),
                    modifiers: windows_modifiers(),
                };
                data.sender.send(if pressed {
                    WindowEvent::MouseDown(event)
                } else {
                    WindowEvent::MouseUp(event)
                });
                return mouse_button_result(message);
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = (wparam >> 16) as u16 as i16 as f64 / 120.0;
                data.sender.send(WindowEvent::Wheel(
                    if message == WM_MOUSEWHEEL {
                        ScrollDelta::Lines(0.0, -delta)
                    } else {
                        ScrollDelta::Lines(delta, 0.0)
                    },
                    windows_modifiers(),
                ));
                return 0;
            }
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP => {
                let pressed = matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN);
                let event = windows_key_event(
                    wparam as u32,
                    lparam,
                    if pressed {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                );
                data.sender.send(if pressed {
                    WindowEvent::KeyDown(event)
                } else {
                    WindowEvent::KeyUp(event)
                });
                // Preserve native Alt+F4 and other system-key handling.
                if matches!(message, WM_KEYDOWN | WM_KEYUP) {
                    return 0;
                }
            }
            #[cfg(feature = "file_drop")]
            WM_DROPFILES => {
                use std::os::windows::ffi::OsStringExt;

                let drop = wparam as HDROP;
                for index in 0..DragQueryFileW(drop, u32::MAX, null_mut(), 0) {
                    let mut path = vec![0; DragQueryFileW(drop, index, null_mut(), 0) as usize + 1];
                    let count =
                        DragQueryFileW(drop, index, path.as_mut_ptr(), path.len() as u32) as usize;
                    data.sender.send(WindowEvent::DroppedFile(
                        std::ffi::OsString::from_wide(&path[..count]).into(),
                    ));
                }
                DragFinish(drop);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, message, wparam, lparam)
    }
}

const fn mouse_button(message: u32, wparam: WPARAM) -> MouseButton {
    match message {
        WM_LBUTTONDOWN | WM_LBUTTONUP => MouseButton::Left,
        WM_RBUTTONDOWN | WM_RBUTTONUP => MouseButton::Right,
        WM_MBUTTONDOWN | WM_MBUTTONUP => MouseButton::Middle,
        _ => MouseButton::Other(((wparam >> 16) & 0xffff) as u16),
    }
}

const fn mouse_button_result(message: u32) -> LRESULT {
    if matches!(message, WM_XBUTTONDOWN | WM_XBUTTONUP) {
        1
    } else {
        0
    }
}

unsafe fn pointer_position(hwnd: HWND, lparam: LPARAM) -> LogicalPoint {
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    LogicalPoint::new(
        lparam as u16 as i16 as f32 / scale,
        (lparam >> 16) as u16 as i16 as f32 / scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_native_extra_mouse_button_numbers() {
        assert_eq!(mouse_button(WM_XBUTTONDOWN, 1 << 16), MouseButton::Other(1));
        assert_eq!(mouse_button(WM_XBUTTONUP, 2 << 16), MouseButton::Other(2));
    }

    #[test]
    fn acknowledges_extra_mouse_button_messages() {
        assert_eq!(mouse_button_result(WM_LBUTTONDOWN), 0);
        assert_eq!(mouse_button_result(WM_XBUTTONDOWN), 1);
        assert_eq!(mouse_button_result(WM_XBUTTONUP), 1);
    }
}
