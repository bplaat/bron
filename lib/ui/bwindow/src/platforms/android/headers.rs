/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    reason = "Raw NDK declarations"
)]

use std::ffi::{c_char, c_int, c_long, c_void};

use jni::sys::{JNIEnv, JavaVM, jobject};

// MARK: NativeActivity
#[repr(C)]
pub struct ANativeActivity {
    pub callbacks: *mut ANativeActivityCallbacks,
    pub vm: *mut JavaVM,
    pub env: *mut JNIEnv,
    pub clazz: jobject,
    pub internalDataPath: *const c_char,
    pub externalDataPath: *const c_char,
    pub sdkVersion: i32,
    pub instance: *mut c_void,
    pub assetManager: *mut c_void,
    pub obbPath: *const c_char,
}

type ActivityCallback = Option<unsafe extern "C" fn(*mut ANativeActivity)>;
type WindowCallback = Option<unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow)>;
type QueueCallback = Option<unsafe extern "C" fn(*mut ANativeActivity, *mut AInputQueue)>;

#[repr(C)]
pub struct ANativeActivityCallbacks {
    pub onStart: ActivityCallback,
    pub onResume: ActivityCallback,
    pub onSaveInstanceState:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut usize) -> *mut c_void>,
    pub onPause: ActivityCallback,
    pub onStop: ActivityCallback,
    pub onDestroy: ActivityCallback,
    pub onWindowFocusChanged: Option<unsafe extern "C" fn(*mut ANativeActivity, c_int)>,
    pub onNativeWindowCreated: WindowCallback,
    pub onNativeWindowResized: WindowCallback,
    pub onNativeWindowRedrawNeeded: WindowCallback,
    pub onNativeWindowDestroyed: WindowCallback,
    pub onInputQueueCreated: QueueCallback,
    pub onInputQueueDestroyed: QueueCallback,
    pub onContentRectChanged: Option<unsafe extern "C" fn(*mut ANativeActivity, *const ARect)>,
    pub onConfigurationChanged: ActivityCallback,
    pub onLowMemory: ActivityCallback,
}

#[repr(C)]
pub struct ARect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
pub struct ANativeWindow {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AInputQueue {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AInputEvent {
    _private: [u8; 0],
}

#[repr(C)]
pub struct ALooper {
    _private: [u8; 0],
}

#[repr(C)]
pub struct AChoreographer {
    _private: [u8; 0],
}

pub type ALooper_callbackFunc = unsafe extern "C" fn(c_int, c_int, *mut c_void) -> c_int;
pub type AChoreographer_frameCallback = unsafe extern "C" fn(c_long, *mut c_void);

pub const ALOOPER_PREPARE_ALLOW_NON_CALLBACKS: c_int = 1;
pub const ALOOPER_POLL_CALLBACK: c_int = -2;
pub const ALOOPER_EVENT_INPUT: c_int = 1;
pub const WINDOW_FORMAT_RGBA_8888: i32 = 1;

pub const AINPUT_EVENT_TYPE_KEY: i32 = 1;
pub const AINPUT_EVENT_TYPE_MOTION: i32 = 2;
pub const AKEY_EVENT_ACTION_DOWN: i32 = 0;
pub const AKEY_EVENT_ACTION_UP: i32 = 1;
pub const AKEYCODE_BACK: i32 = 4;
pub const AMOTION_EVENT_ACTION_MASK: i32 = 0xff;
pub const AMOTION_EVENT_AXIS_VSCROLL: i32 = 9;
pub const AMOTION_EVENT_AXIS_HSCROLL: i32 = 10;
pub const AMOTION_EVENT_TOOL_TYPE_MOUSE: i32 = 3;

#[link(name = "android")]
unsafe extern "C" {
    pub fn ANativeActivity_finish(activity: *mut ANativeActivity);
    pub fn ANativeActivity_setWindowFormat(activity: *mut ANativeActivity, format: i32);

    pub fn ALooper_prepare(options: c_int) -> *mut ALooper;
    pub fn ALooper_forThread() -> *mut ALooper;
    pub fn ALooper_pollOnce(
        timeout: c_int,
        fd: *mut c_int,
        events: *mut c_int,
        data: *mut *mut c_void,
    ) -> c_int;
    pub fn ALooper_addFd(
        looper: *mut ALooper,
        fd: c_int,
        ident: c_int,
        events: c_int,
        callback: ALooper_callbackFunc,
        data: *mut c_void,
    ) -> c_int;

    pub fn AInputQueue_attachLooper(
        queue: *mut AInputQueue,
        looper: *mut ALooper,
        ident: c_int,
        callback: ALooper_callbackFunc,
        data: *mut c_void,
    );
    pub fn AInputQueue_detachLooper(queue: *mut AInputQueue);
    pub fn AInputQueue_getEvent(queue: *mut AInputQueue, event: *mut *mut AInputEvent) -> i32;
    pub fn AInputQueue_preDispatchEvent(queue: *mut AInputQueue, event: *mut AInputEvent) -> i32;
    pub fn AInputQueue_finishEvent(
        queue: *mut AInputQueue,
        event: *mut AInputEvent,
        handled: c_int,
    );

    pub fn AInputEvent_getType(event: *const AInputEvent) -> i32;
    pub fn AInputEvent_getDeviceId(event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getAction(event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getKeyCode(event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getRepeatCount(event: *const AInputEvent) -> i32;
    pub fn AKeyEvent_getMetaState(event: *const AInputEvent) -> i32;
    pub fn AMotionEvent_getAction(event: *const AInputEvent) -> i32;
    pub fn AMotionEvent_getToolType(event: *const AInputEvent, pointer: usize) -> i32;
    pub fn AMotionEvent_getX(event: *const AInputEvent, pointer: usize) -> f32;
    pub fn AMotionEvent_getY(event: *const AInputEvent, pointer: usize) -> f32;
    pub fn AMotionEvent_getAxisValue(event: *const AInputEvent, axis: i32, pointer: usize) -> f32;

    pub fn ANativeWindow_getWidth(window: *mut ANativeWindow) -> i32;
    pub fn ANativeWindow_getHeight(window: *mut ANativeWindow) -> i32;
    pub fn ANativeWindow_toSurface(env: *mut JNIEnv, window: *mut ANativeWindow) -> jobject;

    pub fn AChoreographer_getInstance() -> *mut AChoreographer;
    pub fn AChoreographer_postFrameCallback(
        choreographer: *mut AChoreographer,
        callback: AChoreographer_frameCallback,
        data: *mut c_void,
    );
}

// MARK: Logging and libc
pub const ANDROID_LOG_INFO: c_int = 4;
pub const ANDROID_LOG_ERROR: c_int = 6;
pub const EFD_CLOEXEC: c_int = 0o2000000;

#[link(name = "log")]
unsafe extern "C" {
    pub fn __android_log_write(priority: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

#[repr(C)]
pub struct Dl_info {
    pub dli_fname: *const c_char,
    pub dli_fbase: *mut c_void,
    pub dli_sname: *const c_char,
    pub dli_saddr: *mut c_void,
}

impl Default for Dl_info {
    fn default() -> Self {
        Self {
            dli_fname: std::ptr::null(),
            dli_fbase: std::ptr::null_mut(),
            dli_sname: std::ptr::null(),
            dli_saddr: std::ptr::null_mut(),
        }
    }
}

unsafe extern "C" {
    pub fn dladdr(address: *const c_void, info: *mut Dl_info) -> c_int;
    pub fn pipe(fds: *mut c_int) -> c_int;
    pub fn dup2(old_fd: c_int, new_fd: c_int) -> c_int;
    pub fn eventfd(initial: u32, flags: c_int) -> c_int;
    pub fn read(fd: c_int, buffer: *mut c_void, count: usize) -> isize;
    pub fn write(fd: c_int, buffer: *const c_void, count: usize) -> isize;
}
