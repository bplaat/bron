/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Java VM.

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

use crate::Env;
use crate::errors::{Error, JniError, Result};
use crate::sys::{self, JNI_OK, JNI_VERSION_1_6};

static SINGLETON: AtomicPtr<sys::JavaVM> = AtomicPtr::new(null_mut());

/// The Java VM of the process, there is only one.
#[derive(Debug, Clone)]
pub struct JavaVM(*mut sys::JavaVM);

// SAFETY: The Java VM interface is thread safe.
unsafe impl Send for JavaVM {}
// SAFETY: See above.
unsafe impl Sync for JavaVM {}

// Threads attached by this crate detach again when they exit.
struct AttachGuard;

impl Drop for AttachGuard {
    fn drop(&mut self) {
        if let Ok(vm) = JavaVM::singleton() {
            unsafe { ((**vm.0).DetachCurrentThread)(vm.0) };
        }
    }
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static ATTACHED: AttachGuard = const { AttachGuard };
}

impl JavaVM {
    /// Wrap the raw Java VM, the first one becomes the [`Self::singleton`].
    ///
    /// # Safety
    /// `ptr` must be the valid Java VM of the process.
    pub unsafe fn from_raw(ptr: *mut sys::JavaVM) -> Self {
        let _ = SINGLETON.compare_exchange(null_mut(), ptr, Ordering::AcqRel, Ordering::Acquire);
        Self(ptr)
    }

    /// The Java VM registered with [`Self::from_raw`] or by a native method.
    pub fn singleton() -> Result<Self> {
        let vm = SINGLETON.load(Ordering::Acquire);
        if vm.is_null() {
            Err(Error::UninitializedJavaVM)
        } else {
            Ok(Self(vm))
        }
    }

    // The environment of the current thread, attaching it until it exits if needed.
    pub(crate) fn env(&self) -> Result<*mut sys::JNIEnv> {
        let mut env = null_mut::<c_void>();
        if unsafe { ((**self.0).GetEnv)(self.0, &mut env, JNI_VERSION_1_6) } == JNI_OK {
            return Ok(env.cast());
        }
        let result = unsafe { ((**self.0).AttachCurrentThread)(self.0, &mut env, null_mut()) };
        if result != JNI_OK {
            return Err(Error::JniCall(JniError::from_code(result)));
        }
        ATTACHED.with(|_| {});
        Ok(env.cast())
    }

    /// Run `callback` in a new JNI stack frame on the current thread, attaching it to the Java
    /// VM until it exits if needed. A pending Java exception is logged, cleared, and returned
    /// as [`Error::CaughtJavaException`].
    pub fn attach_current_thread<F, T, E>(&self, callback: F) -> std::result::Result<T, E>
    where
        F: FnOnce(&mut Env) -> std::result::Result<T, E>,
        E: From<Error>,
    {
        let env = unsafe { Env::from_raw(self.env()?) };
        let result = env.with_local_frame(32, callback);
        match env.catch_exception() {
            Some(error) => Err(error.into()),
            None => result,
        }
    }
}
