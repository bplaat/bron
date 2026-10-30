/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Reference traits and global references.

use std::ops::Deref;

use crate::objects::JObject;
use crate::strings::JNIStr;
use crate::sys::jobject;
use crate::vm::JavaVM;

/// A transparent wrapper of a Java object reference.
///
/// # Safety
/// The implementing type must be a transparent `jobject` wrapper of the named class.
pub unsafe trait Reference: Sized {
    /// The type with a local lifetime.
    type Kind<'local>: Reference + Default + Into<JObject<'local>> + AsRef<JObject<'local>> + 'local;
    /// The type of a global reference.
    type GlobalKind: Reference
        + Default
        + Into<JObject<'static>>
        + AsRef<JObject<'static>>
        + Send
        + Sync
        + 'static;

    /// The raw reference.
    fn as_raw(&self) -> jobject;

    /// Whether the reference is null.
    fn is_null(&self) -> bool {
        self.as_raw().is_null()
    }

    /// The JNI name of the class, like `java/lang/String`.
    fn class_name() -> &'static JNIStr;

    /// Wrap a raw local reference.
    ///
    /// # Safety
    /// `local_ref` must be a valid local reference of this class or null.
    unsafe fn kind_from_raw<'local>(local_ref: jobject) -> Self::Kind<'local>;

    /// Wrap a raw global reference.
    ///
    /// # Safety
    /// `global_ref` must be a valid global reference of this class or null.
    unsafe fn global_kind_from_raw(global_ref: jobject) -> Self::GlobalKind;
}

// SAFETY: A reference to a reference type refers to the same object.
unsafe impl<T: Reference> Reference for &T {
    type Kind<'local> = T::Kind<'local>;
    type GlobalKind = T::GlobalKind;

    fn as_raw(&self) -> jobject {
        (*self).as_raw()
    }

    fn class_name() -> &'static JNIStr {
        T::class_name()
    }

    unsafe fn kind_from_raw<'local>(local_ref: jobject) -> Self::Kind<'local> {
        unsafe { T::kind_from_raw(local_ref) }
    }

    unsafe fn global_kind_from_raw(global_ref: jobject) -> Self::GlobalKind {
        unsafe { T::global_kind_from_raw(global_ref) }
    }
}

/// An owned global reference, valid on every thread until it is dropped.
#[derive(Debug)]
pub struct Global<T>
where
    T: Into<JObject<'static>>
        + AsRef<JObject<'static>>
        + Default
        + Reference
        + Send
        + Sync
        + 'static,
{
    obj: T,
}

impl<T> Global<T>
where
    T: Into<JObject<'static>>
        + AsRef<JObject<'static>>
        + Default
        + Reference
        + Send
        + Sync
        + 'static,
{
    pub(crate) const fn new(obj: T) -> Self {
        Self { obj }
    }

    /// The referenced object.
    pub fn as_obj(&self) -> &JObject<'static> {
        self.obj.as_ref()
    }
}

impl<T> Deref for Global<T>
where
    T: Into<JObject<'static>>
        + AsRef<JObject<'static>>
        + Default
        + Reference
        + Send
        + Sync
        + 'static,
{
    type Target = T;

    fn deref(&self) -> &T {
        &self.obj
    }
}

impl<T, U> AsRef<U> for Global<T>
where
    T: AsRef<U>
        + Into<JObject<'static>>
        + AsRef<JObject<'static>>
        + Default
        + Reference
        + Send
        + Sync
        + 'static,
{
    fn as_ref(&self) -> &U {
        self.obj.as_ref()
    }
}

impl<T> Drop for Global<T>
where
    T: Into<JObject<'static>>
        + AsRef<JObject<'static>>
        + Default
        + Reference
        + Send
        + Sync
        + 'static,
{
    fn drop(&mut self) {
        let raw = self.obj.as_raw();
        if raw.is_null() {
            return;
        }
        // A global reference can only exist after the Java VM was registered. Deleting it is
        // allowed with a pending exception, which belongs to the caller and stays pending.
        if let Ok(env) = JavaVM::singleton().and_then(|vm| vm.env()) {
            unsafe { ((**env).DeleteGlobalRef)(env, raw) };
        }
    }
}
