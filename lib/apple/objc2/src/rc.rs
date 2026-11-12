/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt;
use std::marker::PhantomData;
use std::mem::{DropGuard, ManuallyDrop};
use std::ops::Deref;
use std::ptr::NonNull;

use crate::encode::{Encode, Encoding};
use crate::ffi::{
    objc_autorelease, objc_autoreleasePoolPop, objc_autoreleasePoolPush, objc_release, objc_retain,
};
use crate::runtime::{AnyObject, DefinedClass, Message};

/// An Objective-C object that has been allocated but not initialized.
#[repr(transparent)]
pub struct Allocated<T: Message> {
    pointer: *mut T,
    marker: PhantomData<T>,
}

impl<T: Message> Allocated<T> {
    /// Returns the possibly null allocation pointer.
    pub const fn as_ptr(&self) -> *mut T {
        self.pointer
    }

    pub(crate) fn into_raw(self) -> *mut T {
        ManuallyDrop::new(self).pointer
    }

    /// Initialize this class's Rust instance variables before calling its superclass initializer.
    pub fn set_ivars(self, ivars: T::Ivars) -> PartialInit<T>
    where
        T: DefinedClass,
    {
        let pointer = ManuallyDrop::new(self).pointer;
        if let Some(object) = NonNull::new(pointer) {
            // SAFETY: the pointer comes from this uniquely owned allocation and has not had this
            // class's ivars initialized yet.
            unsafe { crate::runtime::initialize_ivars::<T>(object, ivars) };
            PartialInit {
                pointer,
                marker: PhantomData,
            }
        } else {
            drop(ivars);
            if cfg!(debug_assertions) {
                panic!("cannot initialize ivars on a null object");
            }
            PartialInit {
                pointer,
                marker: PhantomData,
            }
        }
    }

    #[doc(hidden)]
    pub const unsafe fn from_raw(pointer: *mut T) -> Self {
        Self {
            pointer,
            marker: PhantomData,
        }
    }
}

impl<T: Message> Drop for Allocated<T> {
    fn drop(&mut self) {
        // SAFETY: Allocated always owns the +1 allocation, and objc_release accepts null.
        unsafe { objc_release(self.pointer.cast::<AnyObject>()) };
    }
}

impl<T: Message> fmt::Debug for Allocated<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.pointer, formatter)
    }
}

// SAFETY: Allocated has the pointer ABI and Objective-C object encoding.
unsafe impl<T: Message> Encode for Allocated<T> {
    const ENCODING: Encoding = Encoding::Object;
    const IS_OBJECT_OWNERSHIP: bool = true;

    unsafe fn from_object_return(pointer: *mut std::ffi::c_void, _: bool) -> Self {
        Self {
            pointer: pointer.cast::<T>(),
            marker: PhantomData,
        }
    }
}

/// An Objective-C object whose Rust ivars are initialized but whose superclass is not yet.
#[repr(transparent)]
pub struct PartialInit<T: Message> {
    pointer: *mut T,
    marker: PhantomData<T>,
}

impl<T: Message> PartialInit<T> {
    pub(crate) fn into_raw(self) -> *mut T {
        ManuallyDrop::new(self).pointer
    }
}

impl<T: Message> Drop for PartialInit<T> {
    fn drop(&mut self) {
        // SAFETY: PartialInit owns the allocation's +1 retain count. The generated dealloc method
        // checks whether ivars were initialized before dropping them.
        unsafe { objc_release(self.pointer.cast::<AnyObject>()) };
    }
}

impl<T: Message> fmt::Debug for PartialInit<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.pointer, formatter)
    }
}

/// A strong reference to an initialized Objective-C object.
#[repr(transparent)]
pub struct Retained<T: Message> {
    pointer: NonNull<T>,
}

impl<T: Message> Retained<T> {
    /// Takes ownership of an object with a +1 retain count.
    ///
    /// # Safety
    ///
    /// `pointer` must either be null or point to a valid initialized `T` with a +1 retain count.
    pub unsafe fn from_raw(pointer: *mut T) -> Option<Self> {
        NonNull::new(pointer).map(|pointer| Self { pointer })
    }

    /// Retains an existing Objective-C object.
    ///
    /// # Safety
    ///
    /// `pointer` must either be null or point to a valid initialized `T`.
    pub unsafe fn retain(pointer: *mut T) -> Option<Self> {
        // SAFETY: The caller guarantees pointer is null or a live Objective-C object.
        let pointer = unsafe { objc_retain(pointer.cast::<AnyObject>()) }.cast::<T>();
        // SAFETY: objc_retain returns the same valid pointer with a +1 retain count.
        unsafe { Self::from_raw(pointer) }
    }

    /// Returns the object pointer, which remains valid while this value is alive.
    pub const fn as_ptr(&self) -> *mut T {
        self.pointer.as_ptr()
    }

    /// Transfers the +1 retain count to the caller.
    pub fn into_raw(this: Self) -> *mut T {
        ManuallyDrop::new(this).pointer.as_ptr()
    }

    /// Erase the concrete Objective-C object type while preserving ownership.
    pub fn into_any(this: Self) -> Retained<AnyObject> {
        Retained {
            pointer: ManuallyDrop::new(this).pointer.cast::<AnyObject>(),
        }
    }

    /// Transfers ownership to the current autorelease pool.
    pub fn autorelease_ptr(this: Self) -> *mut T {
        let pointer = Self::into_raw(this);
        // SAFETY: The pointer owns a +1 retain count transferred to objc_autorelease.
        unsafe { objc_autorelease(pointer.cast::<AnyObject>()) }.cast::<T>()
    }
}

impl<T: Message> Clone for Retained<T> {
    fn clone(&self) -> Self {
        // SAFETY: self keeps the initialized object live during the retain.
        unsafe { Self::retain(self.as_ptr()) }.expect("retaining a non-null object returned null")
    }
}

impl<T: Message> Drop for Retained<T> {
    fn drop(&mut self) {
        // SAFETY: Retained owns one retain count for this live object.
        unsafe { objc_release(self.pointer.as_ptr().cast::<AnyObject>()) };
    }
}

impl<T: Message> Deref for Retained<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: Retained contains a non-null pointer kept live by its retain count.
        unsafe { self.pointer.as_ref() }
    }
}

impl<T: Message> fmt::Debug for Retained<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Pointer::fmt(&self.pointer, formatter)
    }
}

// SAFETY: Retained has the pointer ABI and Objective-C object encoding.
unsafe impl<T: Message> Encode for Retained<T> {
    const ENCODING: Encoding = Encoding::Object;
    const IS_OBJECT_OWNERSHIP: bool = true;

    unsafe fn from_object_return(pointer: *mut std::ffi::c_void, owned: bool) -> Self {
        let pointer = pointer.cast::<T>();
        let retained = if owned {
            // SAFETY: An owned method family returns a +1 object.
            unsafe { Self::from_raw(pointer) }
        } else {
            // SAFETY: A non-owned method result is live at the return boundary.
            unsafe { Self::retain(pointer) }
        };
        retained.expect("Objective-C method returned null for Retained")
    }
}

// SAFETY: Option<Retained<T>> uses the NonNull null-pointer optimization and object encoding.
unsafe impl<T: Message> Encode for Option<Retained<T>> {
    const ENCODING: Encoding = Encoding::Object;
    const IS_OBJECT_OWNERSHIP: bool = true;

    unsafe fn from_object_return(pointer: *mut std::ffi::c_void, owned: bool) -> Self {
        let pointer = pointer.cast::<T>();
        if owned {
            // SAFETY: An owned method family returns null or a +1 object.
            unsafe { Retained::from_raw(pointer) }
        } else {
            // SAFETY: A non-owned method result is null or live at the return boundary.
            unsafe { Retained::retain(pointer) }
        }
    }
}

/// A token representing an active autorelease pool
pub struct AutoreleasePool(());

/// Run a closure within an autorelease pool
pub fn autoreleasepool<F, R>(f: F) -> R
where
    F: FnOnce(&AutoreleasePool) -> R,
{
    // SAFETY: The guard guarantees a matching pop, including while unwinding.
    let _guard = DropGuard::new(unsafe { objc_autoreleasePoolPush() }, |pool| {
        // SAFETY: The guard is dropped on the thread that pushed this pool.
        unsafe { objc_autoreleasePoolPop(pool) };
    });
    f(&AutoreleasePool(()))
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_autoreleasepool_runs_closure() {
        let mut ran = false;
        autoreleasepool(|_| {
            ran = true;
        });
        assert!(ran);
    }

    #[test]
    fn test_autoreleasepool_returns_value() {
        let result = autoreleasepool(|_| 42_i32);
        assert_eq!(result, 42);
    }

    #[test]
    fn test_autoreleasepool_nested() {
        let result = autoreleasepool(|_| autoreleasepool(|_| 7_i32));
        assert_eq!(result, 7);
    }

    #[test]
    fn test_autoreleasepool_token_accessible() {
        autoreleasepool(|pool| {
            let _: &AutoreleasePool = pool;
        });
    }

    #[test]
    fn test_autoreleasepool_with_objc_objects() {
        #[link(name = "Foundation", kind = "framework")]
        unsafe extern "C" {}

        autoreleasepool(|_| {
            // SAFETY: NSObject is a valid Foundation class; new returns a fully initialized object;
            // autorelease hands ownership to the pool, which releases it when drained.
            unsafe {
                let obj: *mut AnyObject = crate::msg_send![crate::class!(NSObject), new];
                assert!(!obj.is_null());
                let _: *mut AnyObject = crate::msg_send![obj, autorelease];
            }
        });
    }

    #[test]
    fn test_allocated_and_retained_manage_ownership() {
        #[link(name = "Foundation", kind = "framework")]
        unsafe extern "C" {}

        autoreleasepool(|_| {
            // SAFETY: NSObject supports alloc, init and retainCount with these signatures.
            unsafe {
                let object: Allocated<AnyObject> = crate::msg_send![crate::class!(NSObject), alloc];
                let object: Retained<AnyObject> = crate::msg_send![object, init];
                let count: usize = crate::msg_send![&object, retainCount];
                let clone = object.clone();
                let cloned_count: usize = crate::msg_send![&object, retainCount];
                assert_eq!(cloned_count, count + 1);
                drop(clone);
                let final_count: usize = crate::msg_send![&object, retainCount];
                assert_eq!(final_count, count);

                let retained: Retained<AnyObject> = crate::msg_send![&object, retain];
                let retained_count: usize = crate::msg_send![&object, retainCount];
                assert_eq!(retained_count, count + 1);
                drop(retained);
            }
        });
    }

    #[test]
    fn test_retained_non_owned_return_is_kept_alive() {
        #[link(name = "Foundation", kind = "framework")]
        unsafe extern "C" {}

        autoreleasepool(|_| {
            // SAFETY: NSNull's null method returns its shared initialized object.
            let object: Option<Retained<AnyObject>> =
                unsafe { crate::msg_send![crate::class!(NSNull), null] };
            assert!(object.is_some());
        });
    }
}
