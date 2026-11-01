/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Win32 media view: a child window with native scroll bars, dark in the dark theme, that
//! draws the viewport with bcanvas.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::sync::Once;

use bcanvas::HwndCanvas;
use bwindow::{NativeViewHandle, NativeWindowHandle, Theme, Window, WindowAttachment};
use imageview_core::{Media, WHEEL_ZOOM_RATE};

use self::headers::*;
use crate::viewport::{Axis, LINE, Step, Viewport};

mod headers;

const CLASS_NAME: &str = "ImageViewMediaView";
const ANIMATION_TIMER: usize = 1;

struct State {
    hwnd: Cell<HWND>,
    canvas: RefCell<Option<HwndCanvas>>,
    viewport: RefCell<Viewport>,
    dark: Cell<bool>,
    updating_scroll_bars: Cell<bool>,
}

/// A native view that zooms and pans media.
pub(crate) struct MediaView {
    state: Box<State>,
    _attachment: WindowAttachment,
}

impl MediaView {
    /// Fills the window with the media, zoomed to fit.
    pub(crate) fn new(window: &Window, media: Media, theme: Theme) -> Self {
        let attachment = window.attach_content().expect("window has no content yet");
        // SAFETY: The window is open while its attachment is.
        let Some(NativeWindowHandle::Win32(parent)) = (unsafe { attachment.window_handle() })
        else {
            panic!("expected a Win32 window");
        };
        let size = window.size();
        let state = Box::new(State {
            hwnd: Cell::new(null_mut()),
            canvas: RefCell::new(None),
            viewport: RefCell::new(Viewport::new(
                media,
                (f64::from(size.width), f64::from(size.height)),
            )),
            dark: Cell::new(theme == Theme::Dark),
            updating_scroll_bars: Cell::new(false),
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
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_HSCROLL | WS_VSCROLL,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!hwnd.is_null(), "could not create the media view");
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*state as *const State as isize);
            state.hwnd.set(hwnd);
            *state.canvas.borrow_mut() = Some(HwndCanvas::new(hwnd));
            state.apply_theme();
            attachment.set_native_view(NativeViewHandle::Win32(hwnd));
        }
        // The view has its size now.
        state.update(Viewport::zoom_to_fit);
        state.animate();
        Self {
            state,
            _attachment: attachment,
        }
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        self.state
            .update(|viewport| viewport.set_media(media, zoom_to_fit));
        self.state.animate();
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        self.state.update(|viewport| viewport.zoom_by(factor, None));
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        self.state.update(Viewport::actual_size);
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&self) {
        self.state.update(Viewport::zoom_to_fit);
    }

    /// Follows the light or dark theme of the window.
    pub(crate) fn theme_changed(&self, theme: Theme) {
        self.state.dark.set(theme == Theme::Dark);
        self.state.apply_theme();
    }
}

impl Drop for MediaView {
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
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class.as_ptr(),
                hIconSm: null_mut(),
            });
            assert!(atom != 0, "could not register the media view class");
        }
    });
}

impl State {
    fn scale(&self) -> f64 {
        // SAFETY: The window is alive while the state is.
        f64::from(unsafe { GetDpiForWindow(self.hwnd.get()) }.max(96)) / 96.0
    }

    // A point in client pixels in logical pixels.
    fn logical(&self, (x, y): (i32, i32)) -> (f64, f64) {
        let scale = self.scale();
        (f64::from(x) / scale, f64::from(y) / scale)
    }

    // Changes the viewport, then shows the change in the scroll bars and the view.
    fn update(&self, change: impl FnOnce(&mut Viewport)) {
        change(&mut self.viewport.borrow_mut());
        self.refresh();
    }

    fn refresh(&self) {
        self.update_scroll_bars();
        self.redraw();
    }

    fn update_scroll_bars(&self) {
        // Showing or hiding a scroll bar resizes the view, which can change the other bar,
        // so the bars are set twice without reentering from the resize.
        if self.updating_scroll_bars.replace(true) {
            return;
        }
        let scale = self.scale();
        for _ in 0..2 {
            for axis in Axis::BOTH {
                let bar = self.viewport.borrow().scroll_bar(axis);
                let mut info = SCROLLINFO::new(SIF_RANGE | SIF_PAGE | SIF_POS);
                info.nMax = (bar.length * scale).round() as i32 - 1;
                info.nPage = (bar.page * scale).round() as u32;
                info.nPos = (bar.position * scale).round() as i32;
                // SAFETY: The window is alive, scroll bars hide when everything is visible.
                unsafe { SetScrollInfo(self.hwnd.get(), bar_of(axis), &info, 1) };
            }
        }
        self.updating_scroll_bars.set(false);
    }

    fn apply_theme(&self) {
        let theme = wide(if self.dark.get() {
            "DarkMode_Explorer"
        } else {
            "Explorer"
        });
        // SAFETY: The window is alive and the theme name is copied by the system.
        unsafe { SetWindowTheme(self.hwnd.get(), theme.as_ptr(), null()) };
        self.redraw();
    }

    fn redraw(&self) {
        // SAFETY: The window is alive while the state is.
        unsafe { InvalidateRect(self.hwnd.get(), null(), 0) };
    }

    fn paint(&self) {
        let mut canvas = self.canvas.borrow_mut();
        let Some(canvas) = canvas.as_mut() else {
            return;
        };
        canvas.paint(|context| self.viewport.borrow().paint(context, self.dark.get()));
    }

    // Shows the current animation frame and schedules the next one.
    fn animate(&self) {
        let (changed, next) = self.viewport.borrow_mut().advance_frame();
        if changed {
            self.redraw();
        }
        // SAFETY: The window is alive and owns its timer.
        unsafe {
            match next {
                Some(next) => SetTimer(
                    self.hwnd.get(),
                    ANIMATION_TIMER,
                    next.as_millis().max(1) as u32,
                    null(),
                ),
                None => KillTimer(self.hwnd.get(), ANIMATION_TIMER) as usize,
            };
        }
    }

    // Scrolls an axis for a scroll bar request.
    fn scroll_bar(&self, axis: Axis, request: u16) {
        let step = match request {
            SB_LINEUP => Step::LineBack,
            SB_LINEDOWN => Step::LineForward,
            SB_PAGEUP => Step::PageBack,
            SB_PAGEDOWN => Step::PageForward,
            SB_TOP => Step::Start,
            SB_BOTTOM => Step::End,
            SB_THUMBTRACK => {
                let mut info = SCROLLINFO::new(SIF_TRACKPOS);
                // SAFETY: The window is alive and the scroll info is valid.
                unsafe { GetScrollInfo(self.hwnd.get(), bar_of(axis), &mut info) };
                let position = f64::from(info.nTrackPos) / self.scale();
                self.update(|viewport| viewport.scroll_axis(axis, position));
                return;
            }
            _ => return,
        };
        self.update(|viewport| viewport.step(axis, step));
    }

    // Control + wheel magnifies around the pointer, shift + wheel scrolls horizontally.
    fn wheel(&self, wparam: WPARAM, lparam: LPARAM, horizontal: bool) {
        let lines = f64::from((wparam >> 16) as u16 as i16) / WHEEL_DELTA;
        // SAFETY: GetKeyState reads the keyboard state of this thread.
        let (control, shift) = unsafe { (GetKeyState(VK_CONTROL) < 0, GetKeyState(VK_SHIFT) < 0) };
        if control && !horizontal {
            let mut point = POINT {
                x: (lparam & 0xffff) as i16 as i32,
                y: ((lparam >> 16) & 0xffff) as i16 as i32,
            };
            // SAFETY: The window is alive, wheel positions are in screen coordinates.
            unsafe { ScreenToClient(self.hwnd.get(), &mut point) };
            let anchor = self.logical((point.x, point.y));
            self.update(|viewport| {
                viewport.zoom_by((lines * WHEEL_ZOOM_RATE).exp(), Some(anchor));
            });
        } else if horizontal || shift {
            let sign = if horizontal { 1.0 } else { -1.0 };
            self.update(|viewport| viewport.scroll_by((sign * lines * LINE, 0.0)));
        } else {
            self.update(|viewport| viewport.scroll_by((0.0, -lines * LINE)));
        }
    }

    fn key(&self, key: usize) -> bool {
        let step = match key {
            VK_UP => Step::LineBack,
            VK_DOWN => Step::LineForward,
            VK_PRIOR => Step::PageBack,
            VK_NEXT => Step::PageForward,
            VK_HOME => Step::Start,
            VK_END => Step::End,
            _ => return false,
        };
        self.update(|viewport| viewport.step(Axis::Vertical, step));
        true
    }
}

const fn bar_of(axis: Axis) -> i32 {
    match axis {
        Axis::Horizontal => SB_HORZ,
        Axis::Vertical => SB_VERT,
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
    let point = (
        (lparam & 0xffff) as i16 as i32,
        ((lparam >> 16) & 0xffff) as i16 as i32,
    );
    match message {
        WM_PAINT => state.paint(),
        WM_ERASEBKGND => return 1,
        WM_SIZE => {
            let mut rect = RECT::default();
            // SAFETY: The window is alive.
            unsafe { GetClientRect(hwnd, &mut rect) };
            let visible = state.logical((rect.right, rect.bottom));
            state.update(|viewport| viewport.resize(visible));
        }
        WM_HSCROLL => state.scroll_bar(Axis::Horizontal, wparam as u16),
        WM_VSCROLL => state.scroll_bar(Axis::Vertical, wparam as u16),
        WM_MOUSEWHEEL => state.wheel(wparam, lparam, false),
        WM_MOUSEHWHEEL => state.wheel(wparam, lparam, true),
        WM_KEYDOWN if state.key(wparam) => {}
        WM_TIMER if wparam == ANIMATION_TIMER => state.animate(),
        WM_LBUTTONDOWN => {
            // SAFETY: The window is alive and takes the focus and mouse capture.
            unsafe { SetFocus(hwnd) };
            if state.viewport.borrow_mut().drag_begin(state.logical(point)) {
                // SAFETY: The window is alive.
                unsafe { SetCapture(hwnd) };
            }
        }
        WM_MOUSEMOVE => {
            if state.viewport.borrow_mut().drag_to(state.logical(point)) {
                state.refresh();
            }
        }
        // SAFETY: The window is alive and releases its own capture.
        WM_LBUTTONUP if unsafe { GetCapture() } == hwnd => unsafe {
            ReleaseCapture();
        },
        WM_CAPTURECHANGED => {
            state.viewport.borrow_mut().drag_end();
        }
        WM_SETCURSOR if lparam as u16 == HTCLIENT && state.viewport.borrow().is_scrollable() => {
            // SAFETY: The system cursor lives for the process lifetime.
            unsafe { SetCursor(LoadCursorW(null_mut(), IDC_SIZEALL)) };
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
