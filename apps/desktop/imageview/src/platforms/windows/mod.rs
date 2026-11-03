/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Windows media view: a WinUI 3 XAML island filling the window, with a header bar in the
//! title bar and a scroll viewer that zooms and pans the media over a checkerboard, following
//! the system light or dark theme. Media is drawn into XAML bitmaps, vector media again at the
//! magnification it settles at so it stays sharp.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Once;
use std::time::Instant;

use bcanvas::OffscreenCanvas;
use bwindow::{
    EventLoopProxy, NativeViewHandle, NativeWindowHandle, Theme, Window, WindowAttachment,
};
use imageview_core::{
    Media, MediaRenderer, clamp_magnification, draw_checkerboard, fit_magnification, window_title,
};

use self::headers::*;
use self::winui::{Bitmap, HEADER_HEIGHT, Island, MIN_WIDTH};
use crate::AppEvent;

mod headers;
mod winrt;
mod winui;

const CLASS_NAME: &str = "ImageViewContent";
const FRAME_SUBCLASS: usize = 1;
const ANIMATION_TIMER: usize = 1;
// The longest side of a media bitmap in pixels, larger media is drawn scaled down.
const MAX_BITMAP: f64 = 8192.0;
// The checkerboard grows in steps of this many pixels, so resizing rarely draws it again.
const CHECKERBOARD_STEP: u32 = 512;

/// The smallest window, wide enough for the header bar.
pub(crate) const MIN_SIZE: (f32, f32) = (MIN_WIDTH as f32, 240.0);

thread_local! {
    static PROXY: RefCell<Option<EventLoopProxy<AppEvent>>> = const { RefCell::new(None) };
}

/// Loads WinUI and keeps the proxy the header bar buttons send their actions with.
pub(crate) fn init(proxy: EventLoopProxy<AppEvent>) {
    winui::init();
    PROXY.with(|stored| *stored.borrow_mut() = Some(proxy));
}

fn send_action(action: &'static str) {
    PROXY.with(|proxy| {
        if let Some(proxy) = proxy.borrow().as_ref() {
            _ = proxy.send_user_event(AppEvent::Action(action));
        }
    });
}

struct State {
    // The window filling the client area, which hosts the island.
    content: Cell<HWND>,
    // The top-level window.
    window: Cell<HWND>,
    island: RefCell<Option<Island>>,
    renderer: RefCell<Option<MediaRenderer>>,
    bitmap: RefCell<Option<Bitmap>>,
    // The checkerboard bitmap, whether it is dark and its scale.
    checkerboard: RefCell<Option<(Bitmap, bool, f64)>>,
    dark: Cell<bool>,
    // The shown animation frame and when the animation started.
    frame: Cell<usize>,
    start: Cell<Instant>,
    // The loading indicator shown until the first media is decoded.
    spinner: Cell<HWND>,
}

/// A native scroll view that zooms and pans media.
pub(crate) struct MediaView {
    state: Rc<State>,
    _attachment: WindowAttachment,
}

impl MediaView {
    /// Fills the window with the media, zoomed to fit.
    pub(crate) fn new(window: &Window, media: Option<Media>, theme: Theme) -> Self {
        let loading = media.is_none();
        let attachment = window.attach_content().expect("window has no content yet");
        // SAFETY: The window is open while its attachment is.
        let Some(NativeWindowHandle::Win32(parent)) = (unsafe { attachment.window_handle() })
        else {
            panic!("expected a Win32 window");
        };
        let state = Rc::new(State {
            content: Cell::new(null_mut()),
            window: Cell::new(parent),
            island: RefCell::new(None),
            renderer: RefCell::new(media.map(MediaRenderer::new)),
            bitmap: RefCell::new(None),
            checkerboard: RefCell::new(None),
            dark: Cell::new(theme == Theme::Dark),
            frame: Cell::new(0),
            start: Cell::new(Instant::now()),
            spinner: Cell::new(null_mut()),
        });
        register_class();
        let class = wide(CLASS_NAME);
        // SAFETY: The class is registered and the state outlives the window, which this view
        // destroys before dropping the state.
        let content = unsafe {
            let content = CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS | WS_CLIPCHILDREN,
                0,
                0,
                0,
                0,
                parent,
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            assert!(!content.is_null(), "could not create the media view");
            SetWindowLongPtrW(content, GWLP_USERDATA, Rc::as_ptr(&state) as isize);
            content
        };
        state.content.set(content);

        // The window was sized for the media, which goes below the header bar.
        let mut media = RECT::default();
        // SAFETY: The window is alive.
        unsafe { GetClientRect(parent, &mut media) };
        let zoomed = Rc::downgrade(&state);
        let island = Island::new(content, parent, state.dark.get(), send_action, move || {
            if let Some(state) = zoomed.upgrade() {
                state.zoomed();
            }
        });
        fit_below_header(parent, &media);
        // SAFETY: GetActiveWindow reads the state of this thread.
        island.set_active(unsafe { GetActiveWindow() } == parent);
        *state.island.borrow_mut() = Some(island);
        state.update_heading();
        // SAFETY: The windows are alive, the subclass is removed before the state is freed.
        unsafe {
            SetWindowSubclass(
                parent,
                frame_proc,
                FRAME_SUBCLASS,
                Rc::as_ptr(&state) as usize,
            );
            attachment.set_native_view(NativeViewHandle::Win32(content));
        }
        if loading {
            state.add_spinner();
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
        let spinner = state.spinner.replace(null_mut());
        if !spinner.is_null() {
            // SAFETY: The spinner is a live child window owned by the content.
            unsafe { DestroyWindow(spinner) };
        }
        *state.renderer.borrow_mut() = Some(MediaRenderer::new(media));
        // A new bitmap shows the new media, also when it has the same size.
        state.bitmap.borrow_mut().take();
        state.frame.set(0);
        state.start.set(Instant::now());
        state.update_heading();
        if zoom_to_fit {
            state.zoom_to_fit();
        } else {
            state.render(state.zoom_factor());
        }
        state.start_animation();
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        self.state.zoom_by(factor);
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        let state = &self.state;
        let (view, media) = (state.viewport(), state.media_size());
        state.change_view(((media.0 - view.0) / 2.0, (media.1 - view.1) / 2.0), 1.0);
    }

    /// Shows all of the media.
    pub(crate) fn zoom_to_fit(&self) {
        self.state.zoom_to_fit();
    }

    /// Follows the light or dark theme of the window.
    pub(crate) fn theme_changed(&self, theme: Theme) {
        let state = &self.state;
        state.dark.set(theme == Theme::Dark);
        if let Some(island) = state.island.borrow().as_ref() {
            island.set_dark(theme == Theme::Dark);
        }
        state.update_checkerboard();
        // SAFETY: The content is alive while the state is.
        unsafe { InvalidateRect(state.content.get(), null(), 1) };
    }
}

impl Drop for MediaView {
    fn drop(&mut self) {
        let state = &self.state;
        // SAFETY: The window procedure and the subclass forget the state before it is freed,
        // destroying the content also destroys the island inside it.
        unsafe {
            let window = state.window.get();
            if IsWindow(window) != 0 {
                RemoveWindowSubclass(window, frame_proc, FRAME_SUBCLASS);
            }
            state.island.borrow_mut().take();
            let content = state.content.get();
            if IsWindow(content) != 0 {
                SetWindowLongPtrW(content, GWLP_USERDATA, 0);
                DestroyWindow(content);
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
                lpfnWndProc: content_proc,
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

// Resizes a window around its center so the area below the header bar is `media`, the client
// area it had before its content extended into the title bar.
fn fit_below_header(window: HWND, media: &RECT) {
    let (mut client, mut frame) = (RECT::default(), RECT::default());
    // SAFETY: The window is alive.
    let scale = unsafe {
        GetClientRect(window, &mut client);
        GetWindowRect(window, &mut frame);
        f64::from(GetDpiForWindow(window).max(96)) / 96.0
    };
    let header = (HEADER_HEIGHT * scale).round() as i32;
    let grow = (
        media.right - client.right,
        media.bottom + header - client.bottom,
    );
    if grow == (0, 0) {
        return;
    }
    // SAFETY: The window is alive, it is not shown yet.
    unsafe {
        SetWindowPos(
            window,
            null_mut(),
            frame.left - grow.0 / 2,
            frame.top - grow.1 / 2,
            frame.right - frame.left + grow.0,
            frame.bottom - frame.top + grow.1,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    };
}

fn window_text(hwnd: HWND) -> String {
    // SAFETY: The buffer holds the text and its terminator.
    unsafe {
        let mut text = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        String::from_utf16_lossy(&text[..length.max(0) as usize])
    }
}

// Converts straight-alpha RGBA pixels to the premultiplied BGRA pixels of a bitmap.
fn premultiplied_bgra(rgba: &[u8]) -> Vec<u8> {
    let mut pixels = rgba.to_vec();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let [red, green, blue, alpha] = *pixel;
        let premultiply = |channel: u8| (u16::from(channel) * u16::from(alpha) / 255) as u8;
        *pixel = [
            premultiply(blue),
            premultiply(green),
            premultiply(red),
            alpha,
        ];
    }
    pixels
}

impl State {
    // Shows a native marquee progress bar in the middle of the view.
    fn add_spinner(&self) {
        let class = wide("msctls_progress32");
        // SAFETY: The progress bar class is registered by comctl32, the view owns the child.
        unsafe {
            InitCommonControlsEx(&INITCOMMONCONTROLSEX {
                dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_PROGRESS_CLASS,
            });
            let spinner = CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | PBS_MARQUEE,
                0,
                0,
                0,
                0,
                self.content.get(),
                null_mut(),
                GetModuleHandleW(null()),
                null_mut(),
            );
            if spinner.is_null() {
                return;
            }
            SendMessageW(spinner, PBM_SETMARQUEE, 1, 0);
            self.spinner.set(spinner);
        }
        self.layout_spinner();
    }

    // Keeps the progress bar in the middle of the view.
    fn layout_spinner(&self) {
        const SIZE: (f64, f64) = (160.0, 12.0);
        let spinner = self.spinner.get();
        if spinner.is_null() {
            return;
        }
        let mut rect = RECT::default();
        let scale = self.scale();
        let (width, height) = ((SIZE.0 * scale) as i32, (SIZE.1 * scale) as i32);
        // SAFETY: The view and its progress bar are alive.
        unsafe {
            GetClientRect(self.content.get(), &mut rect);
            MoveWindow(
                spinner,
                (rect.right - width) / 2,
                (rect.bottom - height) / 2,
                width,
                height,
                1,
            );
        }
    }

    fn scale(&self) -> f64 {
        // SAFETY: The content is alive while the state is.
        f64::from(unsafe { GetDpiForWindow(self.content.get()) }.max(96)) / 96.0
    }

    // The area of the scroll viewer in logical pixels.
    fn viewport(&self) -> (f64, f64) {
        let mut rect = RECT::default();
        // SAFETY: The content is alive while the state is.
        unsafe { GetClientRect(self.content.get(), &mut rect) };
        let scale = self.scale();
        (
            f64::from(rect.right) / scale,
            (f64::from(rect.bottom) / scale - HEADER_HEIGHT).max(0.0),
        )
    }

    fn media_size(&self) -> (f64, f64) {
        self.renderer
            .borrow()
            .as_ref()
            .map_or((1.0, 1.0), |renderer| renderer.media().size())
    }

    fn zoom_factor(&self) -> f64 {
        self.island
            .borrow()
            .as_ref()
            .map_or(1.0, Island::zoom_factor)
    }

    // Scrolls to `offsets` of the media at `magnification` and draws it for that size.
    fn change_view(&self, offsets: (f64, f64), magnification: f64) {
        let magnification = clamp_magnification(magnification);
        if let Some(island) = self.island.borrow().as_ref() {
            island.change_view(Some(offsets.0), Some(offsets.1), Some(magnification));
        }
        self.render(magnification);
    }

    // Magnifies by `factor`, keeping the middle of the visible area in place.
    fn zoom_by(&self, factor: f64) {
        let (Some((x, y)), magnification) = (
            self.island.borrow().as_ref().map(Island::offsets),
            self.zoom_factor(),
        ) else {
            return;
        };
        let (view, media) = (self.viewport(), self.media_size());
        let target = clamp_magnification(magnification * factor);
        // The media is centered while it is smaller than the view.
        let margin = |view: f64, media: f64, magnification: f64| {
            ((view - media * magnification) / 2.0).max(0.0)
        };
        let middle = |offset: f64, view: f64, media: f64| {
            let point = (offset + view / 2.0 - margin(view, media, magnification)) / magnification;
            point * target + margin(view, media, target) - view / 2.0
        };
        self.change_view(
            (middle(x, view.0, media.0), middle(y, view.1, media.1)),
            target,
        );
    }

    fn zoom_to_fit(&self) {
        let magnification = fit_magnification(self.viewport(), self.media_size());
        self.change_view((0.0, 0.0), magnification);
    }

    // The scroll viewer settled, vector media is drawn again for its magnification.
    fn zoomed(&self) {
        if self
            .renderer
            .borrow()
            .as_ref()
            .is_some_and(|renderer| matches!(renderer.media(), Media::Vector(_)))
        {
            self.render(self.zoom_factor());
        }
    }

    // The pixel size of the media bitmap: raster media at its own size, vector media at the size
    // it is shown, both at most the largest bitmap.
    fn bitmap_size(&self, magnification: f64) -> (u32, u32) {
        let renderer = self.renderer.borrow();
        let Some(renderer) = renderer.as_ref() else {
            return (1, 1);
        };
        let (width, height) = match renderer.media() {
            Media::Raster(raster) => (f64::from(raster.width), f64::from(raster.height)),
            Media::Vector(_) => {
                let (width, height) = renderer.media().size();
                let scale = magnification * self.scale();
                (width * scale, height * scale)
            }
        };
        let shrink = (MAX_BITMAP / width.max(height)).min(1.0);
        (
            (width * shrink).round().max(1.0) as u32,
            (height * shrink).round().max(1.0) as u32,
        )
    }

    // Draws the shown frame of the media into its bitmap, which gets the size the media is
    // shown at.
    fn render(&self, magnification: f64) {
        let size = self.bitmap_size(magnification);
        let renderer = self.renderer.borrow();
        let Some(renderer) = renderer.as_ref() else {
            return;
        };
        let mut bitmap = self.bitmap.borrow_mut();
        let fresh = bitmap.as_ref().is_none_or(|bitmap| bitmap.size() != size);
        if fresh {
            *bitmap = Bitmap::new(size.0, size.1);
        } else if matches!(renderer.media(), Media::Vector(_)) {
            // Vector media has one frame, which is drawn at this size already.
            return;
        }
        let Some(target) = bitmap.as_ref() else {
            return;
        };
        match renderer.media() {
            Media::Raster(raster) if (raster.width, raster.height) == size => {
                if let Some(frame) = raster.frames.get(self.frame.get()) {
                    target.write(&frame.pixels);
                }
            }
            _ => {
                let mut canvas = OffscreenCanvas::new(size.0, size.1);
                canvas.draw(|context| {
                    renderer.draw(
                        context,
                        self.frame.get(),
                        (0.0, 0.0, size.0 as f32, size.1 as f32),
                    );
                });
                target.write(&premultiplied_bgra(canvas.pixels()));
            }
        }
        if fresh && let Some(island) = self.island.borrow().as_ref() {
            island.set_media(target, renderer.media().size());
        }
    }

    // Draws the checkerboard behind the media when the view outgrew it or the theme changed.
    // It is centered and drawn larger than the view, so its tiles stay in place while resizing.
    fn update_checkerboard(&self) {
        let island = self.island.borrow();
        let Some(island) = island.as_ref() else {
            return;
        };
        let mut rect = RECT::default();
        // SAFETY: The content is alive while the state is.
        unsafe { GetClientRect(self.content.get(), &mut rect) };
        let scale = self.scale();
        let step =
            |pixels: i32| (pixels.max(1) as u32).div_ceil(CHECKERBOARD_STEP) * CHECKERBOARD_STEP;
        let needed = (step(rect.right), step(rect.bottom));
        let dark = self.dark.get();
        let mut checkerboard = self.checkerboard.borrow_mut();
        if checkerboard
            .as_ref()
            .is_some_and(|(bitmap, drawn_dark, drawn_scale)| {
                *drawn_dark == dark
                    && *drawn_scale == scale
                    && bitmap.size().0 >= needed.0
                    && bitmap.size().1 >= needed.1
            })
        {
            return;
        }
        let logical = (
            (f64::from(needed.0) / scale).ceil() as u32,
            (f64::from(needed.1) / scale).ceil() as u32,
        );
        let mut canvas = OffscreenCanvas::with_scale_factor(logical.0, logical.1, scale as f32);
        canvas.draw(|context| {
            draw_checkerboard(context, logical.0 as f32, logical.1 as f32, dark);
        });
        let Some(bitmap) = Bitmap::new(canvas.width(), canvas.height()) else {
            return;
        };
        bitmap.write(&premultiplied_bgra(canvas.pixels()));
        island.set_checkerboard(
            &bitmap,
            (
                f64::from(canvas.width()) / scale,
                f64::from(canvas.height()) / scale,
            ),
        );
        *checkerboard = Some((bitmap, dark, scale));
    }

    // Shows the current animation frame and schedules the next one.
    fn start_animation(&self) {
        let (frame, next) = {
            let renderer = self.renderer.borrow();
            let Some(renderer) = renderer.as_ref() else {
                return;
            };
            renderer.media().frame_at(self.start.get().elapsed())
        };
        if self.frame.replace(frame) != frame {
            self.render(self.zoom_factor());
        }
        // SAFETY: The content is alive and owns its timer.
        unsafe {
            match next {
                Some(next) => SetTimer(
                    self.content.get(),
                    ANIMATION_TIMER,
                    next.as_millis().max(1) as u32,
                    null(),
                ),
                None => KillTimer(self.content.get(), ANIMATION_TIMER) as usize,
            };
        }
    }

    // Shows the file name of the window title in the header bar, with the media size below it.
    fn update_heading(&self) {
        let island = self.island.borrow();
        let Some(island) = island.as_ref() else {
            return;
        };
        if self.renderer.borrow().is_none() {
            island.set_title(&window_text(self.window.get()), "Loading...");
            return;
        }
        let (width, height) = self.media_size();
        let title = window_text(self.window.get());
        let name = title
            .strip_suffix(&window_title("", (width, height)))
            .unwrap_or(&title);
        let subtitle = format!("{} × {}", width.round() as i64, height.round() as i64);
        island.set_title(name, &subtitle);
    }

    // Fills the content with the island and draws the checkerboard for its size, and vector
    // media for a changed scale.
    fn layout(&self, width: i32, height: i32) {
        self.layout_spinner();
        if let Some(island) = self.island.borrow().as_ref() {
            island.move_and_resize(width, height, self.scale());
        }
        self.update_checkerboard();
        self.zoomed();
    }
}

unsafe extern "system" fn content_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: The user data is the state of the view, which clears it before freeing it.
    let state = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State).as_ref() };
    if let Some(state) = state {
        match message {
            WM_SIZE => {
                state.layout((lparam & 0xffff) as i32, ((lparam >> 16) & 0xffff) as i32);
                return 0;
            }
            // The theme background shows where the island has not drawn yet, like while it
            // catches up with a resize.
            WM_ERASEBKGND => {
                let mut rect = RECT::default();
                // SAFETY: The device context is valid while the message is handled.
                unsafe {
                    GetClientRect(hwnd, &mut rect);
                    let brush = CreateSolidBrush(if state.dark.get() {
                        0x0020_2020
                    } else {
                        0x00f3_f3f3
                    });
                    FillRect(wparam as *mut c_void, &rect, brush);
                    DeleteObject(brush);
                }
                return 1;
            }
            // Keyboard input goes to the scroll viewer.
            WM_SETFOCUS => {
                if let Some(island) = state.island.borrow().as_ref() {
                    island.focus();
                }
                return 0;
            }
            WM_TIMER if wparam == ANIMATION_TIMER => {
                state.start_animation();
                return 0;
            }
            _ => {}
        }
    }
    // SAFETY: Other messages get the default handling.
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

// Follows the activation of the top-level window in the island, and closes it with the window.
unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: The data is the state of the view, which removes this subclass before freeing it.
    let state = unsafe { (data as *const State).as_ref() };
    // The window is destroyed before the view is dropped, the island closes while the windows
    // it lives in still exist.
    if message == WM_DESTROY
        && let Some(state) = state
        && let Ok(mut island) = state.island.try_borrow_mut()
    {
        island.take();
    }
    // SAFETY: Messages pass through to the window first.
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    if message == WM_ACTIVATE
        && let Some(island) = state.and_then(|state| state.island.try_borrow().ok())
        && let Some(island) = island.as_ref()
    {
        island.set_active(wparam & 0xffff != WA_INACTIVE);
    }
    result
}
