/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use core::ffi::c_void;

pub type lwp_t = u32;
pub type mutex_t = u32;
pub type cond_t = u32;

pub const LWP_THREAD_NULL: lwp_t = 0xffff_ffff;
pub const LWP_MUTEX_NULL: mutex_t = 0xffff_ffff;
pub const LWP_COND_NULL: cond_t = 0xffff_ffff;

unsafe extern "C" {
    pub fn LWP_CreateThread(
        thread: *mut lwp_t,
        entry: extern "C" fn(*mut c_void) -> *mut c_void,
        argument: *mut c_void,
        stack: *mut c_void,
        stack_size: u32,
        priority: u8,
    ) -> i32;
    pub fn LWP_JoinThread(thread: lwp_t, result: *mut *mut c_void) -> i32;
    pub fn LWP_GetSelf() -> lwp_t;
    pub fn LWP_SuspendThread(thread: lwp_t) -> i32;
    pub fn LWP_ResumeThread(thread: lwp_t) -> i32;
    pub fn LWP_ThreadIsSuspended(thread: lwp_t) -> u32;
    pub fn LWP_SetThreadPriority(thread: lwp_t, priority: u32);
    pub fn LWP_YieldThread();
    pub fn LWP_MutexDestroy(mutex: mutex_t) -> i32;
    pub fn LWP_MutexTryLock(mutex: mutex_t) -> i32;
    pub fn LWP_MutexInit(mutex: *mut mutex_t, recursive: bool) -> i32;
    pub fn LWP_MutexLock(mutex: mutex_t) -> i32;
    pub fn LWP_MutexUnlock(mutex: mutex_t) -> i32;
    pub fn LWP_CondInit(cond: *mut cond_t) -> i32;
    pub fn LWP_CondWait(cond: cond_t, mutex: mutex_t) -> i32;
    pub fn LWP_CondSignal(cond: cond_t) -> i32;
    pub fn LWP_CondBroadcast(cond: cond_t) -> i32;
    pub fn LWP_CondDestroy(cond: cond_t) -> i32;
}
