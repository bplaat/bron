/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The preview handler, which shows the media fitted in the preview pane above a checkerboard
//! and plays its animation.

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use std::sync::Once;
use std::time::Instant;

use bcanvas::HwndCanvas;
use imageview_core::{MediaRenderer, decode_media, draw_checkerboard};

use crate::headers::*;
use crate::{ComObject, Interfaces, Stream, guard, is_dark, read_stream, unknown};

const CLASS_NAME: &[u16] = w!("ImageViewPreview");
const ANIMATION_TIMER: usize = 1;
const E_NOTIMPL: HRESULT = 0x8000_4001_u32 as i32;

struct Preview {
    parent: Cell<HWND>,
    rect: Cell<(i32, i32, i32, i32)>,
    bytes: RefCell<Option<(Stream, Vec<u8>)>>,
    site: Cell<*mut c_void>,
    view: RefCell<Option<Box<View>>>,
}

// The child window that shows the media.
struct View {
    hwnd: Cell<HWND>,
    canvas: RefCell<HwndCanvas>,
    renderer: MediaRenderer,
    dark: bool,
    start: Instant,
    frame: Cell<usize>,
}

impl Interfaces<4> for Preview {
    const IIDS: [GUID; 4] = [
        IID_IPREVIEW_HANDLER,
        IID_IINITIALIZE_WITH_STREAM,
        IID_IOBJECT_WITH_SITE,
        IID_IOLE_WINDOW,
    ];
}

type Object = ComObject<Preview, 4>;

#[repr(C)]
struct IPreviewHandlerVtbl {
    base: IUnknownVtbl,
    SetWindow: unsafe extern "system" fn(*mut c_void, HWND, *const RECT) -> HRESULT,
    SetRect: unsafe extern "system" fn(*mut c_void, *const RECT) -> HRESULT,
    DoPreview: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    Unload: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    SetFocus: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    QueryFocus: unsafe extern "system" fn(*mut c_void, *mut HWND) -> HRESULT,
    TranslateAccelerator: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IInitializeWithStreamVtbl {
    base: IUnknownVtbl,
    Initialize: unsafe extern "system" fn(*mut c_void, *mut c_void, u32) -> HRESULT,
}

#[repr(C)]
struct IObjectWithSiteVtbl {
    base: IUnknownVtbl,
    SetSite: unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT,
    GetSite: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
}

#[repr(C)]
struct IOleWindowVtbl {
    base: IUnknownVtbl,
    GetWindow: unsafe extern "system" fn(*mut c_void, *mut HWND) -> HRESULT,
    ContextSensitiveHelp: unsafe extern "system" fn(*mut c_void, BOOL) -> HRESULT,
}

static PREVIEW_VTABLE: IPreviewHandlerVtbl = IPreviewHandlerVtbl {
    base: unknown::<Preview, 4, 0>(),
    SetWindow: set_window,
    SetRect: set_rect,
    DoPreview: do_preview,
    Unload: unload,
    SetFocus: set_focus,
    QueryFocus: query_focus,
    TranslateAccelerator: translate_accelerator,
};

static INITIALIZE_VTABLE: IInitializeWithStreamVtbl = IInitializeWithStreamVtbl {
    base: unknown::<Preview, 4, 1>(),
    Initialize: initialize,
};

static SITE_VTABLE: IObjectWithSiteVtbl = IObjectWithSiteVtbl {
    base: unknown::<Preview, 4, 2>(),
    SetSite: set_site,
    GetSite: get_site,
};

static WINDOW_VTABLE: IOleWindowVtbl = IOleWindowVtbl {
    base: unknown::<Preview, 4, 3>(),
    GetWindow: get_window,
    ContextSensitiveHelp: context_sensitive_help,
};

/// Creates a preview handler with one reference.
pub(crate) fn create() -> *mut c_void {
    Object::create(
        [
            (&raw const PREVIEW_VTABLE).cast(),
            (&raw const INITIALIZE_VTABLE).cast(),
            (&raw const SITE_VTABLE).cast(),
            (&raw const WINDOW_VTABLE).cast(),
        ],
        Preview {
            parent: Cell::new(null_mut()),
            rect: Cell::new((0, 0, 0, 0)),
            bytes: RefCell::new(None),
            site: Cell::new(null_mut()),
            view: RefCell::new(None),
        },
    )
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.close();
        set_site_object(&self.site, null_mut());
    }
}

impl Preview {
    fn close(&self) {
        let view = self.view.borrow_mut().take();
        if let Some(view) = view {
            // SAFETY: The view owns its window, which forgets the view before it is freed.
            unsafe {
                let hwnd = view.hwnd.get();
                if !hwnd.is_null() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    DestroyWindow(hwnd);
                }
            }
        }
    }

    fn place(&self) {
        if let Some(view) = &*self.view.borrow() {
            if view.hwnd.get().is_null() {
                return;
            }
            let (left, top, right, bottom) = self.rect.get();
            // SAFETY: The view window is alive.
            per_monitor_dpi(|| unsafe {
                SetWindowPos(
                    view.hwnd.get(),
                    null_mut(),
                    left,
                    top,
                    right - left,
                    bottom - top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                InvalidateRect(view.hwnd.get(), null(), 0);
            });
        }
    }
}

// Explorer is per monitor DPI aware and passes rectangles in physical pixels, so the preview
// window is created and placed with that awareness instead of the one of the host process.
fn per_monitor_dpi<T>(f: impl FnOnce() -> T) -> T {
    struct RestoreDpi(isize);
    impl Drop for RestoreDpi {
        fn drop(&mut self) {
            if self.0 != 0 {
                // SAFETY: This context was returned by SetThreadDpiAwarenessContext.
                unsafe { SetThreadDpiAwarenessContext(self.0) };
            }
        }
    }
    // SAFETY: Only the DPI awareness of this thread changes, until it is restored below.
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let _restore = RestoreDpi(previous);
    f()
}

// Replaces the retained site object.
fn set_site_object(site: &Cell<*mut c_void>, object: *mut c_void) {
    // SAFETY: Sites are live COM objects, the new one is retained before the old is released.
    unsafe {
        if !object.is_null() {
            ((**object.cast::<*const IUnknownVtbl>()).AddRef)(object);
        }
        let previous = site.replace(object);
        if !previous.is_null() {
            ((**previous.cast::<*const IUnknownVtbl>()).Release)(previous);
        }
    }
}

fn rect_of(rect: &RECT) -> Option<(i32, i32, i32, i32)> {
    let width = rect.right.checked_sub(rect.left)?;
    let height = rect.bottom.checked_sub(rect.top)?;
    (width >= 0 && height >= 0).then_some((rect.left, rect.top, rect.right, rect.bottom))
}

unsafe extern "system" fn set_window(
    this: *mut c_void,
    parent: HWND,
    rect: *const RECT,
) -> HRESULT {
    if rect.is_null() {
        return E_POINTER;
    }
    if parent.is_null() {
        return E_INVALIDARG;
    }
    guard(|| {
        // SAFETY: COM calls this with the first interface and a valid rectangle.
        let preview = unsafe { &Object::from_interface(this, 0).value };
        // SAFETY: The rectangle was checked to be non-null.
        let Some(rect) = rect_of(unsafe { &*rect }) else {
            return E_INVALIDARG;
        };
        if preview.parent.get() != parent {
            let hwnd = preview.view.borrow().as_ref().map(|view| view.hwnd.get());
            if let Some(hwnd) = hwnd.filter(|hwnd| !hwnd.is_null()) {
                // SAFETY: Both windows are live; reparent without discarding an active preview.
                if per_monitor_dpi(|| unsafe { SetParent(hwnd, parent) }).is_null() {
                    return E_FAIL;
                }
            }
        }
        preview.parent.set(parent);
        preview.rect.set(rect);
        preview.place();
        S_OK
    })
}

unsafe extern "system" fn set_rect(this: *mut c_void, rect: *const RECT) -> HRESULT {
    if rect.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM calls this with the first interface and a valid rectangle.
        let preview = unsafe { &Object::from_interface(this, 0).value };
        // SAFETY: The rectangle was checked to be non-null.
        let Some(rect) = rect_of(unsafe { &*rect }) else {
            return E_INVALIDARG;
        };
        preview.rect.set(rect);
        preview.place();
        S_OK
    })
}

unsafe extern "system" fn do_preview(this: *mut c_void) -> HRESULT {
    guard(|| {
        // SAFETY: COM calls this with the first interface.
        let preview = unsafe { &Object::from_interface(this, 0).value };
        if preview.parent.get().is_null() {
            return E_FAIL;
        }
        let Some(media) = preview
            .bytes
            .borrow()
            .as_ref()
            .map(|(_, bytes)| bytes.as_slice())
            .and_then(|bytes| decode_media(bytes).ok())
        else {
            return E_FAIL;
        };
        preview.close();
        register_class();
        let class = CLASS_NAME;
        let (left, top, right, bottom) = preview.rect.get();
        // SAFETY: The class is registered and the parent is the window of the preview pane.
        let hwnd = per_monitor_dpi(|| unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                left,
                top,
                right - left,
                bottom - top,
                preview.parent.get(),
                null_mut(),
                module(),
                null_mut(),
            )
        });
        if hwnd.is_null() {
            return E_FAIL;
        }
        let view = Box::new(View {
            hwnd: Cell::new(hwnd),
            // SAFETY: The window lives as long as the view, which destroys it.
            canvas: RefCell::new(unsafe { HwndCanvas::new(hwnd) }),
            renderer: MediaRenderer::new(media),
            dark: is_dark(),
            start: Instant::now(),
            frame: Cell::new(0),
        });
        // SAFETY: The boxed view outlives its window, which forgets it before it is freed.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*view as *const View as isize);
            InvalidateRect(hwnd, null(), 0);
        }
        view.advance();
        *preview.view.borrow_mut() = Some(view);
        S_OK
    })
}

unsafe extern "system" fn unload(this: *mut c_void) -> HRESULT {
    guard(|| {
        // SAFETY: COM calls this with the first interface.
        let preview = unsafe { &Object::from_interface(this, 0).value };
        preview.close();
        preview.bytes.borrow_mut().take();
        S_OK
    })
}

unsafe extern "system" fn set_focus(this: *mut c_void) -> HRESULT {
    // SAFETY: COM calls this with the first interface.
    let preview = unsafe { &Object::from_interface(this, 0).value };
    let hwnd = preview.view.borrow().as_ref().map(|view| view.hwnd.get());
    if let Some(hwnd) = hwnd.filter(|hwnd| !hwnd.is_null()) {
        // SAFETY: The view window is alive.
        unsafe { SetFocus(hwnd) };
    }
    S_OK
}

unsafe extern "system" fn query_focus(_: *mut c_void, hwnd: *mut HWND) -> HRESULT {
    if hwnd.is_null() {
        return E_POINTER;
    }
    // SAFETY: The out pointer was checked.
    unsafe { *hwnd = GetFocus() };
    S_OK
}

const unsafe extern "system" fn translate_accelerator(_: *mut c_void, _: *mut c_void) -> HRESULT {
    S_FALSE
}

unsafe extern "system" fn initialize(this: *mut c_void, stream: *mut c_void, _: u32) -> HRESULT {
    if stream.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM calls this with the second interface and a live stream.
        let preview = unsafe { &Object::from_interface(this, 1).value };
        if preview.bytes.borrow().is_some() {
            return ERROR_ALREADY_INITIALIZED;
        }
        // SAFETY: The caller supplies a live source stream.
        let bytes = unsafe { read_stream(stream) };
        let Some(bytes) = bytes else {
            return E_FAIL;
        };
        // SAFETY: Keep the source open until Unload or the handler is released.
        *preview.bytes.borrow_mut() = Some((unsafe { Stream::retain(stream) }, bytes));
        S_OK
    })
}

unsafe extern "system" fn set_site(this: *mut c_void, site: *mut c_void) -> HRESULT {
    // SAFETY: COM calls this with the third interface.
    let preview = unsafe { &Object::from_interface(this, 2).value };
    set_site_object(&preview.site, site);
    S_OK
}

unsafe extern "system" fn get_site(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM supplies a writable out pointer even when no site is set.
    unsafe { *out = null_mut() };
    if iid.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM calls this with the third interface.
    let preview = unsafe { &Object::from_interface(this, 2).value };
    let site = preview.site.get();
    if site.is_null() {
        return E_FAIL;
    }
    // SAFETY: The retained site is a live COM object.
    unsafe { ((**site.cast::<*const IUnknownVtbl>()).QueryInterface)(site, iid, out) }
}

unsafe extern "system" fn get_window(this: *mut c_void, hwnd: *mut HWND) -> HRESULT {
    if hwnd.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM calls this with the fourth interface.
    let preview = unsafe { &Object::from_interface(this, 3).value };
    let window = preview
        .view
        .borrow()
        .as_ref()
        .map_or(preview.parent.get(), |view| view.hwnd.get());
    // SAFETY: The out pointer was checked.
    unsafe { *hwnd = window };
    S_OK
}

const unsafe extern "system" fn context_sensitive_help(_: *mut c_void, _: BOOL) -> HRESULT {
    E_NOTIMPL
}

impl View {
    // Shows the current animation frame and schedules the next one.
    fn advance(&self) {
        if self.hwnd.get().is_null() {
            return;
        }
        let (frame, next) = self.renderer.media().frame_at(self.start.elapsed());
        if self.frame.replace(frame) != frame {
            // SAFETY: The window is alive while the view is.
            unsafe { InvalidateRect(self.hwnd.get(), null(), 0) };
        }
        if let Some(next) = next {
            // SAFETY: The window is alive and owns its timer.
            unsafe {
                SetTimer(
                    self.hwnd.get(),
                    ANIMATION_TIMER,
                    next.as_millis().max(1) as u32,
                    null(),
                )
            };
        } else {
            // SAFETY: A finished animation must stop its repeating Win32 timer.
            unsafe { KillTimer(self.hwnd.get(), ANIMATION_TIMER) };
        }
    }

    fn paint(&self) {
        self.canvas.borrow_mut().paint(|context| {
            let (width, height) = (context.width(), context.height());
            draw_checkerboard(context, width, height, self.dark);
            self.renderer
                .draw_fitted(context, self.frame.get(), (0.0, 0.0, width, height));
        });
    }
}

// The module of this DLL, whose window class the preview windows use.
fn module() -> *mut c_void {
    let mut module = null_mut();
    // SAFETY: The window procedure is code of this DLL, which stays loaded.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            window_proc as *const c_void,
            &mut module,
        )
    };
    module
}

fn register_class() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        let class = CLASS_NAME;
        // SAFETY: The class structure is valid and its name is copied by the system.
        unsafe {
            RegisterClassExW(&WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: window_proc,
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: module(),
                hIcon: null_mut(),
                hCursor: null_mut(),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class.as_ptr(),
                hIconSm: null_mut(),
            })
        };
    });
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // SAFETY: The user data is the view of the window, which is cleared before it is freed.
    let view = unsafe { (GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const View).as_ref() };
    match (message, view) {
        (WM_PAINT, Some(view)) => view.paint(),
        (WM_ERASEBKGND, _) => return 1,
        (WM_TIMER, Some(view)) if wparam == ANIMATION_TIMER => view.advance(),
        (WM_NCDESTROY, _) => {
            if let Some(view) = view {
                view.hwnd.set(null_mut());
            }
            // SAFETY: The window is going away, so it forgets the view.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        }
        // SAFETY: Other messages get the default handling.
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Apartment, Interface, MemoryStream};

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetParent(window: HWND) -> HWND;
        fn IsWindow(window: HWND) -> BOOL;
        fn SendMessageW(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    }

    // A hidden standard window keeps the test on real HWNDs without displaying any UI.
    struct Window(HWND);

    impl Window {
        fn new() -> Self {
            // SAFETY: STATIC is a built-in class; the root window is never made visible.
            let window = unsafe {
                CreateWindowExW(
                    0,
                    w!("STATIC").as_ptr(),
                    null(),
                    0,
                    0,
                    0,
                    100,
                    100,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null_mut(),
                )
            };
            assert!(!window.is_null());
            Self(window)
        }
    }

    impl Drop for Window {
        fn drop(&mut self) {
            // SAFETY: This guard owns the hidden window on its creating thread.
            unsafe { DestroyWindow(self.0) };
        }
    }

    #[test]
    fn live_preview_reparents_and_survives_host_destruction() {
        let _apartment = Apartment::new();
        let first = Window::new();
        let second = Window::new();
        let handler = Interface::create(&crate::CLSID_PREVIEW_HANDLER, &IID_IPREVIEW_HANDLER);
        let initializer = handler.query(&IID_IINITIALIZE_WITH_STREAM);
        let ole_window = handler.query(&IID_IOLE_WINDOW);
        let source = include_bytes!("../../imageview/tests/fixtures/rgb.qoi");
        let stream = MemoryStream::new(source);
        let rect = RECT {
            left: 0,
            top: 0,
            right: 80,
            bottom: 60,
        };
        // SAFETY: All interfaces and HWNDs are live; all rectangle and output buffers are valid.
        unsafe {
            assert_eq!(do_preview(handler.0), E_FAIL);
            assert_eq!(initialize(initializer.0, null_mut(), 0), E_POINTER);
            assert_eq!(initialize(initializer.0, stream.0.0, 0), S_OK);
            assert_eq!(stream.references(), 2);
            assert_eq!(
                initialize(initializer.0, stream.0.0, 0),
                ERROR_ALREADY_INITIALIZED
            );
            assert_eq!(set_window(handler.0, null_mut(), &rect), E_INVALIDARG);
            assert_eq!(set_window(handler.0, first.0, &rect), S_OK);
            assert_eq!(do_preview(handler.0), S_OK);
            let mut child = null_mut();
            assert_eq!(get_window(ole_window.0, &mut child), S_OK);
            assert_eq!(GetParent(child), first.0);
            SendMessageW(child, WM_PAINT, 0, 0);
            assert_eq!(set_window(handler.0, second.0, &rect), S_OK);
            let mut after = null_mut();
            assert_eq!(get_window(ole_window.0, &mut after), S_OK);
            assert_eq!(
                after, child,
                "SetWindow must preserve the existing preview window"
            );
            assert_eq!(GetParent(child), second.0);
            let invalid = RECT {
                left: i32::MIN,
                top: 0,
                right: i32::MAX,
                bottom: 1,
            };
            assert_eq!(set_rect(handler.0, &invalid), E_INVALIDARG);
            drop(second);
            let preview = &Object::from_interface(handler.0, 0).value;
            assert!(
                preview
                    .view
                    .borrow()
                    .as_ref()
                    .expect("view")
                    .hwnd
                    .get()
                    .is_null()
            );
            let replacement = Window::new();
            assert_eq!(unload(handler.0), S_OK);
            assert_ne!(
                IsWindow(replacement.0),
                0,
                "Unload must not destroy a reused HWND"
            );
            assert_eq!(stream.references(), 1);
            assert_eq!(unload(handler.0), S_OK);
            let next = MemoryStream::new(source);
            assert_eq!(initialize(initializer.0, next.0.0, 0), S_OK);
            assert_eq!(set_window(handler.0, first.0, &rect), S_OK);
            assert_eq!(do_preview(handler.0), S_OK);
            assert_eq!(unload(handler.0), S_OK);
            assert_eq!(next.references(), 1);
        }
    }

    #[test]
    fn missing_preview_site_clears_its_output() {
        let _apartment = Apartment::new();
        let handler = Interface::create(&crate::CLSID_PREVIEW_HANDLER, &IID_IOBJECT_WITH_SITE);
        let mut output = std::ptr::dangling_mut();
        // SAFETY: The handler and output are valid; no site has been assigned.
        unsafe {
            assert_eq!(get_site(handler.0, &IID_IUNKNOWN, &mut output), E_FAIL);
            assert!(output.is_null());
        }
    }
}
