/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The cdecl ABI from the wke.h shipped with MiniBlink 2023.11.15.

#![allow(non_snake_case)]

use std::ffi::{CStr, c_char, c_void};
use std::sync::OnceLock;

use super::super::headers::{GetLastError, GetProcAddress, HMODULE, HWND, LoadLibraryW, wide};

pub(super) type View = *mut c_void;
pub(super) type Frame = *mut c_void;
pub(super) type JsState = *mut c_void;
pub(super) type JsValue = i64;

macro_rules! api {
    ($($(#[$meta:meta])* $name:ident($($arg:ty),* $(,)?) $(-> $result:ty)?;)*) => {
        pub(super) struct Api {
            $($(#[$meta])* pub(super) $name: unsafe extern "C" fn($($arg),*) $(-> $result)?,)*
        }

        static API: OnceLock<Api> = OnceLock::new();

        impl Api {
            pub(super) fn get() -> &'static Self {
                API.get().expect("MiniBlink not loaded")
            }

            pub(super) fn load() -> &'static Self {
                API.get_or_init(|| {
                    let module = load_module();
                    Self {
                        $($(#[$meta])* $name: {
                            let name = concat!(stringify!($name), "\0");
                            let symbol = unsafe { GetProcAddress(module, name.as_ptr().cast()) };
                            assert!(!symbol.is_null(), "MiniBlink is missing {}", stringify!($name));
                            unsafe { std::mem::transmute::<*const c_void, unsafe extern "C" fn($($arg),*) $(-> $result)?>(symbol) }
                        },)*
                    }
                })
            }
        }
    };
}

fn load_module() -> HMODULE {
    // The full path keeps the DLL search order from picking up another miniblink.dll
    let dll = std::env::current_exe()
        .expect("cannot find the executable")
        .with_file_name("miniblink.dll");
    // Keep the module loaded for the process lifetime. Its threads and global state
    // can outlive an individual browser window.
    let module = unsafe { LoadLibraryW(wide(&dll.to_string_lossy()).as_ptr()) };
    assert!(
        !module.is_null(),
        "cannot load {}: Win32 error {}",
        dll.display(),
        unsafe { GetLastError() }
    );
    module
}

api! {
    wkeInitialize();
    wkeCreateWebWindow(i32, HWND, i32, i32, i32, i32) -> View;
    wkeDestroyWebWindow(View);
    wkeGetWindowHandle(View) -> HWND;
    wkeSetDragEnable(View, bool);
    wkeSetUserAgent(View, *const c_char);
    wkeGetUserAgent(View) -> *const c_char;
    wkeSetZoomFactor(View, f32);
    wkeSetUserKeyValue(View, *const c_char, *mut c_void);
    wkeGetUserKeyValue(View, *const c_char) -> *mut c_void;
    wkeGetURL(View) -> *const c_char;
    wkeGetString(*mut c_void) -> *const c_char;
    wkeIsMainFrame(View, Frame) -> i32;
    wkeLoadURL(View, *const c_char);
    wkeLoadHtmlWithBaseUrl(View, *const c_char, *const c_char);
    wkeRunJS(View, *const c_char) -> JsValue;
    wkeRunJsByFrame(View, Frame, *const c_char, bool) -> JsValue;
    wkeOnURLChanged2(View, Option<unsafe extern "C" fn(View, *mut c_void, Frame, *mut c_void)>, *mut c_void);
    wkeOnTitleChanged(View, Option<unsafe extern "C" fn(View, *mut c_void, *mut c_void)>, *mut c_void);
    wkeOnDocumentReady2(View, Option<unsafe extern "C" fn(View, *mut c_void, Frame)>, *mut c_void);
    wkeOnLoadingFinish(View, Option<unsafe extern "C" fn(View, *mut c_void, *mut c_void, i32, *mut c_void)>, *mut c_void);
    wkeOnDidCreateScriptContext(View, Option<unsafe extern "C" fn(View, *mut c_void, Frame, *mut c_void, i32, i32)>, *mut c_void);
    wkeOnCreateView(View, Option<unsafe extern "C" fn(View, *mut c_void, i32, *mut c_void, *const c_void) -> View>, *mut c_void);
    wkeJsBindFunction(*const c_char, unsafe extern "C" fn(JsState, *mut c_void) -> JsValue, *mut c_void, u32);
    jsGetWebView(JsState) -> View;
    jsArgCount(JsState) -> i32;
    jsArg(JsState, i32) -> JsValue;
    jsEval(JsState, *const c_char) -> JsValue;
    jsToTempString(JsState, JsValue) -> *const c_char;
    jsUndefined() -> JsValue;
    #[cfg(feature = "custom_protocol")]
    wkeOnLoadUrlBegin(View, Option<unsafe extern "C" fn(View, *mut c_void, *const c_char, *mut c_void) -> bool>, *mut c_void);
    #[cfg(feature = "custom_protocol")]
    wkeNetSetMIMEType(*mut c_void, *const c_char);
    #[cfg(feature = "custom_protocol")]
    wkeNetSetHTTPHeaderField(*mut c_void, *const u16, *const u16, bool);
    #[cfg(feature = "custom_protocol")]
    wkeNetSetData(*mut c_void, *mut c_void, i32);
    #[cfg(feature = "custom_protocol")]
    wkeNetGetRequestMethod(*mut c_void) -> i32;
    #[cfg(feature = "custom_protocol")]
    wkeNetGetRawHttpHead(*mut c_void) -> *const Header;
    #[cfg(feature = "custom_protocol")]
    wkeNetGetPostBody(*mut c_void) -> *mut PostBody;
    #[cfg(feature = "custom_protocol")]
    wkeNetFreePostBodyElements(*mut PostBody);
    #[cfg(feature = "custom_protocol")]
    wkeNetCancelRequest(*mut c_void);
}

#[cfg(feature = "custom_protocol")]
#[repr(C)]
pub(super) struct Header {
    pub(super) data: *const c_char,
    pub(super) next: *const Header,
}

pub(super) unsafe fn string(value: *const c_char) -> String {
    if value.is_null() {
        String::new()
    } else {
        unsafe { CStr::from_ptr(value) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(feature = "custom_protocol")]
#[repr(C)]
pub(super) struct MemBuf {
    pub(super) unused: i32,
    pub(super) data: *const u8,
    pub(super) length: usize,
}

#[cfg(feature = "custom_protocol")]
#[repr(C)]
pub(super) struct BodyElement {
    pub(super) size: i32,
    pub(super) kind: i32,
    pub(super) data: *const MemBuf,
    pub(super) file_path: *mut c_void,
    pub(super) file_start: i64,
    pub(super) file_length: i64,
}

#[cfg(feature = "custom_protocol")]
#[repr(C)]
pub(super) struct PostBody {
    pub(super) size: i32,
    pub(super) elements: *const *const BodyElement,
    pub(super) length: usize,
    pub(super) dirty: bool,
}
