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
use crate::{ComObject, Interfaces, guard, is_dark, read_stream, unknown};

const CLASS_NAME: &str = "ImageViewPreview";
const ANIMATION_TIMER: usize = 1;
const E_NOTIMPL: HRESULT = 0x8000_4001_u32 as i32;

struct Preview {
    parent: Cell<HWND>,
    rect: Cell<(i32, i32, i32, i32)>,
    bytes: RefCell<Option<Vec<u8>>>,
    site: Cell<*mut c_void>,
    view: RefCell<Option<Box<View>>>,
}

// The child window that shows the media.
struct View {
    hwnd: HWND,
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
        if let Some(view) = self.view.borrow_mut().take() {
            // SAFETY: The view owns its window, which forgets the view before it is freed.
            unsafe {
                SetWindowLongPtrW(view.hwnd, GWLP_USERDATA, 0);
                DestroyWindow(view.hwnd);
            }
        }
    }

    fn place(&self) {
        if let Some(view) = &*self.view.borrow() {
            let (left, top, right, bottom) = self.rect.get();
            // SAFETY: The view window is alive.
            per_monitor_dpi(|| unsafe {
                SetWindowPos(
                    view.hwnd,
                    null_mut(),
                    left,
                    top,
                    right - left,
                    bottom - top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
                InvalidateRect(view.hwnd, null(), 0);
            });
        }
    }
}

// Explorer is per monitor DPI aware and passes rectangles in physical pixels, so the preview
// window is created and placed with that awareness instead of the one of the host process.
fn per_monitor_dpi<T>(f: impl FnOnce() -> T) -> T {
    // SAFETY: Only the DPI awareness of this thread changes, until it is restored below.
    let previous =
        unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    let result = f();
    // SAFETY: The previous context came from the call above.
    unsafe { SetThreadDpiAwarenessContext(previous) };
    result
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

const fn rect_of(rect: &RECT) -> (i32, i32, i32, i32) {
    (rect.left, rect.top, rect.right, rect.bottom)
}

unsafe extern "system" fn set_window(
    this: *mut c_void,
    parent: HWND,
    rect: *const RECT,
) -> HRESULT {
    if rect.is_null() {
        return E_POINTER;
    }
    guard(|| {
        // SAFETY: COM calls this with the first interface and a valid rectangle.
        let preview = unsafe { &Object::from_interface(this, 0).value };
        // SAFETY: The rectangle was checked to be non-null.
        preview.rect.set(rect_of(unsafe { &*rect }));
        // A new parent needs a new window, which the next preview creates.
        if preview.parent.replace(parent) != parent {
            preview.close();
        }
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
        preview.rect.set(rect_of(unsafe { &*rect }));
        preview.place();
        S_OK
    })
}

unsafe extern "system" fn do_preview(this: *mut c_void) -> HRESULT {
    guard(|| {
        // SAFETY: COM calls this with the first interface.
        let preview = unsafe { &Object::from_interface(this, 0).value };
        let Some(media) = preview
            .bytes
            .borrow()
            .as_deref()
            .and_then(|bytes| decode_media(bytes).ok())
        else {
            return E_FAIL;
        };
        preview.close();
        register_class();
        let class = wide(CLASS_NAME);
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
            hwnd,
            // SAFETY: The window lives as long as the view, which destroys it.
            canvas: RefCell::new(unsafe { HwndCanvas::new(hwnd) }),
            renderer: MediaRenderer::new(media),
            dark: is_dark(),
            start: Instant::now(),
            frame: Cell::new(0),
        });
        // SAFETY: The boxed view outlives its window, which forgets it before it is freed.
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, &*view as *const View as isize) };
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
    if let Some(view) = &*preview.view.borrow() {
        // SAFETY: The view window is alive.
        unsafe { SetFocus(view.hwnd) };
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
    guard(|| {
        // SAFETY: COM calls this with the second interface and a live stream.
        let (preview, bytes) =
            unsafe { (&Object::from_interface(this, 1).value, read_stream(stream)) };
        let Some(bytes) = bytes else {
            return E_FAIL;
        };
        *preview.bytes.borrow_mut() = Some(bytes);
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
        .map_or(preview.parent.get(), |view| view.hwnd);
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
        let (frame, next) = self.renderer.media().frame_at(self.start.elapsed());
        if self.frame.replace(frame) != frame {
            // SAFETY: The window is alive while the view is.
            unsafe { InvalidateRect(self.hwnd, null(), 0) };
        }
        if let Some(next) = next {
            // SAFETY: The window is alive and owns its timer.
            unsafe {
                SetTimer(
                    self.hwnd,
                    ANIMATION_TIMER,
                    next.as_millis().max(1) as u32,
                    null(),
                )
            };
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
        let class = wide(CLASS_NAME);
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
            // SAFETY: The window is going away, so it forgets the view.
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
        }
        // SAFETY: Other messages get the default handling.
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    0
}
