/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::{NonNull, null_mut};
use std::rc::Rc;

use super::headers::IUnknownVtbl;

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
        let owned = NonNull::new(pointer).map(|pointer| Self {
            pointer,
            _thread: PhantomData,
        });
        check(result)?;
        owned.ok_or(0x80004003u32 as i32)
    }

    pub(crate) const fn as_ptr(&self) -> *mut T {
        self.pointer.as_ptr()
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
