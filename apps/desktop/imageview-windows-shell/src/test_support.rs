/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Owned native COM interfaces and streams for shell integration tests.

use std::ffi::c_void;
use std::ptr::null_mut;

use crate::headers::*;

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, mode: u32) -> HRESULT;
    fn CoUninitialize();
}

#[link(name = "shlwapi")]
unsafe extern "system" {
    fn SHCreateMemStream(bytes: *const u8, size: u32) -> *mut c_void;
}

pub(crate) struct Apartment;

impl Apartment {
    pub(crate) fn new() -> Self {
        // SAFETY: Each test owns its STA and drops all interfaces before this guard.
        assert!(unsafe { CoInitializeEx(null_mut(), 2) } >= 0);
        Self
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: Matches the successful initialization on this test thread.
        unsafe { CoUninitialize() };
    }
}

pub(crate) struct Interface(pub(crate) *mut c_void);

impl Interface {
    pub(crate) fn create(clsid: &GUID, iid: &GUID) -> Self {
        let mut factory = null_mut();
        // SAFETY: GUIDs and output buffers are valid, and both returned references are owned.
        unsafe {
            assert_eq!(
                crate::DllGetClassObject(
                    std::ptr::from_ref(clsid).cast(),
                    std::ptr::from_ref(&IID_ICLASS_FACTORY).cast(),
                    &mut factory
                ),
                S_OK
            );
            let factory = Self(factory);
            let vtable = &**factory.0.cast::<*const crate::IClassFactoryVtbl>();
            let mut object = null_mut();
            assert_eq!(
                (vtable.CreateInstance)(factory.0, null_mut(), iid, &mut object),
                S_OK
            );
            Self(object)
        }
    }

    pub(crate) fn query(&self, iid: &GUID) -> Self {
        let mut object = null_mut();
        // SAFETY: This interface is live and QueryInterface returns an owned reference.
        unsafe {
            let vtable = &**self.0.cast::<*const IUnknownVtbl>();
            assert_eq!((vtable.QueryInterface)(self.0, iid, &mut object), S_OK);
        }
        Self(object)
    }
}

impl Drop for Interface {
    fn drop(&mut self) {
        // SAFETY: This wrapper owns exactly one reference.
        unsafe { ((**self.0.cast::<*const IUnknownVtbl>()).Release)(self.0) };
    }
}

pub(crate) struct MemoryStream(pub(crate) Interface);

impl MemoryStream {
    pub(crate) fn new(bytes: &[u8]) -> Self {
        // SAFETY: The API copies the bytes and transfers a reference to the stream.
        let stream = unsafe { SHCreateMemStream(bytes.as_ptr(), bytes.len() as u32) };
        assert!(!stream.is_null());
        Self(Interface(stream))
    }

    pub(crate) fn references(&self) -> u32 {
        // SAFETY: A balanced temporary reference measures native ownership without changing it.
        unsafe {
            let vtable = &**self.0.0.cast::<*const IUnknownVtbl>();
            (vtable.AddRef)(self.0.0);
            (vtable.Release)(self.0.0)
        }
    }
}
