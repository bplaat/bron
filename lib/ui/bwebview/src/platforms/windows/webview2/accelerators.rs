/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::rc::Rc;

use bwindow::ffi::windows_translate_accelerator;

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
struct AcceleratorKeyPressedHandlerVtbl {
    base: IUnknownVtbl,
    invoke: unsafe extern "system" fn(
        *mut c_void,
        *mut ICoreWebView2Controller,
        *mut AcceleratorKeyPressedArgs,
    ) -> HRESULT,
}

#[repr(C)]
struct AcceleratorKeyPressedArgs {
    vtable: *const AcceleratorKeyPressedArgsVtbl,
}

#[repr(C)]
struct AcceleratorKeyPressedArgsVtbl {
    base: IUnknownVtbl,
    kind: unsafe extern "system" fn(*mut AcceleratorKeyPressedArgs, *mut i32) -> HRESULT,
    key: unsafe extern "system" fn(*mut AcceleratorKeyPressedArgs, *mut u32) -> HRESULT,
    lparam: unsafe extern "system" fn(*mut AcceleratorKeyPressedArgs, *mut i32) -> HRESULT,
    _unused_6_7: [*const c_void; 2],
    handled: unsafe extern "system" fn(*mut AcceleratorKeyPressedArgs, BOOL) -> HRESULT,
}

pub(super) unsafe fn register(
    controller: *mut ICoreWebView2Controller,
    owner: Rc<WebviewData>,
) -> HRESULT {
    static VTABLE: AcceleratorKeyPressedHandlerVtbl = AcceleratorKeyPressedHandlerVtbl {
        base: callback::IUNKNOWN,
        invoke,
    };
    let handler = unsafe { CallbackHandle::<c_void>::new(&VTABLE, IID_HANDLER, owner) };
    let mut token = EventRegistrationToken { value: 0 };
    unsafe {
        ((*(*controller).vtbl).add_AcceleratorKeyPressed)(controller, handler.as_ptr(), &mut token)
    }
}

unsafe extern "system" fn invoke(
    this: *mut c_void,
    _: *mut ICoreWebView2Controller,
    args: *mut AcceleratorKeyPressedArgs,
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
