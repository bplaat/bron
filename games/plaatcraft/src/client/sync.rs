/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A mutex and condition variable on top of the system's, like std's.
#![allow(unsafe_code)]

use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};

use super::platform::{Lock, Signal};

pub(super) struct Mutex<L, T> {
    lock: L,
    value: UnsafeCell<T>,
}

// SAFETY: The lock gives one thread at a time access to the value.
unsafe impl<L: Lock, T: Send> Sync for Mutex<L, T> {}

impl<L: Lock, T> Mutex<L, T> {
    pub(super) fn new(value: T) -> Self {
        Self {
            lock: L::new(),
            value: UnsafeCell::new(value),
        }
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, L, T> {
        self.lock.lock();
        MutexGuard {
            mutex: self,
            owner: PhantomData,
        }
    }
}

pub(super) struct MutexGuard<'a, L: Lock, T> {
    mutex: &'a Mutex<L, T>,
    // The native mutex must be unlocked by the thread that acquired it.
    owner: PhantomData<*mut ()>,
}

impl<L: Lock, T> Deref for MutexGuard<'_, L, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: The guard holds the lock, so no other thread uses the value.
        unsafe { &*self.mutex.value.get() }
    }
}

impl<L: Lock, T> DerefMut for MutexGuard<'_, L, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: The guard holds the lock, so no other thread uses the value.
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<L: Lock, T> Drop for MutexGuard<'_, L, T> {
    fn drop(&mut self) {
        // SAFETY: This guard holds the mutex and stays on the acquiring thread.
        unsafe { self.mutex.lock.unlock() };
    }
}

pub(super) struct Condvar<L, S> {
    signal: S,
    lock: PhantomData<L>,
}

impl<L: Lock, S: Signal<L>> Condvar<L, S> {
    pub(super) fn new() -> Self {
        Self {
            signal: S::new(),
            lock: PhantomData,
        }
    }

    /// Unlocks the mutex until the condition is signaled.
    pub(super) fn wait<'a, T>(&self, guard: MutexGuard<'a, L, T>) -> MutexGuard<'a, L, T> {
        // SAFETY: The guard gives this thread ownership of its mutex.
        unsafe { self.signal.wait(&guard.mutex.lock) };
        guard
    }

    pub(super) fn signal(&self) {
        self.signal.signal();
    }
}
