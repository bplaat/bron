/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::{c_char, c_int, c_void};

use crate::{BOOL, OSTime};

pub const OS_THREAD_ATTRIB_AFFINITY_CPU0: u8 = 1;
pub const OS_THREAD_ATTRIB_AFFINITY_CPU1: u8 = 2;
pub const OS_THREAD_ATTRIB_AFFINITY_CPU2: u8 = 4;
pub const OS_THREAD_ATTRIB_AFFINITY_ANY: u8 = 7;
pub const OS_THREAD_ATTRIB_DETACHED: u8 = 8;
pub const OS_THREAD_ATTRIB_STACK_USAGE: u8 = 32;

pub type OSThreadEntryPointFn = unsafe extern "C" fn(c_int, *mut *const c_char) -> c_int;

/// Storage for wut's thread object. Initialise with `OSCreateThread` before use.
#[repr(C, align(8))]
pub struct OSThread {
    _private: [u8; 0x6a0],
}

#[repr(C)]
pub struct OSThreadQueue {
    pub head: *mut OSThread,
    pub tail: *mut OSThread,
    pub parent: *mut c_void,
    _unknown: [u8; 4],
}

#[repr(C)]
pub struct OSMutexLink {
    pub next: *mut OSMutex,
    pub prev: *mut OSMutex,
}

#[repr(C)]
pub struct OSMutex {
    pub tag: u32,
    pub name: *const c_char,
    _unknown: [u8; 4],
    pub queue: OSThreadQueue,
    pub owner: *mut OSThread,
    pub count: i32,
    pub link: OSMutexLink,
}

#[repr(C)]
pub struct OSCondition {
    pub tag: u32,
    pub name: *const c_char,
    _unknown: [u8; 4],
    pub queue: OSThreadQueue,
}

unsafe extern "C" {
    /// `stack` points to the top (highest address) of a 16-byte-aligned stack.
    /// A newly created thread is suspended; call `OSResumeThread` to start it.
    pub fn OSCreateThread(
        thread: *mut OSThread,
        entry: OSThreadEntryPointFn,
        argc: i32,
        argv: *mut c_char,
        stack: *mut c_void,
        stack_size: u32,
        priority: i32,
        attributes: u8,
    ) -> BOOL;
    pub fn OSResumeThread(thread: *mut OSThread) -> i32;
    pub fn OSJoinThread(thread: *mut OSThread, result: *mut c_int) -> BOOL;
    pub fn OSDetachThread(thread: *mut OSThread);
    pub fn OSGetCurrentThread() -> *mut OSThread;
    pub fn OSGetThreadPriority(thread: *mut OSThread) -> i32;
    pub fn OSSetThreadPriority(thread: *mut OSThread, priority: i32) -> BOOL;
    pub fn OSSetThreadAffinity(thread: *mut OSThread, affinity: u32) -> BOOL;
    pub fn OSYieldThread();
    pub fn OSSleepTicks(ticks: OSTime);
    pub fn OSInitMutex(mutex: *mut OSMutex);
    pub fn OSLockMutex(mutex: *mut OSMutex);
    pub fn OSTryLockMutex(mutex: *mut OSMutex) -> BOOL;
    pub fn OSUnlockMutex(mutex: *mut OSMutex);
    pub fn OSInitCond(condition: *mut OSCondition);
    pub fn OSWaitCond(condition: *mut OSCondition, mutex: *mut OSMutex);
    pub fn OSSignalCond(condition: *mut OSCondition);
    pub fn DCFlushRange(pointer: *mut c_void, size: u32);
    pub fn DCInvalidateRange(pointer: *mut c_void, size: u32);
}
