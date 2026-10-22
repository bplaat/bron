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
pub(crate) struct ANativeActivity {
    pub(crate) callbacks: *mut ANativeActivityCallbacks,
    pub(crate) vm: *mut JavaVM,
    pub(crate) env: *mut JNIEnv,
    pub(crate) clazz: jobject,
    pub(crate) internalDataPath: *const c_char,
    pub(crate) externalDataPath: *const c_char,
    pub(crate) sdkVersion: i32,
    pub(crate) instance: *mut c_void,
    pub(crate) assetManager: *mut c_void,
    pub(crate) obbPath: *const c_char,
}

type ActivityCallback = Option<unsafe extern "C" fn(*mut ANativeActivity)>;
type WindowCallback = Option<unsafe extern "C" fn(*mut ANativeActivity, *mut ANativeWindow)>;
type QueueCallback = Option<unsafe extern "C" fn(*mut ANativeActivity, *mut AInputQueue)>;

#[repr(C)]
pub(crate) struct ANativeActivityCallbacks {
    pub(crate) onStart: ActivityCallback,
    pub(crate) onResume: ActivityCallback,
    pub(crate) onSaveInstanceState:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *mut usize) -> *mut c_void>,
    pub(crate) onPause: ActivityCallback,
    pub(crate) onStop: ActivityCallback,
    pub(crate) onDestroy: ActivityCallback,
    pub(crate) onWindowFocusChanged: Option<unsafe extern "C" fn(*mut ANativeActivity, c_int)>,
    pub(crate) onNativeWindowCreated: WindowCallback,
    pub(crate) onNativeWindowResized: WindowCallback,
    pub(crate) onNativeWindowRedrawNeeded: WindowCallback,
    pub(crate) onNativeWindowDestroyed: WindowCallback,
    pub(crate) onInputQueueCreated: QueueCallback,
    pub(crate) onInputQueueDestroyed: QueueCallback,
    pub(crate) onContentRectChanged:
        Option<unsafe extern "C" fn(*mut ANativeActivity, *const ARect)>,
    pub(crate) onConfigurationChanged: ActivityCallback,
    pub(crate) onLowMemory: ActivityCallback,
}

#[repr(C)]
pub(crate) struct ARect {
    pub(crate) left: i32,
    pub(crate) top: i32,
    pub(crate) right: i32,
    pub(crate) bottom: i32,
}

#[repr(C)]
pub(crate) struct ANativeWindow {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AInputQueue {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AInputEvent {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct ALooper {
    _private: [u8; 0],
}

#[repr(C)]
pub(crate) struct AChoreographer {
    _private: [u8; 0],
}

pub(crate) type ALooper_callbackFunc = unsafe extern "C" fn(c_int, c_int, *mut c_void) -> c_int;
pub(crate) type AChoreographer_frameCallback = unsafe extern "C" fn(c_long, *mut c_void);

pub(crate) const ALOOPER_PREPARE_ALLOW_NON_CALLBACKS: c_int = 1;
pub(crate) const ALOOPER_POLL_CALLBACK: c_int = -2;
pub(crate) const ALOOPER_EVENT_INPUT: c_int = 1;
pub(crate) const WINDOW_FORMAT_RGBX_8888: i32 = 2;

pub(crate) const AINPUT_EVENT_TYPE_KEY: i32 = 1;
pub(crate) const AINPUT_EVENT_TYPE_MOTION: i32 = 2;
pub(crate) const AKEY_EVENT_ACTION_DOWN: i32 = 0;
pub(crate) const AKEY_EVENT_ACTION_UP: i32 = 1;
pub(crate) const AKEYCODE_BACK: i32 = 4;
pub(crate) const AMOTION_EVENT_ACTION_MASK: i32 = 0xff;
pub(crate) const AMOTION_EVENT_AXIS_VSCROLL: i32 = 9;
pub(crate) const AMOTION_EVENT_AXIS_HSCROLL: i32 = 10;
pub(crate) const AMOTION_EVENT_TOOL_TYPE_MOUSE: i32 = 3;

#[link(name = "android")]
unsafe extern "C" {
    pub(crate) fn ANativeActivity_finish(activity: *mut ANativeActivity);
    pub(crate) fn ANativeActivity_setWindowFormat(activity: *mut ANativeActivity, format: i32);

    pub(crate) fn ALooper_prepare(options: c_int) -> *mut ALooper;
    pub(crate) fn ALooper_forThread() -> *mut ALooper;
    pub(crate) fn ALooper_pollOnce(
        timeout: c_int,
        fd: *mut c_int,
        events: *mut c_int,
        data: *mut *mut c_void,
    ) -> c_int;
    pub(crate) fn ALooper_addFd(
        looper: *mut ALooper,
        fd: c_int,
        ident: c_int,
        events: c_int,
        callback: ALooper_callbackFunc,
        data: *mut c_void,
    ) -> c_int;

    pub(crate) fn AInputQueue_attachLooper(
        queue: *mut AInputQueue,
        looper: *mut ALooper,
        ident: c_int,
        callback: ALooper_callbackFunc,
        data: *mut c_void,
    );
    pub(crate) fn AInputQueue_detachLooper(queue: *mut AInputQueue);
    pub(crate) fn AInputQueue_getEvent(
        queue: *mut AInputQueue,
        event: *mut *mut AInputEvent,
    ) -> i32;
    pub(crate) fn AInputQueue_preDispatchEvent(
        queue: *mut AInputQueue,
        event: *mut AInputEvent,
    ) -> i32;
    pub(crate) fn AInputQueue_finishEvent(
        queue: *mut AInputQueue,
        event: *mut AInputEvent,
        handled: c_int,
    );

    pub(crate) fn AInputEvent_getType(event: *const AInputEvent) -> i32;
    pub(crate) fn AInputEvent_getDeviceId(event: *const AInputEvent) -> i32;
    pub(crate) fn AKeyEvent_getAction(event: *const AInputEvent) -> i32;
    pub(crate) fn AKeyEvent_getKeyCode(event: *const AInputEvent) -> i32;
    pub(crate) fn AKeyEvent_getRepeatCount(event: *const AInputEvent) -> i32;
    pub(crate) fn AKeyEvent_getMetaState(event: *const AInputEvent) -> i32;
    pub(crate) fn AMotionEvent_getAction(event: *const AInputEvent) -> i32;
    pub(crate) fn AMotionEvent_getToolType(event: *const AInputEvent, pointer: usize) -> i32;
    pub(crate) fn AMotionEvent_getX(event: *const AInputEvent, pointer: usize) -> f32;
    pub(crate) fn AMotionEvent_getY(event: *const AInputEvent, pointer: usize) -> f32;
    pub(crate) fn AMotionEvent_getAxisValue(
        event: *const AInputEvent,
        axis: i32,
        pointer: usize,
    ) -> f32;

    pub(crate) fn ANativeWindow_getWidth(window: *mut ANativeWindow) -> i32;
    pub(crate) fn ANativeWindow_getHeight(window: *mut ANativeWindow) -> i32;
    pub(crate) fn ANativeWindow_toSurface(env: *mut JNIEnv, window: *mut ANativeWindow) -> jobject;

    pub(crate) fn AChoreographer_getInstance() -> *mut AChoreographer;
    pub(crate) fn AChoreographer_postFrameCallback(
        choreographer: *mut AChoreographer,
        callback: AChoreographer_frameCallback,
        data: *mut c_void,
    );
}

// MARK: Logging and libc
pub(crate) const ANDROID_LOG_INFO: c_int = 4;
pub(crate) const ANDROID_LOG_ERROR: c_int = 6;
pub(crate) const EFD_CLOEXEC: c_int = 0o2000000;

#[link(name = "log")]
unsafe extern "C" {
    pub(crate) fn __android_log_write(
        priority: c_int,
        tag: *const c_char,
        text: *const c_char,
    ) -> c_int;
}

#[repr(C)]
pub(crate) struct Dl_info {
    pub(crate) dli_fname: *const c_char,
    pub(crate) dli_fbase: *mut c_void,
    pub(crate) dli_sname: *const c_char,
    pub(crate) dli_saddr: *mut c_void,
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
    pub(crate) fn dladdr(address: *const c_void, info: *mut Dl_info) -> c_int;
    pub(crate) fn pipe(fds: *mut c_int) -> c_int;
    pub(crate) fn dup2(old_fd: c_int, new_fd: c_int) -> c_int;
    pub(crate) fn eventfd(initial: u32, flags: c_int) -> c_int;
    pub(crate) fn read(fd: c_int, buffer: *mut c_void, count: usize) -> isize;
    pub(crate) fn write(fd: c_int, buffer: *const c_void, count: usize) -> isize;
}
