/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Keyboard, pointer, scroll, focus and cursor handling of the window client area, which
//! is the content host renderers draw into. Window children like a web view receive their
//! own input, so their interaction is never delivered twice.

use std::cell::Cell;
use std::ptr::null;

use super::headers::*;
use super::input::{windows_key_event, windows_modifiers};
use crate::dispatch::send as send_event;
use crate::{
    ButtonState, CursorIcon, Event, LogicalPoint, MouseButton, MouseEvent, ScrollDelta,
    WindowEvent, WindowId,
};

const WM_ACTIVATE: u32 = 0x0006;
const WM_SETCURSOR: u32 = 0x0020;
const WM_KEYDOWN: u32 = 0x0100;
const WM_KEYUP: u32 = 0x0101;
const WM_SYSKEYDOWN: u32 = 0x0104;
const WM_SYSKEYUP: u32 = 0x0105;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_RBUTTONDOWN: u32 = 0x0204;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_MBUTTONDOWN: u32 = 0x0207;
const WM_MBUTTONUP: u32 = 0x0208;
const WM_MOUSEWHEEL: u32 = 0x020a;
const WM_XBUTTONDOWN: u32 = 0x020b;
const WM_XBUTTONUP: u32 = 0x020c;
const WM_MOUSEHWHEEL: u32 = 0x020e;
const WM_MOUSELEAVE: u32 = 0x02a3;
const HTCLIENT: u16 = 1;
const TME_LEAVE: u32 = 2;
// The mouse button flags of a button message, any held button keeps the capture.
const MK_BUTTONS: usize = 0x0001 | 0x0002 | 0x0010 | 0x0020 | 0x0040;
const IDC_ARROW: *const w_char = 32512usize as *const w_char;
const IDC_HAND: *const w_char = 32649usize as *const w_char;
const IDC_APPSTARTING: *const w_char = 32650usize as *const w_char;

#[repr(C)]
#[derive(Default)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
struct TRACKMOUSEEVENT {
    cbSize: u32,
    dwFlags: u32,
    hwndTrack: HWND,
    dwHoverTime: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetCursorPos(point: *mut POINT) -> BOOL;
    fn WindowFromPoint(point: POINT) -> HWND;
    fn SetCursor(cursor: HCURSOR) -> HCURSOR;
    fn LoadCursorW(instance: HMODULE, name: *const w_char) -> HCURSOR;
    fn SetCapture(hwnd: HWND) -> HWND;
    fn ReleaseCapture() -> BOOL;
    fn TrackMouseEvent(event: *mut TRACKMOUSEEVENT) -> BOOL;
}

/// The interaction state of one window.
#[derive(Default)]
pub(super) struct Interaction {
    cursor: Cell<CursorIcon>,
}

impl Interaction {
    /// Show `cursor` over the client area, immediately when the pointer is over it.
    pub(super) fn set_cursor(&self, hwnd: HWND, cursor: CursorIcon) {
        if self.cursor.replace(cursor) == cursor {
            return;
        }
        let mut point = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut point) } != FALSE && unsafe { WindowFromPoint(point) } == hwnd
        {
            self.apply_cursor();
        }
    }

    fn apply_cursor(&self) {
        let name = match self.cursor.get() {
            CursorIcon::Default => IDC_ARROW,
            CursorIcon::Pointer => IDC_HAND,
            CursorIcon::Progress => IDC_APPSTARTING,
        };
        unsafe { SetCursor(LoadCursorW(null(), name)) };
    }

    /// Handle an interaction message of the client area, returns its result when handled.
    /// Events are sent last, their handlers can close the window and free this state.
    pub(super) unsafe fn handle(
        &self,
        hwnd: HWND,
        id: WindowId,
        dpi: u32,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        let send = |event| send_event(Event::Window(id, event));
        match message {
            WM_ACTIVATE => {
                send(if wparam & 0xffff != 0 {
                    WindowEvent::Focus
                } else {
                    WindowEvent::Blur
                });
                None
            }
            // The cursor over a window child or the frame is set by them.
            WM_SETCURSOR if wparam as HWND == hwnd && lparam as u16 == HTCLIENT => {
                self.apply_cursor();
                Some(1)
            }
            WM_MOUSEMOVE => {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    ..Default::default()
                };
                unsafe { TrackMouseEvent(&mut track) };
                send(WindowEvent::MouseMove {
                    position: pointer_position(dpi, lparam),
                    modifiers: windows_modifiers(),
                });
                Some(0)
            }
            WM_MOUSELEAVE => {
                send(WindowEvent::MouseLeave);
                Some(0)
            }
            WM_LBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_RBUTTONUP | WM_MBUTTONDOWN
            | WM_MBUTTONUP | WM_XBUTTONDOWN | WM_XBUTTONUP => {
                let pressed = matches!(
                    message,
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                );
                // Keep receiving pointer input while a button is held outside the window.
                if pressed {
                    unsafe { SetCapture(hwnd) };
                } else if wparam & MK_BUTTONS == 0 {
                    unsafe { ReleaseCapture() };
                }
                let event = MouseEvent {
                    button: mouse_button(message, wparam),
                    position: pointer_position(dpi, lparam),
                    modifiers: windows_modifiers(),
                };
                send(if pressed {
                    WindowEvent::MouseDown(event)
                } else {
                    WindowEvent::MouseUp(event)
                });
                Some(mouse_button_result(message))
            }
            WM_MOUSEWHEEL | WM_MOUSEHWHEEL => {
                let delta = (wparam >> 16) as u16 as i16 as f64 / 120.0;
                send(WindowEvent::Wheel(
                    if message == WM_MOUSEWHEEL {
                        ScrollDelta::Lines(0.0, -delta)
                    } else {
                        ScrollDelta::Lines(delta, 0.0)
                    },
                    windows_modifiers(),
                ));
                Some(0)
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
                send(if pressed {
                    WindowEvent::KeyDown(event)
                } else {
                    WindowEvent::KeyUp(event)
                });
                // Preserve native Alt+F4 and other system-key handling.
                matches!(message, WM_KEYDOWN | WM_KEYUP).then_some(0)
            }
            _ => None,
        }
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

// Extra mouse button messages must return TRUE when handled.
const fn mouse_button_result(message: u32) -> LRESULT {
    if matches!(message, WM_XBUTTONDOWN | WM_XBUTTONUP) {
        1
    } else {
        0
    }
}

fn pointer_position(dpi: u32, lparam: LPARAM) -> LogicalPoint {
    let scale = dpi.max(USER_DEFAULT_SCREEN_DPI) as f32 / USER_DEFAULT_SCREEN_DPI as f32;
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

    #[test]
    fn pointer_positions_are_logical_pixels() {
        let lparam = (200 << 16) | 100;
        let point = pointer_position(192, lparam);
        assert_eq!((point.x, point.y), (50.0, 100.0));
        let point = pointer_position(96, lparam);
        assert_eq!((point.x, point.y), (100.0, 200.0));
    }
}
