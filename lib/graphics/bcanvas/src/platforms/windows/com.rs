/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::{NonNull, null_mut};
use std::rc::Rc;

use crate::platforms::windows::headers::{GUID, IUnknownVtbl};

/// Associates an interface's ABI type with its IID.
///
/// # Safety
/// Implementors must represent the interface identified by IID with an IUnknown-compatible vtable.
pub(crate) unsafe trait ComInterface {
    const IID: GUID;
}

const E_POINTER: i32 = 0x80004003u32 as i32;

/// One owned COM reference. Canvas COM objects stay on the window thread.
pub(crate) struct ComPtr<T> {
    pointer: NonNull<T>,
    _thread: PhantomData<Rc<()>>,
}

impl<T> ComPtr<T> {
    /// The callback must follow COM out-parameter ownership: any non-null result
    /// is an owned reference to T with an IUnknown-compatible vtable prefix.
    pub(crate) unsafe fn create(create: impl FnOnce(*mut *mut T) -> i32) -> Result<Self, i32> {
        let mut pointer = null_mut();
        let result = create(&mut pointer);
        let owned = NonNull::new(pointer).map(|pointer| Self {
            pointer,
            _thread: PhantomData,
        });
        check(result)?;
        owned.ok_or(E_POINTER)
    }

    pub(crate) const fn as_ptr(&self) -> *mut T {
        self.pointer.as_ptr()
    }

    /// Share this reference as its base interface. Interface Deref impls only
    /// reinterpret the same pointer, because COM bases are vtable prefixes.
    pub(crate) fn upcast<U>(&self) -> ComPtr<U>
    where
        T: Deref<Target = U>,
    {
        let base = self.clone();
        let pointer = NonNull::from(&**base);
        std::mem::forget(base);
        ComPtr {
            pointer,
            _thread: PhantomData,
        }
    }

    pub(crate) fn query<U: ComInterface>(&self) -> Result<ComPtr<U>, i32> {
        let pointer = self.query_raw(&U::IID)?.cast();
        Ok(ComPtr {
            pointer,
            _thread: PhantomData,
        })
    }

    /// Checks for an interface and releases the reference obtained by QueryInterface.
    pub(crate) fn supports(&self, iid: &GUID) -> bool {
        let Ok(pointer) = self.query_raw(iid) else {
            return false;
        };
        // SAFETY: The returned interface is live and starts with IUnknown.
        unsafe { ((**pointer.as_ptr().cast::<*const IUnknownVtbl>()).Release)(pointer.as_ptr()) };
        true
    }

    fn query_raw(&self, iid: &GUID) -> Result<NonNull<c_void>, i32> {
        let mut pointer = null_mut();
        // SAFETY: The source interface is live and pointer is writable.
        check(unsafe { (self.unknown().QueryInterface)(self.as_ptr().cast(), iid, &mut pointer) })?;
        NonNull::new(pointer).ok_or(E_POINTER)
    }

    fn unknown(&self) -> &IUnknownVtbl {
        // Every COM interface begins with an IUnknown vtable prefix.
        unsafe { &**self.as_ptr().cast::<*const IUnknownVtbl>() }
    }
}

impl<T> Clone for ComPtr<T> {
    fn clone(&self) -> Self {
        unsafe { (self.unknown().AddRef)(self.as_ptr().cast()) };
        Self {
            pointer: self.pointer,
            _thread: PhantomData,
        }
    }
}

impl<T> Deref for ComPtr<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.pointer.as_ref() }
    }
}

impl<T> Drop for ComPtr<T> {
    fn drop(&mut self) {
        unsafe { (self.unknown().Release)(self.as_ptr().cast()) };
    }
}

pub(crate) const fn check(result: i32) -> Result<(), i32> {
    if result < 0 { Err(result) } else { Ok(()) }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[repr(C)]
    struct Object {
        vtable: *const IUnknownVtbl,
        refs: Cell<u32>,
        releases: Cell<u32>,
    }

    // SAFETY: This IID describes the mock object's IUnknown-compatible interface.
    unsafe impl ComInterface for Object {
        const IID: GUID = GUID {
            data1: 1,
            data2: 0,
            data3: 0,
            data4: [0; 8],
        };
    }

    unsafe extern "system" fn query(
        this: *mut c_void,
        _: *const GUID,
        out: *mut *mut c_void,
    ) -> i32 {
        // Simulate a COM implementation that writes a borrowed pointer despite returning failure.
        unsafe { *out = this };
        -1
    }

    unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
        let object = unsafe { &*this.cast::<Object>() };
        object.refs.set(object.refs.get() + 1);
        object.refs.get()
    }

    unsafe extern "system" fn release(this: *mut c_void) -> u32 {
        let object = unsafe { &*this.cast::<Object>() };
        object.refs.set(object.refs.get() - 1);
        object.releases.set(object.releases.get() + 1);
        object.refs.get()
    }

    const VTABLE: IUnknownVtbl = IUnknownVtbl {
        QueryInterface: query,
        AddRef: add_ref,
        Release: release,
    };

    #[test]
    fn owned_references_clone_and_release_once() {
        let mut object = Object {
            vtable: &VTABLE,
            refs: Cell::new(1),
            releases: Cell::new(0),
        };
        let owned = unsafe {
            ComPtr::create(|out| {
                *out = &mut object;
                0
            })
        }
        .expect("COM object");
        let cloned = owned.clone();
        assert_eq!(object.refs.get(), 2);
        drop(owned);
        assert_eq!(cloned.refs.get(), 1);
        drop(cloned);
        assert_eq!(object.refs.get(), 0);
        assert_eq!(object.releases.get(), 2);
    }

    #[test]
    fn failed_query_does_not_adopt_the_output() {
        let mut object = Object {
            vtable: &VTABLE,
            refs: Cell::new(1),
            releases: Cell::new(0),
        };
        let owned = unsafe {
            ComPtr::create(|out| {
                *out = &mut object;
                0
            })
        }
        .expect("COM object");
        assert!(matches!(owned.query::<Object>(), Err(-1)));
        assert_eq!(object.refs.get(), 1);
        assert_eq!(object.releases.get(), 0);
        drop(owned);
        assert_eq!(object.releases.get(), 1);
    }

    #[test]
    fn failed_creation_releases_output_and_null_success_is_rejected() {
        let mut object = Object {
            vtable: &VTABLE,
            refs: Cell::new(1),
            releases: Cell::new(0),
        };
        assert!(
            unsafe {
                ComPtr::create(|out| {
                    *out = &mut object;
                    -1
                })
            }
            .is_err()
        );
        assert_eq!(object.refs.get(), 0);
        assert!(unsafe { ComPtr::<Object>::create(|_| 0) }.is_err());
        assert!(check(1).is_ok());
    }
}
