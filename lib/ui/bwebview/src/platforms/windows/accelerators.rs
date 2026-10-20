/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::rc::Rc;

use super::callback::{self, CallbackHandle};
use super::headers::*;
use super::webview::WebviewData;

const IID_HANDLER: GUID = GUID {
    data1: 0xb29c7e28,
    data2: 0xfa79,
    data3: 0x41a8,
    data4: [0x8e, 0x44, 0x65, 0x81, 0x1c, 0x76, 0xdc, 0xb2],
};

#[repr(C)]
struct HandlerVtable {
    query: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    invoke:
        unsafe extern "system" fn(*mut c_void, *mut ICoreWebView2Controller, *mut Args) -> HRESULT,
}

#[repr(C)]
struct Args {
    vtable: *const ArgsVtable,
}

#[repr(C)]
struct ArgsVtable {
    unknown: [usize; 3],
    kind: unsafe extern "system" fn(*mut Args, *mut i32) -> HRESULT,
    key: unsafe extern "system" fn(*mut Args, *mut u32) -> HRESULT,
    lparam: unsafe extern "system" fn(*mut Args, *mut i32) -> HRESULT,
    unused: [usize; 2],
    handled: unsafe extern "system" fn(*mut Args, BOOL) -> HRESULT,
}

pub(super) unsafe fn register(controller: *mut ICoreWebView2Controller, owner: Rc<WebviewData>) {
    static VTABLE: HandlerVtable = HandlerVtable {
        query: callback::query_interface,
        add_ref: callback::add_ref,
        release: callback::release,
        invoke,
    };
    let handler = CallbackHandle::<c_void>::new(&VTABLE, IID_HANDLER, owner);
    let mut token = EventRegistrationToken { value: 0 };
    let result = unsafe {
        ((*(*controller).lpVtbl).add_AcceleratorKeyPressed)(
            controller,
            handler.as_ptr(),
            &mut token,
        )
    };
    assert_eq!(result, S_OK, "could not register WebView2 accelerators");
}

unsafe extern "system" fn invoke(
    this: *mut c_void,
    _: *mut ICoreWebView2Controller,
    args: *mut Args,
) -> HRESULT {
    let owner = unsafe { callback::state(this) };
    if owner.attachment.is_closed() {
        return S_OK;
    }
    unsafe {
        let vtable = &*(*args).vtable;
        let (mut kind, mut key, mut lparam) = (0, 0, 0);
        if (vtable.kind)(args, &mut kind) == S_OK
            && matches!(kind, 0 | 2)
            && (vtable.key)(args, &mut key) == S_OK
            && (vtable.lparam)(args, &mut lparam) == S_OK
            && windows_translate_accelerator(owner.hwnd, key, lparam as isize, kind == 2)
        {
            (vtable.handled)(args, TRUE);
        }
    }
    S_OK
}
