/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Windows media view: a WinUI 3 XAML island filling the window, with a header bar in the
//! title bar and a scroll viewer that zooms and pans the media over a checkerboard, following
//! the system light or dark theme. Media is drawn into XAML bitmaps, vector media again at the
//! magnification it settles at so it stays sharp. While editing, the header bar shows the edit
//! controls, a card the controls of the open tool, and an overlay the crop rectangle or
//! selection, which the shared viewport follows the scroll viewer for.

#![allow(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::rc::Rc;
use std::sync::Once;

use bcanvas::OffscreenCanvas;
use bwindow::{
    EventLoopProxy, NativeViewHandle, NativeWindowHandle, Theme, Window, WindowAttachment, WindowId,
};
use imageview_core::edit::{Control, Selection, Shape, Tool};
use imageview_core::{
    InfoSection, Media, clamp_magnification, draw_checkerboard, fit_magnification, window_title,
};

use self::headers::*;
use self::panels::Panels;
pub(crate) use self::save::SaveOptions;
use self::tool::ToolPanel;
use self::winui::{Bitmap, HEADER_HEIGHT, Handlers, Island, MIN_WIDTH, Mark, Panel};
use crate::AppEvent;
use crate::viewport::Viewport;

mod headers;
mod panels;
mod save;
mod tool;
mod winrt;
mod winui;

const CLASS_NAME: &[u16] = w!("ImageViewContent");
const FRAME_SUBCLASS: usize = 1;
const ANIMATION_TIMER: usize = 1;
const REVEAL_TIMER: usize = 2;
// How long a new window waits at most for the first frame of the island before it shows.
const REVEAL_TIMEOUT_MS: u32 = 1000;
// Posted by bwindow to a new window to show it, see `WM_REVEAL` in its window module.
const WM_REVEAL: u32 = WM_USER + 17;
// Posted to the content once the island has rendered its first frame.
const WM_RENDERED: u32 = WM_APP + 1;
// The longest side of a media bitmap in pixels, larger media is drawn scaled down.
const MAX_BITMAP: f64 = 8192.0;
// The checkerboard grows in steps of this many pixels, so resizing rarely draws it again.
const CHECKERBOARD_STEP: u32 = 512;

/// The smallest window, wide enough for the header bar.
pub(crate) const MIN_SIZE: (f32, f32) = (MIN_WIDTH as f32, 240.0);
pub(crate) const CONTENT_HEADER_HEIGHT: f32 = HEADER_HEIGHT as f32;

thread_local! {
    static PROXY: RefCell<Option<EventLoopProxy<AppEvent>>> = const { RefCell::new(None) };
}

/// Loads WinUI and keeps the proxy the header bar buttons send their actions with.
pub(crate) fn init(proxy: EventLoopProxy<AppEvent>) {
    winui::init();
    PROXY.with(|stored| *stored.borrow_mut() = Some(proxy));
}

pub(crate) fn shutdown() {
    PROXY.with_borrow_mut(|proxy| *proxy = None);
    winui::shutdown();
}

// Sends an event to the application.
fn send(event: AppEvent) {
    PROXY.with(|proxy| {
        if let Some(proxy) = proxy.borrow().as_ref() {
            _ = proxy.send_user_event(event);
        }
    });
}

struct State {
    // The window filling the client area, which hosts the island.
    content: Cell<HWND>,
    // The top-level window.
    window: Cell<HWND>,
    island: RefCell<Option<Island>>,
    // The media with its animation, preview, crop rectangle and selection, which follows the
    // scroll viewer.
    viewport: RefCell<Viewport>,
    // Whether the first media is decoded.
    loaded: Cell<bool>,
    bitmap: RefCell<Option<Bitmap>>,
    // Enlarged raster pixels at display resolution, bypassing XAML's linear scaling.
    pixels: RefCell<Option<Bitmap>>,
    pixel_canvas: RefCell<Option<OffscreenCanvas>>,
    // The bitmap of the crop rectangle or selection over the visible area.
    overlay: RefCell<Option<Bitmap>>,
    // The controls of the open edit tool.
    tool: RefCell<Option<ToolPanel>>,
    // The checkerboard bitmap, whether it is dark and its scale.
    checkerboard: RefCell<Option<(Bitmap, bool, f64)>>,
    dark: Cell<bool>,
    // Whether the window draws the Mica backdrop itself, behind transparent window surfaces.
    window_backdrop: Cell<bool>,
    // Whether the window may show, and whether it held back showing until then.
    revealed: Cell<bool>,
    reveal_held: Cell<bool>,
    // The sidebar with the pages and the inspector beside the image.
    panels: Panels,
}

/// A native scroll view that zooms and pans media.
pub(crate) struct MediaView {
    dialog: Option<winui::Dialog>,
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
            viewport: RefCell::new(Viewport::new(media, (1.0, 1.0))),
            loaded: Cell::new(!loading),
            bitmap: RefCell::new(None),
            pixels: RefCell::new(None),
            pixel_canvas: RefCell::new(None),
            overlay: RefCell::new(None),
            tool: RefCell::new(None),
            checkerboard: RefCell::new(None),
            dark: Cell::new(theme == Theme::Dark),
            window_backdrop: Cell::new(set_window_backdrop(parent)),
            revealed: Cell::new(false),
            reveal_held: Cell::new(false),
            panels: Panels::new(window.id()),
        });
        state.update_background();
        register_class();
        let class = CLASS_NAME;
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

        let (viewed, marked) = (Rc::downgrade(&state), Rc::downgrade(&state));
        let island = Island::new(
            content,
            parent,
            state.dark.get(),
            state.window_backdrop.get(),
            Handlers {
                action: Box::new(|action| send(AppEvent::Action(action))),
                view: Box::new(move |settled| {
                    if let Some(state) = viewed.upgrade() {
                        state.view_changed(settled);
                    }
                }),
                mark: Box::new(move |mark| marked.upgrade().is_some_and(|state| state.mark(mark))),
                // Handled outside of the rendering of the island, which it stops following.
                // SAFETY: Posting to a destroyed window does nothing.
                rendered: Box::new(move || unsafe {
                    PostMessageW(content, WM_RENDERED, 0, 0);
                }),
            },
        );
        // SAFETY: GetActiveWindow reads the state of this thread.
        island.set_active(unsafe { GetActiveWindow() } == parent);
        // Only still bitmaps can be edited, the loading media has no frames.
        island.set_editable(!loading && state.viewport.borrow().media().is_still());
        island.set_spinner(loading);
        state.panels.show(&island);
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
            SetTimer(content, REVEAL_TIMER, REVEAL_TIMEOUT_MS, null());
        }
        state.zoom_to_fit();
        state.start_animation();
        Self {
            dialog: None,
            state,
            _attachment: attachment,
        }
    }

    /// Replaces the media, keeping the zoom or fitting it to the window.
    pub(crate) fn set_media(&mut self, media: Media, zoom_to_fit: bool) {
        let state = &self.state;
        // Vectors and animations cannot be edited, only saved as bitmaps.
        if let Some(island) = state.island.borrow().as_ref() {
            island.set_editable(media.is_still());
            if !state.loaded.get() {
                island.set_spinner(false);
            }
        }
        state.viewport.borrow_mut().set_media(media, zoom_to_fit);
        state.loaded.set(true);
        // A new bitmap shows the new media, also when it has the same size.
        state.bitmap.borrow_mut().take();
        state.update_heading();
        if zoom_to_fit {
            state.zoom_to_fit();
        } else {
            state.render(state.zoom_factor());
        }
        state.start_animation();
        state.draw_marks();
    }

    /// The shown media.
    pub(crate) fn media(&self) -> Media {
        self.state.viewport.borrow().media().clone()
    }

    pub(crate) fn show_alert(&mut self, id: WindowId, title: &str, description: &str, save: bool) {
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_spinner(false);
            match island.show_alert(title, description, save, move |result| {
                send(AppEvent::AlertClosed(id, result))
            }) {
                Ok(dialog) => self.dialog = Some(dialog),
                Err(error) => {
                    eprintln!("Could not show alert: {title}: {description} (0x{error:08x})");
                    send(AppEvent::AlertClosed(id, 0));
                }
            }
        }
    }

    pub(crate) fn close_alert(&mut self) {
        self.dialog.take();
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.focus();
        }
    }

    /// Magnifies by `factor`, keeping the middle of the visible area in place.
    pub(crate) fn zoom_by(&self, factor: f64) {
        self.state.zoom_by(factor);
    }

    /// Shows the media at its natural size.
    pub(crate) fn actual_size(&self) {
        let state = &self.state;
        let (view, media) = (state.visible(), state.media_size());
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
        state.update_background();
        state.update_checkerboard();
        // SAFETY: The content is alive while the state is.
        unsafe { InvalidateRect(state.content.get(), null(), 1) };
    }

    /// Shows the edit controls in the header bar, or the view controls again.
    pub(crate) fn set_editing(&mut self, editing: bool) {
        self.close_tool();
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_editing(editing);
        }
        // Only the shown page is edited, so the other pages and the information are hidden
        // meanwhile, the media fits the room that is left.
        let shown = self.state.panels.width();
        self.state.panels.set_hidden(editing);
        self.state.show_panels(self.state.panels.width() != shown);
    }

    /// Lists the pages of a file with several in the sidebar with one selected, or hides it.
    pub(crate) fn set_pages(&self, pages: &[Media], page: usize) {
        let shown = self.state.panels.width();
        if let Some(island) = self.state.island.borrow().as_ref()
            && let Err(error) = self.state.panels.set_pages(island, pages, page)
        {
            eprintln!("Could not show the pages: 0x{error:08x}");
        }
        // The media fits the space that is left when the sidebar comes or goes.
        self.state.show_panels(self.state.panels.width() != shown);
    }

    /// Selects the shown page in the sidebar.
    pub(crate) fn select_page(&self, page: usize) {
        self.state.panels.select(page);
    }

    /// Opens or closes the sidebar with the pages, when there is one.
    pub(crate) fn toggle_sidebar(&self) {
        self.state.panels.toggle(Panel::Pages);
        self.state.show_panels(false);
    }

    /// Opens or closes the inspector with the information about the image.
    pub(crate) fn toggle_info(&self) {
        self.state.panels.toggle(Panel::Info);
        self.state.show_panels(false);
    }

    /// Lists the information about the image in the inspector.
    pub(crate) fn set_info(&self, sections: &[InfoSection]) {
        if let Some(island) = self.state.island.borrow().as_ref()
            && let Err(error) = self.state.panels.set_sections(island, sections)
        {
            eprintln!("Could not show the information: 0x{error:08x}");
        }
    }

    /// Shows the controls of a tool in a card over the image, and the crop rectangle for a crop.
    /// Changes of the controls go to `window`. Reset goes back to the defaults.
    pub(crate) fn show_tool(
        &mut self,
        window: WindowId,
        tool: Tool,
        controls: &[Control],
        values: &[f32],
        defaults: &[f32],
        _action: &str,
    ) {
        self.close_tool();
        // Without a card there is no tool that closing would remove the crop rectangle of.
        let Ok(panel) = ToolPanel::new(window, tool, controls, values, defaults) else {
            return;
        };
        if tool == Tool::Crop {
            self.state
                .change_marks(|viewport| viewport.set_cropping(true));
        }
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_tool(Some(panel.content()));
        }
        *self.state.tool.borrow_mut() = Some(panel);
    }

    /// Shows changed values of the open tool.
    pub(crate) fn set_tool_values(&self, values: &[f32]) {
        if let Some(panel) = self.state.tool.borrow().as_ref() {
            panel.set_values(values);
        }
    }

    /// Closes the controls and crop rectangle of the open tool.
    pub(crate) fn close_tool(&mut self) {
        if self.state.tool.borrow_mut().take().is_none() {
            return;
        }
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_tool(None);
            island.focus();
        }
        self.state
            .change_marks(|viewport| viewport.set_cropping(false));
    }

    /// Makes drags draw selections of a shape, or selects the whole image again, shown as on in
    /// the header bar.
    pub(crate) fn set_selecting(&self, shape: Option<Shape>) {
        self.state
            .change_marks(|viewport| viewport.set_selecting(shape));
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_toggled(match shape {
                None => "select.whole",
                Some(Shape::Rectangle) => "select.rectangle",
                Some(Shape::Ellipse) => "select.ellipse",
            });
        }
    }

    /// The selected part of the media.
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.state.viewport.borrow().selection()
    }

    /// Selects everything outside the selection instead.
    pub(crate) fn invert_selection(&self) {
        self.state.change_marks(Viewport::invert_selection);
    }

    /// Puts a PNG image on the clipboard, in the format of that name.
    pub(crate) fn copy_png(&self, png: &[u8]) {
        let format = w!("PNG");
        // SAFETY: The clipboard takes over the global memory once it has it, which is freed
        // here otherwise.
        unsafe {
            if OpenClipboard(self.state.window.get()) == 0 {
                return;
            }
            EmptyClipboard();
            let memory = GlobalAlloc(GMEM_MOVEABLE, png.len());
            let target = GlobalLock(memory);
            if !target.is_null() {
                std::ptr::copy_nonoverlapping(png.as_ptr(), target.cast(), png.len());
                GlobalUnlock(memory);
                if SetClipboardData(RegisterClipboardFormatW(format.as_ptr()), memory).is_null() {
                    GlobalFree(memory);
                }
            } else if !memory.is_null() {
                GlobalFree(memory);
            }
            CloseClipboard();
        }
    }

    /// Shows a preview of any size in the place of the image, or the image again.
    pub(crate) fn set_preview(&self, preview: Option<Media>) {
        self.state.viewport.borrow_mut().set_preview(preview);
        self.state.render(self.state.zoom_factor());
    }

    /// Places the crop rectangle at a position with a size in pixels.
    pub(crate) fn set_crop_rect(&self, rect: (u32, u32, u32, u32)) {
        self.state.change_marks(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_rect(rect);
            }
        });
    }

    /// Makes the crop rectangle keep a width divided by height, or any shape.
    pub(crate) fn set_crop_aspect(&self, aspect: Option<f64>) {
        self.state.change_marks(|viewport| {
            if let Some(crop) = viewport.crop() {
                crop.set_aspect(aspect);
            }
        });
    }

    /// The crop rectangle in pixels, or all of the media without one.
    pub(crate) fn crop_rect(&self) -> (u32, u32, u32, u32) {
        let mut viewport = self.state.viewport.borrow_mut();
        let (width, height) = viewport.media().size();
        viewport
            .crop()
            .map_or((0, 0, width as u32, height as u32), |crop| crop.rect())
    }

    /// The longest side of the view in device pixels, which previews do not need to exceed.
    pub(crate) fn longest_pixels(&self) -> u32 {
        let (width, height) = self.state.visible();
        (width.max(height) * self.state.scale()).ceil().max(1.0) as u32
    }

    /// Shows a progress bar while an edit is applied.
    pub(crate) fn set_busy(&self, busy: bool) {
        if let Some(island) = self.state.island.borrow().as_ref() {
            island.set_spinner(busy || !self.state.loaded.get());
        }
    }
}

impl Drop for MediaView {
    fn drop(&mut self) {
        self.dialog.take();
        // Hide immediately; releasing the XAML tree may wait for composition work.
        // SAFETY: The parent window belongs to this view and may already be destroyed.
        unsafe { ShowWindow(self.state.window.get(), 0) };
        self.close_tool();
        let state = &self.state;
        // SAFETY: The window procedure and the subclass forget the state before it is freed,
        // destroying the content also destroys the island inside it.
        unsafe {
            let window = state.window.get();
            if IsWindow(window) != 0 {
                RemoveWindowSubclass(window, frame_proc, FRAME_SUBCLASS);
            }
            // Closing XAML can synchronously move focus back to the content window.
            // Release the RefCell borrow before it runs those native callbacks.
            let island = state.island.borrow_mut().take();
            drop(island);
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
        let class = CLASS_NAME;
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

// Lets the window draw the Mica backdrop behind its whole client area, where Windows supports
// it. The backdrop is part of the window frame, so it fills the pixels a resize uncovers right
// away, where the island and the client area catch up a frame later. Black in the client area is
// transparent.
fn set_window_backdrop(window: HWND) -> bool {
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let backdrop = DWMSBT_MAINWINDOW;
    // SAFETY: The window is alive and the attribute takes a DWM_SYSTEMBACKDROP_TYPE.
    unsafe {
        DwmSetWindowAttribute(
            window,
            DWMWA_SYSTEMBACKDROP_TYPE,
            (&raw const backdrop).cast(),
            size_of::<u32>() as u32,
        ) >= 0
            && DwmExtendFrameIntoClientArea(window, &margins) >= 0
    }
}

// The brush the window surfaces behind the island are filled with: transparent over the Mica
// of the window, or else the background color of the theme. The brushes live as long as the
// process.
fn background_brush(window_backdrop: bool, dark: bool) -> *mut c_void {
    thread_local! {
        static THEME_BRUSHES: [usize; 2] =
            // SAFETY: Creating a brush has no preconditions.
            unsafe { [CreateSolidBrush(0x00f3_f3f3), CreateSolidBrush(0x0020_2020)] }
                .map(|brush| brush as usize);
    }
    if window_backdrop {
        // SAFETY: Stock objects need no cleanup.
        unsafe { GetStockObject(BLACK_BRUSH) }
    } else {
        THEME_BRUSHES.with(|brushes| brushes[usize::from(dark)] as *mut c_void)
    }
}

// Fills a window with the background brush, for WM_ERASEBKGND.
fn erase_background(state: &State, hwnd: HWND, hdc: WPARAM) -> LRESULT {
    let mut rect = RECT::default();
    // SAFETY: The device context is valid while the message is handled.
    unsafe {
        GetClientRect(hwnd, &mut rect);
        FillRect(
            hdc as *mut c_void,
            &rect,
            background_brush(state.window_backdrop.get(), state.dark.get()),
        );
    }
    1
}

// Reads the window title, which also supplies the heading of the XAML island.
fn window_text(hwnd: HWND) -> String {
    // SAFETY: The buffer holds the text and its terminator.
    unsafe {
        let mut text = vec![0u16; GetWindowTextLengthW(hwnd).max(0) as usize + 1];
        let length = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
        String::from_utf16_lossy(&text[..length.max(0) as usize])
    }
}

impl State {
    fn scale(&self) -> f64 {
        // SAFETY: The content is alive while the state is.
        f64::from(unsafe { GetDpiForWindow(self.content.get()) }.max(96)) / 96.0
    }

    // The area of the scroll viewer in logical pixels, beside the shown panels.
    fn visible(&self) -> (f64, f64) {
        let mut rect = RECT::default();
        // SAFETY: The content is alive while the state is.
        unsafe { GetClientRect(self.content.get(), &mut rect) };
        let scale = self.scale();
        (
            (f64::from(rect.right) / scale - self.panels.width()).max(0.0),
            (f64::from(rect.bottom) / scale - HEADER_HEIGHT).max(0.0),
        )
    }

    // Shows the panels as they are, fitting the media to the space left when asked.
    fn show_panels(&self, zoom_to_fit: bool) {
        if let Some(island) = self.island.borrow().as_ref() {
            self.panels.show(island);
        }
        if zoom_to_fit {
            self.zoom_to_fit();
        }
    }

    fn media_size(&self) -> (f64, f64) {
        if self.loaded.get() {
            self.viewport.borrow().media().size()
        } else {
            (1.0, 1.0)
        }
    }

    fn zoom_factor(&self) -> f64 {
        self.island
            .borrow()
            .as_ref()
            .map_or(1.0, Island::zoom_factor)
    }

    // Scrolls to `offsets` of the media at `magnification` and draws it for that size. Loading
    // media has no size to zoom, the media zooms once it is decoded.
    fn change_view(&self, offsets: (f64, f64), magnification: f64) {
        if !self.loaded.get() {
            return;
        }
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
        let (view, media) = (self.visible(), self.media_size());
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
        let magnification = fit_magnification(self.visible(), self.media_size());
        self.change_view((0.0, 0.0), magnification);
    }

    // The scroll viewer zoomed or scrolled: the crop rectangle or selection follows it, and vector
    // media is drawn again for its magnification once it settled.
    fn view_changed(&self, settled: bool) {
        self.draw_marks();
        if settled {
            self.zoomed();
        }
    }

    // The scroll viewer settled, vector media is drawn again for its magnification.
    fn zoomed(&self) {
        if matches!(self.viewport.borrow().media(), Media::Vector(_)) {
            self.render(self.zoom_factor());
        }
    }

    // Lets the viewport follow the scroll viewer, so it places the crop rectangle and selection.
    fn follow(&self) {
        let offsets = self
            .island
            .borrow()
            .as_ref()
            .map_or((0.0, 0.0), Island::offsets);
        let (visible, magnification) = (self.visible(), self.zoom_factor());
        self.viewport
            .borrow_mut()
            .follow(visible, magnification, offsets);
    }

    // Takes a drag that crops or selects, returns whether it does.
    fn mark(&self, mark: Mark) -> bool {
        self.follow();
        let marked = {
            let mut viewport = self.viewport.borrow_mut();
            match mark {
                Mark::Begin(pointer) => viewport.mark_begin(pointer),
                Mark::Drag(pointer, square) => viewport.mark_drag(pointer, square),
                Mark::End => {
                    let marked = viewport.mark_end();
                    // A click without a drag goes back to editing the whole image, the menu
                    // follows a drawn selection.
                    if marked && viewport.selecting().is_some() && viewport.selection().is_none() {
                        send(AppEvent::Action("select.whole"));
                    } else if marked {
                        send(AppEvent::Action("select.changed"));
                    }
                    marked
                }
            }
        };
        if marked {
            self.draw_marks();
        }
        marked
    }

    // Changes the crop rectangle or selection and shows it.
    fn change_marks(&self, change: impl FnOnce(&mut Viewport)) {
        change(&mut self.viewport.borrow_mut());
        self.draw_marks();
    }

    // Draws the crop rectangle or selection over the visible area, or hides the overlay.
    fn draw_marks(&self) {
        self.follow();
        self.draw_pixels();
        let island = self.island.borrow();
        let Some(island) = island.as_ref() else {
            return;
        };
        let viewport = self.viewport.borrow();
        if !viewport.has_marks() {
            island.set_overlay(None);
            self.overlay.borrow_mut().take();
            return;
        }
        let (width, height) = self.visible();
        let logical = (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32);
        let mut canvas =
            OffscreenCanvas::with_scale_factor(logical.0, logical.1, self.scale() as f32);
        canvas.draw(|context| viewport.paint_marks(context));
        let size = (canvas.width(), canvas.height());
        let mut overlay = self.overlay.borrow_mut();
        if overlay.as_ref().is_none_or(|bitmap| bitmap.size() != size) {
            *overlay = Bitmap::new(size.0, size.1);
        }
        let Some(bitmap) = overlay.as_ref() else {
            return;
        };
        bitmap.write_rgba(canvas.pixels());
        island.set_overlay(Some((bitmap, (f64::from(logical.0), f64::from(logical.1)))));
    }

    fn draw_pixels(&self) {
        let island = self.island.borrow();
        let Some(island) = island.as_ref() else {
            return;
        };
        let (width, height) = self.visible();
        let scale = self.scale() as f32;
        let mut pixels = self.pixels.borrow_mut();
        let mut pixel_canvas = self.pixel_canvas.borrow_mut();
        if !self.loaded.get() || !self.viewport.borrow().has_pixels(scale) {
            if pixels.take().is_some() {
                island.set_pixels(None);
            }
            pixel_canvas.take();
            return;
        }
        let logical = (width.ceil().max(1.0) as u32, height.ceil().max(1.0) as u32);
        if pixel_canvas.as_ref().is_none_or(|canvas| {
            canvas.logical_width() != logical.0 as f32
                || canvas.logical_height() != logical.1 as f32
                || canvas.scale_factor() != scale
        }) {
            *pixel_canvas = Some(OffscreenCanvas::with_scale_factor(
                logical.0, logical.1, scale,
            ));
        }
        let canvas = pixel_canvas.as_mut().expect("pixel canvas initialized");
        canvas.draw(|context| {
            // Reused canvases must also erase pixels where the image became transparent or
            // moved away. Include the last physical pixel at fractional display scales.
            context.clear_rect(
                0.0,
                0.0,
                width.ceil() as f32 + 1.0,
                height.ceil() as f32 + 1.0,
            );
            self.viewport.borrow().paint_pixels(context);
        });
        let size = (canvas.width(), canvas.height());
        if pixels.as_ref().is_none_or(|bitmap| bitmap.size() != size) {
            *pixels = Bitmap::new(size.0, size.1);
        }
        let Some(bitmap) = pixels.as_ref() else {
            island.set_pixels(None);
            return;
        };
        bitmap.write_rgba(canvas.pixels());
        // The canvas rounds its native dimensions up. Using the original logical dimensions
        // here would shrink it by a fraction of a pixel and reintroduce linear filtering.
        island.set_pixels(Some((
            bitmap,
            (
                f64::from(size.0) / f64::from(scale),
                f64::from(size.1) / f64::from(scale),
            ),
        )));
    }

    // The pixel size of the bitmap of media: raster media at its own size, vector media at the
    // size it is shown, both at most the largest bitmap.
    fn bitmap_size(&self, media: &Media, magnification: f64) -> (u32, u32) {
        let (width, height) = match media {
            Media::Raster(raster) => (f64::from(raster.width), f64::from(raster.height)),
            Media::Vector(_) => {
                let (width, height) = media.size();
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

    // Draws the shown frame of the media, or the preview of an edit, into its bitmap, which gets
    // the size the media is shown at.
    fn render(&self, magnification: f64) {
        if !self.loaded.get() {
            return;
        }
        let viewport = self.viewport.borrow();
        let (renderer, frame) = viewport.drawn();
        let size = self.bitmap_size(renderer.media(), magnification);
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
                if let Some(frame) = raster.frames.get(frame) {
                    target.write(&frame.pixels);
                }
            }
            _ => {
                let mut canvas = OffscreenCanvas::new(size.0, size.1);
                canvas.draw(|context| {
                    renderer.draw(context, frame, (0.0, 0.0, size.0 as f32, size.1 as f32));
                });
                target.write_rgba(canvas.pixels());
            }
        }
        // A preview shows at the size of the media.
        if fresh && let Some(island) = self.island.borrow().as_ref() {
            island.set_media(target, viewport.media().size());
        }
        drop(bitmap);
        drop(viewport);
        self.follow();
        self.draw_pixels();
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
        bitmap.write_rgba(canvas.pixels());
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
        if !self.loaded.get() {
            return;
        }
        let (changed, next) = self.viewport.borrow_mut().advance_frame();
        if changed {
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
        if !self.loaded.get() {
            island.set_title(&window_text(self.window.get()), "Loading...");
            return;
        }
        let (width, height) = self.media_size();
        let title = window_text(self.window.get());
        // The title can still have the size of other media, like while the media changes or a
        // tool shows the picture before its last change.
        let name = title
            .strip_suffix(&window_title("", (width, height)))
            .or_else(|| {
                let (name, size) = title.rsplit_once(" (")?;
                let (width, height) = size.strip_suffix(')')?.split_once('x')?;
                let number =
                    |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
                (number(width) && number(height)).then_some(name)
            })
            .unwrap_or(&title);
        let subtitle = format!("{} × {}", width.round() as i64, height.round() as i64);
        island.set_title(name, &subtitle);
    }

    // The system fills the pixels a resize uncovers with the class brush of the window, which
    // is white for windows of bwindow, before the window paints them.
    fn update_background(&self) {
        let brush = background_brush(self.window_backdrop.get(), self.dark.get());
        // SAFETY: The window is alive, all windows of its class follow the same theme.
        unsafe {
            SetClassLongPtrW(self.window.get(), GCLP_HBRBACKGROUND, brush as isize);
            InvalidateRect(self.window.get(), null(), 1);
        }
    }

    // Lets the window show, also when it held back showing before.
    fn reveal(&self) {
        if self.revealed.replace(true) {
            return;
        }
        if let Some(island) = self.island.borrow().as_ref() {
            island.stop_rendered();
        }
        // SAFETY: The windows are alive.
        unsafe {
            KillTimer(self.content.get(), REVEAL_TIMER);
            if self.reveal_held.get() {
                PostMessageW(self.window.get(), WM_REVEAL, 0, 0);
            }
        }
    }

    // Fills the content with the island and draws the checkerboard for its size, and vector
    // media for a changed scale.
    fn layout(&self, width: i32, height: i32) {
        if let Some(island) = self.island.borrow().as_ref() {
            island.move_and_resize(width, height, self.scale());
        }
        self.update_checkerboard();
        self.zoomed();
        self.draw_marks();
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
            // The backdrop shows where the island has not drawn yet, like while it catches up
            // with a resize.
            WM_ERASEBKGND => return erase_background(state, hwnd, wparam),
            WM_RENDERED => {
                state.reveal();
                return 0;
            }
            WM_TIMER if wparam == REVEAL_TIMER => {
                state.reveal();
                return 0;
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
    {
        let island = state
            .island
            .try_borrow_mut()
            .ok()
            .and_then(|mut island| island.take());
        drop(island);
    }
    if let Some(state) = state {
        match message {
            // The window shows once the island has rendered its first frame, instead of the
            // empty window behind it.
            WM_REVEAL if !state.revealed.get() => {
                state.reveal_held.set(true);
                return 0;
            }
            WM_ERASEBKGND => return erase_background(state, hwnd, wparam),
            _ => {}
        }
    }
    // SAFETY: Messages pass through to the window first.
    let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
    // The header bar shows the title, which changes with unsaved changes.
    if message == WM_SETTEXT
        && let Some(state) = state
    {
        state.update_heading();
    }
    if message == WM_ACTIVATE
        && let Some(island) = state.and_then(|state| state.island.try_borrow().ok())
        && let Some(island) = island.as_ref()
    {
        island.set_active(wparam & 0xffff != WA_INACTIVE);
    }
    result
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use bwindow::{EventLoopBuilder, WindowBuilder};
    use imageview_core::Pages;
    use imageview_core::edit::Picture;

    use super::*;

    #[repr(C)]
    struct Message {
        window: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        x: i32,
        y: i32,
        private: u32,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn PeekMessageW(
            message: *mut Message,
            window: HWND,
            min: u32,
            max: u32,
            flags: u32,
        ) -> BOOL;
        fn TranslateMessage(message: *const Message) -> BOOL;
        fn DispatchMessageW(message: *const Message) -> isize;
    }

    fn layout_frames() {
        let until = Instant::now() + Duration::from_millis(200);
        while Instant::now() < until {
            // SAFETY: MSG is initialized and passed to the thread's native message pump.
            unsafe {
                let mut message: Message = std::mem::zeroed();
                while PeekMessageW(&mut message, null_mut(), 0, 0, 1) != 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    #[ignore = "requires an installed Windows App Runtime and a desktop"]
    fn native_windows_layout_and_alerts() {
        let event_loop = EventLoopBuilder::new()
            .with_user_event::<AppEvent>()
            .single_instance(false)
            .build();
        init(event_loop.create_proxy());
        let media = std::env::var_os("IMAGEVIEW_TEST_IMAGE").map_or_else(
            || Picture::new(1024, 1024, vec![255; 1024 * 1024 * 4]).to_media(),
            |path| {
                imageview_core::decode_media(&std::fs::read(path).expect("test image"))
                    .expect("decode test image")
            },
        );
        let samples = [
            media,
            Picture::new(1023, 1023, vec![255; 1023 * 1023 * 4]).to_media(),
            Picture::new(1001, 753, vec![255; 1001 * 753 * 4]).to_media(),
        ];
        for media in samples {
            for deferred in [false, true] {
                let size = crate::content_size((!deferred).then_some(&media));
                let mut window = WindowBuilder::new()
                    .title("Initial image fit test")
                    .size(size)
                    .build();
                let mut view =
                    MediaView::new(&window, (!deferred).then(|| media.clone()), Theme::Light);
                window.set_size(size);
                view.zoom_to_fit();
                layout_frames();
                if deferred {
                    window.set_size(crate::content_size(Some(&media)));
                    view.set_media(media.clone(), true);
                    layout_frames();
                }
                let visible = view.state.visible();
                let zoom = view.state.zoom_factor();
                let fitted = (media.size().0 * zoom, media.size().1 * zoom);
                let scrollable = view
                    .state
                    .island
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .scrollable_size();
                drop(view);
                drop(window);
                assert_eq!(
                    scrollable,
                    (0.0, 0.0),
                    "deferred={deferred}, visible={visible:?}, fitted={fitted:?}"
                );
            }
        }
        // Exercise ContentDialog on a real island, including its Closed callback and theme.
        let mut app = crate::App {
            proxy: std::sync::Arc::new(event_loop.create_proxy()),
            windows: Vec::new(),
            active: None,
            pending_loads: 0,
            opening: std::collections::HashMap::new(),
            next_open: 0,
            theme: Theme::Dark,
        };
        let media = Picture::new(2, 1, vec![255; 8]).to_media();
        app.open_window(
            "alert-test.png".into(),
            None,
            Some(Pages::single(media)),
            None,
        );
        let id = app.windows[0].window.id();
        layout_frames();
        // Resize a live island in both modes, including restoring overflowed controls.
        for width in [1600.0, MIN_WIDTH as f32, 900.0, MIN_WIDTH as f32, 1600.0] {
            app.windows[0]
                .window
                .set_size(bwindow::LogicalSize::new(width, 480.0));
            layout_frames();
            for editing in [false, true] {
                app.windows[0]
                    .view
                    .state
                    .island
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .set_editing(editing);
                layout_frames();
                app.windows[0]
                    .view
                    .state
                    .island
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .assert_header_layout(
                        (editing && width != 900.0).then_some(width == MIN_WIDTH as f32),
                    );
            }
        }
        app.windows[0]
            .view
            .state
            .island
            .borrow()
            .as_ref()
            .unwrap()
            .set_editing(false);
        // The display-sized layer is used for enlarged rasters and released when zoomed out.
        assert!(app.windows[0].view.state.pixels.borrow().is_some());
        app.windows[0].view.actual_size();
        layout_frames();
        if app.windows[0].view.state.scale() < 2.0 {
            assert!(app.windows[0].view.state.pixels.borrow().is_none());
            assert!(app.windows[0].view.state.pixel_canvas.borrow().is_none());
        }
        app.windows[0].view.zoom_by(4.0);
        layout_frames();
        assert!(app.windows[0].view.state.pixels.borrow().is_some());
        assert!(app.windows[0].view.state.pixel_canvas.borrow().is_some());
        let answer = Rc::new(Cell::new(-1i32));
        let output = answer.clone();
        let dialog = app.windows[0]
            .view
            .state
            .island
            .borrow()
            .as_ref()
            .unwrap()
            .show_alert(
                "Could not save <photo>",
                "An error with & and \"quotes\".",
                false,
                move |result| output.set(result),
            )
            .expect("window-attached error dialog");
        layout_frames();
        assert_eq!(answer.get(), -1);
        app.open_window(
            "other-alert-test.png".into(),
            None,
            Some(Pages::single(Picture::new(2, 1, vec![255; 8]).to_media())),
            None,
        );
        let other_id = app.windows.last().unwrap().window.id();
        app.edit_action("image.rotate_right");
        assert_eq!(
            app.windows.last().unwrap().view.media().size(),
            (1.0, 2.0),
            "another image window must remain usable"
        );
        assert!(!app.finish_editing(other_id, crate::edit::AfterSave::Close));
        layout_frames();
        assert!(
            app.windows.last().unwrap().view.dialog.is_some(),
            "each window must own its dialog"
        );
        app.windows
            .last()
            .unwrap()
            .view
            .dialog
            .as_ref()
            .unwrap()
            .hide();
        layout_frames();
        app.alert_closed(other_id, 2);
        app.active = Some(id);
        dialog.hide();
        for _ in 0..10 {
            layout_frames();
            if answer.get() != -1 {
                break;
            }
        }
        assert_eq!(
            answer.get(),
            0,
            "Close must complete the native dialog asynchronously"
        );
        drop(dialog);

        app.edit_action("image.rotate_right");
        assert!(!app.finish_editing(id, crate::edit::AfterSave::Close));
        layout_frames();
        assert!(app.windows[0].is_modal());
        assert!(
            app.windows[0].view.dialog.is_some(),
            "save prompt must be a ContentDialog"
        );
        let size = app.windows[0].view.media().size();
        app.action("image.rotate_right");
        assert_eq!(
            app.windows[0].view.media().size(),
            size,
            "modal dialog must block image shortcuts"
        );
        app.windows[0].view.dialog.as_ref().unwrap().hide();
        layout_frames();
        app.alert_closed(id, 0);
        assert!(
            app.windows[0].is_editing(),
            "Cancel must retain unsaved edits"
        );
        assert!(!app.windows[0].is_modal());

        assert!(!app.finish_editing(id, crate::edit::AfterSave::Close));
        layout_frames();
        app.windows[0].view.dialog.as_ref().unwrap().hide();
        layout_frames();
        app.alert_closed(id, 2);
        assert!(
            app.windows.is_empty(),
            "Don't Save must resume the requested close"
        );

        app.open_error("missing.png".into(), None, None, "File not found".into());
        layout_frames();
        assert_eq!(
            app.windows.len(),
            1,
            "a startup error needs its own XAML host"
        );
        assert!(app.windows[0].view.dialog.is_some());
        let error_id = app.windows[0].window.id();
        app.windows[0].view.dialog.as_ref().unwrap().hide();
        layout_frames();
        app.alert_closed(error_id, 0);
        assert!(
            app.windows.is_empty(),
            "closing a startup error must close its host"
        );

        // Save starts its worker without losing the requested continuation or blocking XAML.
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "imageview-alert-{}-{unique}.png",
            std::process::id()
        ));
        app.open_window(
            path.clone(),
            None,
            Some(Pages::single(Picture::new(2, 1, vec![255; 8]).to_media())),
            None,
        );
        let id = app.windows[0].window.id();
        app.edit_action("image.rotate_right");
        assert!(!app.finish_editing(id, crate::edit::AfterSave::Close));
        layout_frames();
        app.windows[0].view.dialog.as_ref().unwrap().hide();
        layout_frames();
        app.alert_closed(id, 1);
        assert!(app.windows[0].busy, "Save must start asynchronously");
        let until = Instant::now() + Duration::from_secs(3);
        while !path.exists() && Instant::now() < until {
            layout_frames();
        }
        let saved = std::fs::read(&path).expect("Save must write the edited image");
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            imageview_core::decode_media(&saved).unwrap().size(),
            (1.0, 2.0)
        );
        app.close_window(id);
        shutdown();
        drop(event_loop);
    }
}
