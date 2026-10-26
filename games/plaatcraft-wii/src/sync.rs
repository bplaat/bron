/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Threads, mutexes and condition variables on top of libogc's LWP kernel.

use alloc::boxed::Box;
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::ops::{Deref, DerefMut};

use libogc_sys::*;

pub struct Mutex<T> {
    handle: mutex_t,
    value: UnsafeCell<T>,
}

// The LWP mutex gives one thread at a time access to the value
unsafe impl<T: Send> Sync for Mutex<T> {}

impl<T> Mutex<T> {
    pub fn new(value: T) -> Self {
        let mut handle = LWP_MUTEX_NULL;
        unsafe { LWP_MutexInit(&mut handle, false) };
        Self {
            handle,
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, T> {
        unsafe { LWP_MutexLock(self.handle) };
        MutexGuard { mutex: self }
    }
}

pub struct MutexGuard<'a, T> {
    mutex: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { LWP_MutexUnlock(self.mutex.handle) };
    }
}

pub struct Condvar(cond_t);

unsafe impl Sync for Condvar {}

impl Condvar {
    pub fn new() -> Self {
        let mut handle = LWP_COND_NULL;
        unsafe { LWP_CondInit(&mut handle) };
        Self(handle)
    }

    /// Unlocks the mutex until the condition is signaled.
    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        unsafe { LWP_CondWait(self.0, guard.mutex.handle) };
        guard
    }

    pub fn signal(&self) {
        unsafe { LWP_CondSignal(self.0) };
    }
}

/// Starts a thread that runs forever, higher priorities run first. The stack is allocated
/// on the heap, libogc's own thread stacks come from a small workspace.
pub fn spawn(stack_size: usize, priority: u8, entry: impl FnOnce() + Send + 'static) {
    extern "C" fn start(argument: *mut c_void) -> *mut c_void {
        let entry = unsafe { Box::from_raw(argument.cast::<Box<dyn FnOnce()>>()) };
        entry();
        core::ptr::null_mut()
    }
    let entry: Box<Box<dyn FnOnce()>> = Box::new(Box::new(entry));
    let stack = Box::leak(alloc::vec![0u8; stack_size].into_boxed_slice());
    let mut thread = LWP_THREAD_NULL;
    let result = unsafe {
        LWP_CreateThread(
            &mut thread,
            start,
            Box::into_raw(entry).cast(),
            stack.as_mut_ptr().cast(),
            stack_size as u32,
            priority,
        )
    };
    assert!(result >= 0, "failed to start thread");
}
