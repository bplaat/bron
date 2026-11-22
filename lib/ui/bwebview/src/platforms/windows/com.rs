/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::{NonNull, null_mut};
use std::rc::Rc;

use super::headers::{E_POINTER, GUID, IUnknownVtbl};

/// Associates an interface's ABI type with its IID.
///
/// # Safety
/// Implementors must represent the interface identified by IID with an IUnknown-compatible vtable.
pub(crate) unsafe trait ComInterface {
    const IID: GUID;
}

/// One owned COM reference on the window thread.
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
        let owned = unsafe { Self::from_raw(pointer) };
        check(result)?;
        owned.ok_or(E_POINTER)
    }

    /// Takes ownership of one reference to a live COM interface.
    pub(crate) unsafe fn from_raw(pointer: *mut T) -> Option<Self> {
        NonNull::new(pointer).map(|pointer| Self {
            pointer,
            _thread: PhantomData,
        })
    }

    /// Acquires a reference to a borrowed COM interface.
    pub(crate) unsafe fn retain(pointer: *mut T) -> Option<Self> {
        let owned = unsafe { Self::from_raw(pointer) }?;
        unsafe { (owned.unknown().AddRef)(pointer.cast()) };
        Some(owned)
    }

    pub(crate) const fn as_ptr(&self) -> *mut T {
        self.pointer.as_ptr()
    }

    pub(crate) fn query<U: ComInterface>(&self) -> Result<ComPtr<U>, i32> {
        let mut pointer = null_mut();
        // SAFETY: U supplies its IID and QueryInterface receives a writable out parameter.
        let result =
            unsafe { (self.unknown().QueryInterface)(self.as_ptr().cast(), &U::IID, &mut pointer) };
        check(result)?;
        // SAFETY: A successful query transfers one reference to U.
        unsafe { ComPtr::from_raw(pointer.cast()) }.ok_or(E_POINTER)
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
