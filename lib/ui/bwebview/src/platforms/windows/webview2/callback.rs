/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::any::Any;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, Ordering, fence};

use super::super::com::ComPtr;
use super::headers::*;
use super::webview::WebviewData;

// All WebView2 handler interfaces have the same COM prefix. WebView2 invokes
// these callbacks on the creating STA; Rust state stays on that thread.
#[repr(C)]
struct Callback {
    vtable: *const c_void,
    refs: AtomicU32,
    iid: GUID,
    owner: Rc<dyn Any>,
}

pub(super) struct CallbackHandle<T>(ComPtr<T>);

impl<T> CallbackHandle<T> {
    /// The vtable must implement T and start with the callback IUnknown methods.
    pub(super) unsafe fn new<V, O: Any>(vtable: &'static V, iid: GUID, owner: Rc<O>) -> Self {
        let pointer = Box::into_raw(Box::new(Callback {
            vtable: (vtable as *const V).cast(),
            refs: AtomicU32::new(1),
            iid,
            owner,
        }));
        Self(unsafe { ComPtr::from_raw(pointer.cast()) }.expect("callback allocation is non-null"))
    }

    pub(super) const fn as_ptr(&self) -> *mut T {
        self.0.as_ptr()
    }
}

pub(super) unsafe fn state(this: *mut c_void) -> Rc<WebviewData> {
    unsafe { &*this.cast::<Callback>() }
        .owner
        .clone()
        .downcast::<WebviewData>()
        .expect("wrong callback owner")
}

unsafe extern "system" fn query_interface(
    this: *mut c_void,
    iid: *const GUID,
    object: *mut *mut c_void,
) -> HRESULT {
    if object.is_null() || iid.is_null() {
        return E_POINTER;
    }
    unsafe { *object = std::ptr::null_mut() };
    let callback = unsafe { &*this.cast::<Callback>() };
    if unsafe { *iid == IID_IUNKNOWN || *iid == callback.iid } {
        unsafe {
            *object = this;
            add_ref(this);
        }
        S_OK
    } else {
        E_NOINTERFACE
    }
}

unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    let callback = unsafe { &*this.cast::<Callback>() };
    callback.refs.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    let callback = unsafe { &*this.cast::<Callback>() };
    let remaining = callback.refs.fetch_sub(1, Ordering::Release) - 1;
    if remaining == 0 {
        fence(Ordering::Acquire);
        unsafe { drop(Box::from_raw(this.cast::<Callback>())) };
    }
    remaining
}

pub(super) const IUNKNOWN: IUnknownVtbl = IUnknownVtbl {
    QueryInterface: query_interface,
    AddRef: add_ref,
    Release: release,
};

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_callback_owns_state_until_final_com_release() {
        let owner = Rc::new(42u32);
        let weak = Rc::downgrade(&owner);
        static VTABLE: IUnknownVtbl = IUNKNOWN;
        let callback: CallbackHandle<c_void> = unsafe {
            CallbackHandle::new(
                &VTABLE,
                IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
                owner,
            )
        };
        let mut object = std::ptr::null_mut();
        unsafe {
            assert_eq!(
                query_interface(
                    callback.as_ptr(),
                    &IID_ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
                    &mut object
                ),
                S_OK
            );
        }
        drop(callback);
        assert!(weak.upgrade().is_some());
        unsafe {
            assert_eq!(release(object), 0);
        }
        assert!(weak.upgrade().is_none());
    }
}
