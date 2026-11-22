/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::NonNull;
use std::rc::Rc;

use super::headers::IUnknownVtbl;

/// One owned COM reference on the window thread.
pub(crate) struct ComPtr<T> {
    pointer: NonNull<T>,
    _thread: PhantomData<Rc<()>>,
}

impl<T> ComPtr<T> {
    /// Takes ownership of one reference to a live COM interface.
    pub(crate) unsafe fn from_raw(pointer: *mut T) -> Option<Self> {
        NonNull::new(pointer).map(|pointer| Self {
            pointer,
            _thread: PhantomData,
        })
    }

    /// Acquires a reference to a borrowed COM interface.
    pub(crate) unsafe fn retain(pointer: *mut T) -> Option<Self> {
        // SAFETY: The caller supplies a live interface or null.
        let owned = unsafe { Self::from_raw(pointer) }?;
        // SAFETY: The borrowed reference keeps the interface live through AddRef.
        unsafe { (owned.unknown().AddRef)(pointer.cast()) };
        Some(owned)
    }

    pub(crate) const fn as_ptr(&self) -> *mut T {
        self.pointer.as_ptr()
    }

    fn unknown(&self) -> &IUnknownVtbl {
        // SAFETY: Every live COM interface begins with an IUnknown vtable prefix.
        unsafe { &**self.as_ptr().cast::<*const IUnknownVtbl>() }
    }
}

impl<T> Clone for ComPtr<T> {
    fn clone(&self) -> Self {
        // SAFETY: This reference keeps the interface alive during AddRef.
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
        // SAFETY: The owned pointer keeps the interface alive for this borrow.
        unsafe { self.pointer.as_ref() }
    }
}

impl<T> Drop for ComPtr<T> {
    fn drop(&mut self) {
        // SAFETY: This pointer owns the reference it releases.
        unsafe { (self.unknown().Release)(self.as_ptr().cast()) };
    }
}
