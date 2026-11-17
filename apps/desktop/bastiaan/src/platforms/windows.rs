/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Win32 scroll view: a child window with a native vertical scroll bar, dark in the dark
//! theme, that bcanvas paints. It works on Windows XP too, the dark scroll bar needs Windows 10.

#![allow(non_snake_case, clippy::upper_case_acronyms)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Once;

use bcanvas::{CanvasRenderingContext2d, HwndCanvas};
use bwindow::{
    LogicalPoint, Modifiers, MouseButton, MouseEvent, NativeViewHandle, NativeWindowHandle, Window,
    WindowAttachment, WindowEvent,
};

use super::LINE;

type HWND = *mut c_void;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;

const CLASS_NAME: &str = "BastiaanScrollView";
const REPAINT_TIMER: usize = 1;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const GWLP_USERDATA: i32 = -21;
const WM_SIZE: u32 = 0x0005;
const WM_PAINT: u32 = 0x000f;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_SETCURSOR: u32 = 0x0020;
const WM_NCDESTROY: u32 = 0x0082;
const WM_KEYDOWN: u32 = 0x0100;
const WM_TIMER: u32 = 0x0113;
const WM_VSCROLL: u32 = 0x0115;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_MOUSEWHEEL: u32 = 0x020a;
const WM_MOUSELEAVE: u32 = 0x02a3;
const SB_VERT: i32 = 1;
const SB_LINEUP: u16 = 0;
const SB_LINEDOWN: u16 = 1;
const SB_PAGEUP: u16 = 2;
const SB_PAGEDOWN: u16 = 3;
const SB_THUMBTRACK: u16 = 5;
const SB_TOP: u16 = 6;
const SB_BOTTOM: u16 = 7;
const SIF_RANGE: u32 = 0x1;
const SIF_PAGE: u32 = 0x2;
const SIF_POS: u32 = 0x4;
const SIF_TRACKPOS: u32 = 0x10;
const VK_PRIOR: usize = 0x21;
const VK_NEXT: usize = 0x22;
const VK_END: usize = 0x23;
const VK_HOME: usize = 0x24;
const VK_UP: usize = 0x26;
const VK_DOWN: usize = 0x28;
const TME_LEAVE: u32 = 0x2;
const HTCLIENT: u16 = 1;
const IDC_ARROW: *const u16 = 32512 as *const u16;
const IDC_HAND: *const u16 = 32649 as *const u16;

#[repr(C)]
#[derive(Default)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct SCROLLINFO {
    cbSize: u32,
    fMask: u32,
    nMin: i32,
    nMax: i32,
    nPage: u32,
    nPos: i32,
    nTrackPos: i32,
}

impl SCROLLINFO {
    const fn new(mask: u32) -> Self {
        Self {
            cbSize: size_of::<Self>() as u32,
            fMask: mask,
            nMin: 0,
            nMax: 0,
            nPage: 0,
            nPos: 0,
            nTrackPos: 0,
        }
    }
}

#[repr(C)]
struct TRACKMOUSEEVENT {
    cbSize: u32,
    dwFlags: u32,
    hwndTrack: HWND,
    dwHoverTime: u32,
}

#[repr(C)]
struct WNDCLASSEXW {
    cbSize: u32,
    style: u32,
    lpfnWndProc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    cbClsExtra: i32,
    cbWndExtra: i32,
    hInstance: *mut c_void,
    hIcon: *mut c_void,
    hCursor: *mut c_void,
    hbrBackground: *mut c_void,
    lpszMenuName: *const u16,
    lpszClassName: *const u16,
    hIconSm: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WNDCLASSEXW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: HWND,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> HWND;
    fn DestroyWindow(hwnd: HWND) -> i32;
    fn IsWindow(hwnd: HWND) -> i32;
    fn DefWindowProcW(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    // 32-bit Windows only has the 32-bit functions, of the same size there
    #[cfg_attr(target_pointer_width = "32", link_name = "GetWindowLongW")]
    fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    #[cfg_attr(target_pointer_width = "32", link_name = "SetWindowLongW")]
    fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
    fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> i32;
    fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: i32) -> i32;
    fn ValidateRect(hwnd: HWND, rect: *const RECT) -> i32;
    fn SetScrollInfo(hwnd: HWND, bar: i32, info: *const SCROLLINFO, redraw: i32) -> i32;
    fn GetScrollInfo(hwnd: HWND, bar: i32, info: *mut SCROLLINFO) -> i32;
    fn SetTimer(hwnd: HWND, id: usize, elapse: u32, function: *const c_void) -> usize;
    fn KillTimer(hwnd: HWND, id: usize) -> i32;
    fn TrackMouseEvent(event: *mut TRACKMOUSEEVENT) -> i32;
    fn SetFocus(hwnd: HWND) -> HWND;
    fn SetCapture(hwnd: HWND) -> HWND;
    fn GetCapture() -> HWND;
    fn ReleaseCapture() -> i32;
    fn LoadCursorW(instance: *mut c_void, name: *const u16) -> *mut c_void;
    fn SetCursor(cursor: *mut c_void) -> *mut c_void;
}

#[link(name = "uxtheme")]
unsafe extern "system" {
    fn SetWindowTheme(hwnd: HWND, app_name: *const u16, id_list: *const u16) -> i32;
}

struct State {
    attachment: WindowAttachment,
    hwnd: Cell<HWND>,
    canvas: RefCell<Option<HwndCanvas>>,
    content_height: Cell<f32>,
    scroll: Cell<f32>,
    // Whether the window is in its WM_PAINT, the only time the canvas paints.
    painting: Cell<bool>,
    pointer: Cell<bool>,
    tracking: Cell<bool>,
    updating_scroll_bar: Cell<bool>,
}

/// A native view that scrolls a page vertically.
pub(crate) struct ScrollView {
    state: Box<State>,
}

impl ScrollView {
    /// Fills the window with an empty page.
    pub(crate) fn new(window: &Window) -> Self {
        let attachment = window.attach_content().expect("window has no content yet");
        // SAFETY: The window is open while its attachment is.
        let Some(NativeWindowHandle::Win32(parent)) = (unsafe { attachment.window_handle() })
        else {
            unreachable!("expected a Win32 window");
        };
        let state = Box::new(State {
            attachment,
            hwnd: Cell::new(null_mut()),
            canvas: RefCell::new(None),
            content_height: Cell::new(0.0),
            scroll: Cell::new(0.0),
            painting: Cell::new(false),
            pointer: Cell::new(false),
            tracking: Cell::new(false),
            updating_scroll_bar: Cell::new(false),
        });
        register_class();
        let class = wide(CLASS_NAME);
        // SAFETY: The class is registered and the boxed state outlives the window, which this
        // view destroys before dropping it.
        unsafe {
            let hwnd = CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_VSCROLL,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!hwnd.is_null(), "could not create the scroll view");
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &raw const *state as isize);
            state.hwnd.set(hwnd);
            *state.canvas.borrow_mut() = Some(HwndCanvas::new(hwnd));
            state
                .attachment
                .set_native_view(NativeViewHandle::Win32(hwnd));
        }
        // Redraws, also the animation frames, paint through the view
        let hwnd = state.hwnd.get();
        state.attachment.on_redraw(move || {
            // SAFETY: Invalidating a destroyed window fails harmlessly.
            unsafe { InvalidateRect(hwnd, null(), 0) };
        });
        Self { state }
    }

    /// Paints the visible part of the page during the window's `RedrawRequested`, returns
    /// whether it painted.
    pub(crate) fn draw(&self, draw: impl FnOnce(&mut CanvasRenderingContext2d<'_>)) -> bool {
        let state = &self.state;
        let mut canvas = state.canvas.borrow_mut();
        let Some(canvas) = canvas.as_mut().filter(|_| state.painting.get()) else {
            return false;
        };
        canvas.paint(draw);
        true
    }

    /// The scroll position in logical pixels.
    pub(crate) fn scroll(&self) -> f32 {
        self.state.scroll.get()
    }

    /// Sets the height of the page, which the scroll bar scrolls through.
    pub(crate) fn set_content_height(&self, height: f32) {
        if height != self.state.content_height.replace(height) {
            self.state.update_scroll_bar();
        }
    }

    /// Paints the page again.
    pub(crate) fn request_redraw(&self) {
        self.state.redraw();
    }

    /// Paints the page again at the next display frame.
    pub(crate) fn request_animation_frame(&self) {
        self.state.attachment.request_animation_frame();
    }

    /// Shows a pointing hand over the page instead of the arrow.
    pub(crate) fn set_pointer_cursor(&self, pointer: bool) {
        self.state.pointer.set(pointer);
    }

    /// Follows the light or dark theme with the scroll bar.
    pub(crate) fn set_dark(&self, dark: bool) {
        let theme = wide(if dark {
            "DarkMode_Explorer"
        } else {
            "Explorer"
        });
        // SAFETY: The window is alive and the theme name is copied by the system.
        unsafe { SetWindowTheme(self.state.hwnd.get(), theme.as_ptr(), null()) };
    }
}

impl Drop for ScrollView {
    fn drop(&mut self) {
        let hwnd = self.state.hwnd.get();
        // SAFETY: The window procedure forgets the state before it is freed.
        unsafe {
            if IsWindow(hwnd) != 0 {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                DestroyWindow(hwnd);
            }
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

fn register_class() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let class = wide(CLASS_NAME);
        // SAFETY: The class structure is valid and its name is copied by the system.
        unsafe {
            let atom = RegisterClassExW(&WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: window_proc,
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: GetModuleHandleW(null()),
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class.as_ptr(),
                hIconSm: null_mut(),
            });
            assert!(atom != 0, "could not register the scroll view class");
        }
    });
}

impl State {
    fn scale(&self) -> f32 {
        self.attachment.scale_factor()
    }

    fn view_height(&self) -> f32 {
        let mut rect = RECT::default();
        // SAFETY: The window is alive while the state is.
        unsafe { GetClientRect(self.hwnd.get(), &mut rect) };
        rect.bottom as f32 / self.scale()
    }

    fn redraw(&self) {
        // SAFETY: The window is alive while the state is.
        unsafe { InvalidateRect(self.hwnd.get(), null(), 0) };
    }

    fn scroll_to(&self, scroll: f32) {
        let max = (self.content_height.get() - self.view_height()).max(0.0);
        let scroll = scroll.clamp(0.0, max).round();
        if scroll != self.scroll.replace(scroll) {
            self.update_scroll_bar();
            self.redraw();
        }
    }

    fn update_scroll_bar(&self) {
        // Showing or hiding the scroll bar resizes the view, which sets it again
        if self.updating_scroll_bar.replace(true) {
            return;
        }
        let scale = self.scale();
        let mut info = SCROLLINFO::new(SIF_RANGE | SIF_PAGE | SIF_POS);
        info.nMax = (self.content_height.get() * scale).round() as i32 - 1;
        info.nPage = (self.view_height() * scale).round() as u32;
        info.nPos = (self.scroll.get() * scale).round() as i32;
        // SAFETY: The window is alive, the scroll bar hides when the page fits.
        unsafe { SetScrollInfo(self.hwnd.get(), SB_VERT, &info, 1) };
        self.updating_scroll_bar.set(false);
        self.scroll_to(self.scroll.get());
    }

    fn scroll_bar(&self, request: u16) {
        let (scroll, page) = (self.scroll.get(), self.view_height());
        match request {
            SB_LINEUP => self.scroll_to(scroll - LINE),
            SB_LINEDOWN => self.scroll_to(scroll + LINE),
            SB_PAGEUP => self.scroll_to(scroll - page),
            SB_PAGEDOWN => self.scroll_to(scroll + page),
            SB_TOP => self.scroll_to(0.0),
            SB_BOTTOM => self.scroll_to(f32::MAX),
            SB_THUMBTRACK => {
                let mut info = SCROLLINFO::new(SIF_TRACKPOS);
                // SAFETY: The window is alive and the scroll info is valid.
                unsafe { GetScrollInfo(self.hwnd.get(), SB_VERT, &mut info) };
                self.scroll_to(info.nTrackPos as f32 / self.scale());
            }
            _ => {}
        }
    }

    fn key(&self, key: usize) -> bool {
        let (scroll, page) = (self.scroll.get(), self.view_height());
        let scroll = match key {
            VK_UP => scroll - LINE,
            VK_DOWN => scroll + LINE,
            VK_PRIOR => scroll - page,
            VK_NEXT => scroll + page,
            VK_HOME => 0.0,
            VK_END => f32::MAX,
            _ => return false,
        };
        self.scroll_to(scroll);
        true
    }

    // Paints through the window's RedrawRequested, or later when the event loop is busy.
    fn paint(&self) {
        self.painting.set(true);
        let drawn = self.attachment.event_sender().redraw(|| {});
        self.painting.set(false);
        let hwnd = self.hwnd.get();
        // SAFETY: The window is alive, a frame that wasn't drawn is drawn by the timer.
        unsafe {
            ValidateRect(hwnd, null());
            if !drawn {
                SetTimer(hwnd, REPAINT_TIMER, 16, null());
            }
        }
    }

    fn send_mouse(&self, message: u32, lparam: LPARAM) {
        let scale = self.scale();
        let position = LogicalPoint::new(
            f32::from(lparam as u16 as i16) / scale,
            f32::from((lparam >> 16) as u16 as i16) / scale,
        );
        let event = MouseEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::NONE,
        };
        self.attachment.event_sender().send(match message {
            WM_LBUTTONDOWN => WindowEvent::MouseDown(event),
            WM_LBUTTONUP => WindowEvent::MouseUp(event),
            _ => WindowEvent::MouseMove {
                position,
                movement: LogicalPoint::new(0.0, 0.0),
                modifiers: Modifiers::NONE,
            },
        });
    }
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: The user data is the state of the view, which clears it before freeing it.
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() };
    let Some(state) = state else {
        // SAFETY: Messages without state get the default handling.
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };
    match message {
        WM_PAINT => state.paint(),
        WM_ERASEBKGND => return 1,
        WM_TIMER if wparam == REPAINT_TIMER => {
            // SAFETY: The window is alive and owns its timer.
            unsafe { KillTimer(hwnd, REPAINT_TIMER) };
            state.redraw();
        }
        WM_SIZE => {
            state.update_scroll_bar();
            state.redraw();
        }
        WM_VSCROLL => state.scroll_bar(wparam as u16),
        WM_MOUSEWHEEL => {
            let notches = f32::from((wparam >> 16) as u16 as i16) / 120.0;
            state.scroll_to(state.scroll.get() - notches * 3.0 * LINE);
        }
        WM_KEYDOWN if state.key(wparam) => {}
        WM_MOUSEMOVE => {
            if !state.tracking.replace(true) {
                let mut event = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                // SAFETY: The window is alive and the structure is valid.
                unsafe { TrackMouseEvent(&mut event) };
            }
            state.send_mouse(message, lparam);
        }
        WM_MOUSELEAVE => {
            state.tracking.set(false);
            state
                .attachment
                .event_sender()
                .send(WindowEvent::MouseLeave);
        }
        WM_LBUTTONDOWN => {
            // SAFETY: The window is alive and takes the focus and mouse capture.
            unsafe {
                SetFocus(hwnd);
                SetCapture(hwnd);
            }
            state.send_mouse(message, lparam);
        }
        WM_LBUTTONUP => {
            // SAFETY: The window is alive and releases its own capture.
            unsafe {
                if GetCapture() == hwnd {
                    ReleaseCapture();
                }
            }
            state.send_mouse(message, lparam);
        }
        WM_SETCURSOR if lparam as u16 == HTCLIENT => {
            let cursor = if state.pointer.get() {
                IDC_HAND
            } else {
                IDC_ARROW
            };
            // SAFETY: The system cursors live for the process lifetime.
            unsafe { SetCursor(LoadCursorW(null_mut(), cursor)) };
            return 1;
        }
        WM_NCDESTROY => {
            // SAFETY: The window is going away, so it forgets the state.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
            state.hwnd.set(null_mut());
        }
        // SAFETY: Other messages get the default handling.
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    0
}
