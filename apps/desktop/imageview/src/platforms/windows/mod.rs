/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Win32 media view: a child window with native scroll bars, dark in the dark theme, that
//! draws the media with bcanvas above a checkerboard.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::sync::Once;
use std::time::Instant;

use bcanvas::HwndCanvas;
use bwindow::{NativeViewHandle, NativeWindowHandle, Theme, Window, WindowAttachment};
use imageview_core::{
    Media, MediaRenderer, WHEEL_ZOOM_RATE, clamp_magnification, draw_checkerboard,
    fit_magnification,
};

use self::headers::*;

mod headers;

const CLASS_NAME: &str = "ImageViewMediaView";
const ANIMATION_TIMER: usize = 1;
// How far a scroll bar arrow or wheel line scrolls, in logical pixels.
const LINE: f64 = 48.0;

// Where a drag started: the pointer position and the scroll position.
type Drag = ((i32, i32), (f64, f64));

// One scroll axis of the view.
#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

impl Axis {
    const fn of(self, (x, y): (f64, f64)) -> f64 {
        match self {
            Self::Horizontal => x,
            Self::Vertical => y,
        }
    }

    const fn bar(self) -> i32 {
        match self {
            Self::Horizontal => SB_HORZ,
            Self::Vertical => SB_VERT,
        }
    }
}

struct State {
    hwnd: Cell<HWND>,
    canvas: RefCell<Option<HwndCanvas>>,
    renderer: RefCell<MediaRenderer>,
    magnification: Cell<f64>,
    // The scroll position in logical pixels.
    scroll: Cell<(f64, f64)>,
    dark: Cell<bool>,
    updating_scroll_bars: Cell<bool>,
    // The shown animation frame and when the animation started.
    frame: Cell<usize>,
    start: Cell<Instant>,
    drag: Cell<Option<Drag>>,
}

/// A native scroll view that zooms and pans media.
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
        let state = Box::new(State {
            hwnd: Cell::new(null_mut()),
            canvas: RefCell::new(None),
            renderer: RefCell::new(MediaRenderer::new(media)),
            magnification: Cell::new(1.0),
            scroll: Cell::new((0.0, 0.0)),
            dark: Cell::new(theme == Theme::Dark),
            updating_scroll_bars: Cell::new(false),
            frame: Cell::new(0),
            start: Cell::new(Instant::now()),
            drag: Cell::new(None),
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
        state.zoom_to_fit();
        state.start_animation();
        Self {
            state,
            _attachment: attachment,
        }
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        let state = &self.state;
        *state.renderer.borrow_mut() = MediaRenderer::new(media);
        state.frame.set(0);
        state.start.set(Instant::now());
        if zoom_to_fit {
            state.zoom_to_fit();
        } else {
            state.magnify(state.magnification.get(), None);
        }
        state.start_animation();
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        self.state
            .magnify(self.state.magnification.get() * factor, None);
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        self.state.magnify(1.0, None);
        self.state.center();
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&self) {
        self.state.zoom_to_fit();
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
    // The visible area in logical pixels.
    fn view(&self) -> (f64, f64) {
        let mut rect = RECT::default();
        // SAFETY: The window is alive while the state is.
        unsafe { GetClientRect(self.hwnd.get(), &mut rect) };
        let scale = self.scale();
        (
            f64::from(rect.right) / scale,
            f64::from(rect.bottom) / scale,
        )
    }

    fn scale(&self) -> f64 {
        // SAFETY: The window is alive while the state is.
        f64::from(unsafe { GetDpiForWindow(self.hwnd.get()) }.max(96)) / 96.0
    }

    // The magnified media size in logical pixels.
    fn content(&self) -> (f64, f64) {
        let (width, height) = self.renderer.borrow().media().size();
        let magnification = self.magnification.get();
        (width * magnification, height * magnification)
    }

    // Where the media starts in the view, it is centered when smaller than the view.
    fn offset(&self, axis: Axis) -> f64 {
        ((axis.of(self.view()) - axis.of(self.content())) / 2.0).max(0.0)
    }

    // Scrolls to a position kept inside the media, updating the scroll bars.
    fn scroll_to(&self, (x, y): (f64, f64)) {
        let clamp = |axis: Axis, value: f64| {
            value.clamp(
                0.0,
                (axis.of(self.content()) - axis.of(self.view())).max(0.0),
            )
        };
        self.scroll
            .set((clamp(Axis::Horizontal, x), clamp(Axis::Vertical, y)));
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
            for axis in [Axis::Horizontal, Axis::Vertical] {
                let mut info = SCROLLINFO::new(SIF_RANGE | SIF_PAGE | SIF_POS);
                info.nMax = (axis.of(self.content()) * scale).round() as i32 - 1;
                info.nPage = (axis.of(self.view()) * scale).round() as u32;
                info.nPos = (axis.of(self.scroll.get()) * scale).round() as i32;
                // SAFETY: The window is alive, scroll bars hide when everything is visible.
                unsafe { SetScrollInfo(self.hwnd.get(), axis.bar(), &info, 1) };
            }
        }
        self.updating_scroll_bars.set(false);
    }

    // Magnifies the media, keeping the media under `anchor` in the view in place, or the
    // middle of the view.
    fn magnify(&self, magnification: f64, anchor: Option<(f64, f64)>) {
        let before = self.magnification.get();
        let view = self.view();
        let anchor = anchor.unwrap_or((view.0 / 2.0, view.1 / 2.0));
        let media = [Axis::Horizontal, Axis::Vertical].map(|axis| {
            (axis.of(self.scroll.get()) + axis.of(anchor) - self.offset(axis)) / before
        });
        self.magnification.set(clamp_magnification(magnification));
        let after = self.magnification.get();
        self.scroll_to((
            media[0] * after + self.offset(Axis::Horizontal) - anchor.0,
            media[1] * after + self.offset(Axis::Vertical) - anchor.1,
        ));
    }

    fn center(&self) {
        let (content, view) = (self.content(), self.view());
        self.scroll_to(((content.0 - view.0) / 2.0, (content.1 - view.1) / 2.0));
    }

    fn zoom_to_fit(&self) {
        let magnification = fit_magnification(self.view(), self.renderer.borrow().media().size());
        self.magnify(magnification, None);
        self.center();
    }

    fn scroll_by(&self, (x, y): (f64, f64)) {
        let (scroll_x, scroll_y) = self.scroll.get();
        self.scroll_to((scroll_x + x, scroll_y + y));
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
        let (scroll_x, scroll_y) = self.scroll.get();
        let (content_width, content_height) = self.content();
        let rect = (
            (self.offset(Axis::Horizontal) - scroll_x) as f32,
            (self.offset(Axis::Vertical) - scroll_y) as f32,
            content_width as f32,
            content_height as f32,
        );
        let mut canvas = self.canvas.borrow_mut();
        let Some(canvas) = canvas.as_mut() else {
            return;
        };
        canvas.paint(|context| {
            draw_checkerboard(context, context.width(), context.height(), self.dark.get());
            self.renderer.borrow().draw(context, self.frame.get(), rect);
        });
    }

    // Shows the current animation frame and schedules the next one.
    fn start_animation(&self) {
        let (frame, next) = self
            .renderer
            .borrow()
            .media()
            .frame_at(self.start.get().elapsed());
        if self.frame.replace(frame) != frame {
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
        let mut info = SCROLLINFO::new(SIF_TRACKPOS);
        // SAFETY: The window is alive and the scroll info is valid.
        unsafe { GetScrollInfo(self.hwnd.get(), axis.bar(), &mut info) };
        let page = axis.of(self.view());
        let current = axis.of(self.scroll.get());
        let value = match request {
            SB_LINEUP => current - LINE,
            SB_LINEDOWN => current + LINE,
            SB_PAGEUP => current - page,
            SB_PAGEDOWN => current + page,
            SB_THUMBTRACK => f64::from(info.nTrackPos) / self.scale(),
            SB_TOP => 0.0,
            SB_BOTTOM => f64::INFINITY,
            _ => return,
        };
        let (x, y) = self.scroll.get();
        self.scroll_to(match axis {
            Axis::Horizontal => (value, y),
            Axis::Vertical => (x, value),
        });
    }

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
            let scale = self.scale();
            self.magnify(
                self.magnification.get() * (lines * WHEEL_ZOOM_RATE).exp(),
                Some((f64::from(point.x) / scale, f64::from(point.y) / scale)),
            );
        } else if horizontal || shift {
            self.scroll_by((lines * LINE * if horizontal { 1.0 } else { -1.0 }, 0.0));
        } else {
            self.scroll_by((0.0, -lines * LINE));
        }
    }

    fn key(&self, key: usize) -> bool {
        let page = self.view().1;
        match key {
            VK_UP => self.scroll_by((0.0, -LINE)),
            VK_DOWN => self.scroll_by((0.0, LINE)),
            VK_PRIOR => self.scroll_by((0.0, -page)),
            VK_NEXT => self.scroll_by((0.0, page)),
            VK_HOME => self.scroll_to((self.scroll.get().0, 0.0)),
            VK_END => self.scroll_to((self.scroll.get().0, f64::INFINITY)),
            _ => return false,
        }
        true
    }

    // The media can be dragged when it does not fit.
    fn is_scrollable(&self) -> bool {
        let (content, view) = (self.content(), self.view());
        content.0 > view.0 || content.1 > view.1
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
        WM_SIZE => state.scroll_to(state.scroll.get()),
        WM_HSCROLL => state.scroll_bar(Axis::Horizontal, wparam as u16),
        WM_VSCROLL => state.scroll_bar(Axis::Vertical, wparam as u16),
        WM_MOUSEWHEEL => state.wheel(wparam, lparam, false),
        WM_MOUSEHWHEEL => state.wheel(wparam, lparam, true),
        WM_KEYDOWN if state.key(wparam) => {}
        WM_TIMER if wparam == ANIMATION_TIMER => state.start_animation(),
        WM_LBUTTONDOWN => {
            // SAFETY: The window is alive and takes the focus and mouse capture.
            unsafe { SetFocus(hwnd) };
            if state.is_scrollable() {
                state.drag.set(Some((point, state.scroll.get())));
                // SAFETY: The window is alive.
                unsafe { SetCapture(hwnd) };
            }
        }
        WM_MOUSEMOVE => {
            if let Some(((start_x, start_y), (scroll_x, scroll_y))) = state.drag.get() {
                let scale = state.scale();
                state.scroll_to((
                    scroll_x - f64::from(point.0 - start_x) / scale,
                    scroll_y - f64::from(point.1 - start_y) / scale,
                ));
            }
        }
        // SAFETY: The window is alive and releases its own capture.
        WM_LBUTTONUP if unsafe { GetCapture() } == hwnd => unsafe {
            ReleaseCapture();
        },
        WM_CAPTURECHANGED => state.drag.set(None),
        WM_SETCURSOR if lparam as u16 == HTCLIENT && state.is_scrollable() => {
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
