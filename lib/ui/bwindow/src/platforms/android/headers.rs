/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    reason = "Raw JNI and NDK declarations"
)]

use std::ffi::{c_char, c_int, c_long, c_void};
use std::mem::{offset_of, size_of};

// MARK: JNI types
pub type jboolean = u8;
pub type jbyte = i8;
pub type jchar = u16;
pub type jint = i32;
pub type jlong = i64;
pub type jfloat = f32;
pub type jdouble = f64;
pub type jsize = jint;
pub type jobject = *mut c_void;
pub type jclass = jobject;
pub type jstring = jobject;
pub type jarray = jobject;
pub type jbyteArray = jarray;
pub type jfloatArray = jarray;
pub type jobjectArray = jarray;
pub type jfieldID = *mut c_void;
pub type jmethodID = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy)]
pub union jvalue {
    pub z: jboolean,
    pub b: jbyte,
    pub c: jchar,
    pub i: jint,
    pub j: jlong,
    pub f: jfloat,
    pub d: jdouble,
    pub l: jobject,
}

#[repr(C)]
pub struct JNINativeMethod {
    pub name: *const c_char,
    pub signature: *const c_char,
    pub fnPtr: *mut c_void,
}

pub const JNI_OK: jint = 0;
pub const JNI_VERSION_1_6: jint = 0x0001_0006;

pub type JNIEnv = *const JNINativeInterface;
pub type JavaVM = *const JNIInvokeInterface;

type Reserved = *const c_void;

// MARK: JNIEnv
/// The JNI function table, unused slots are kept as padding to preserve the C layout.
#[repr(C)]
pub struct JNINativeInterface {
    _reserved0: [Reserved; 6],
    pub FindClass: unsafe extern "C" fn(*mut JNIEnv, *const c_char) -> jclass,
    _reserved7: [Reserved; 9],
    pub ExceptionDescribe: unsafe extern "C" fn(*mut JNIEnv),
    pub ExceptionClear: unsafe extern "C" fn(*mut JNIEnv),
    _reserved18: Reserved,
    pub PushLocalFrame: unsafe extern "C" fn(*mut JNIEnv, jint) -> jint,
    pub PopLocalFrame: unsafe extern "C" fn(*mut JNIEnv, jobject) -> jobject,
    pub NewGlobalRef: unsafe extern "C" fn(*mut JNIEnv, jobject) -> jobject,
    pub DeleteGlobalRef: unsafe extern "C" fn(*mut JNIEnv, jobject),
    pub DeleteLocalRef: unsafe extern "C" fn(*mut JNIEnv, jobject),
    _reserved24: [Reserved; 6],
    pub NewObjectA: unsafe extern "C" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jobject,
    pub GetObjectClass: unsafe extern "C" fn(*mut JNIEnv, jobject) -> jclass,
    _reserved32: Reserved,
    pub GetMethodID:
        unsafe extern "C" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jmethodID,
    _reserved34: [Reserved; 2],
    pub CallObjectMethodA:
        unsafe extern "C" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jobject,
    _reserved37: [Reserved; 2],
    pub CallBooleanMethodA:
        unsafe extern "C" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jboolean,
    _reserved40: [Reserved; 11],
    pub CallIntMethodA:
        unsafe extern "C" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jint,
    _reserved52: [Reserved; 5],
    pub CallFloatMethodA:
        unsafe extern "C" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jfloat,
    _reserved58: [Reserved; 5],
    pub CallVoidMethodA: unsafe extern "C" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue),
    _reserved64: [Reserved; 30],
    pub GetFieldID:
        unsafe extern "C" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jfieldID,
    _reserved95: [Reserved; 5],
    pub GetIntField: unsafe extern "C" fn(*mut JNIEnv, jobject, jfieldID) -> jint,
    _reserved101: [Reserved; 12],
    pub GetStaticMethodID:
        unsafe extern "C" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jmethodID,
    _reserved114: [Reserved; 2],
    pub CallStaticObjectMethodA:
        unsafe extern "C" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jobject,
    _reserved117: [Reserved; 14],
    pub CallStaticIntMethodA:
        unsafe extern "C" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jint,
    _reserved132: [Reserved; 5],
    pub CallStaticFloatMethodA:
        unsafe extern "C" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jfloat,
    _reserved138: [Reserved; 5],
    pub CallStaticVoidMethodA: unsafe extern "C" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue),
    pub GetStaticFieldID:
        unsafe extern "C" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jfieldID,
    pub GetStaticObjectField: unsafe extern "C" fn(*mut JNIEnv, jclass, jfieldID) -> jobject,
    _reserved146: [Reserved; 17],
    pub NewString: unsafe extern "C" fn(*mut JNIEnv, *const jchar, jsize) -> jstring,
    pub GetStringLength: unsafe extern "C" fn(*mut JNIEnv, jstring) -> jsize,
    _reserved165: [Reserved; 6],
    pub GetArrayLength: unsafe extern "C" fn(*mut JNIEnv, jarray) -> jsize,
    pub NewObjectArray: unsafe extern "C" fn(*mut JNIEnv, jsize, jclass, jobject) -> jobjectArray,
    pub GetObjectArrayElement: unsafe extern "C" fn(*mut JNIEnv, jobjectArray, jsize) -> jobject,
    pub SetObjectArrayElement: unsafe extern "C" fn(*mut JNIEnv, jobjectArray, jsize, jobject),
    _reserved175: Reserved,
    pub NewByteArray: unsafe extern "C" fn(*mut JNIEnv, jsize) -> jbyteArray,
    _reserved177: [Reserved; 4],
    pub NewFloatArray: unsafe extern "C" fn(*mut JNIEnv, jsize) -> jfloatArray,
    _reserved182: [Reserved; 18],
    pub GetByteArrayRegion: unsafe extern "C" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *mut jbyte),
    _reserved201: [Reserved; 7],
    pub SetByteArrayRegion:
        unsafe extern "C" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *const jbyte),
    _reserved209: [Reserved; 4],
    pub SetFloatArrayRegion:
        unsafe extern "C" fn(*mut JNIEnv, jfloatArray, jsize, jsize, *const jfloat),
    _reserved214: Reserved,
    pub RegisterNatives:
        unsafe extern "C" fn(*mut JNIEnv, jclass, *const JNINativeMethod, jint) -> jint,
    _reserved216: [Reserved; 4],
    pub GetStringRegion: unsafe extern "C" fn(*mut JNIEnv, jstring, jsize, jsize, *mut jchar),
    _reserved221: [Reserved; 7],
    pub ExceptionCheck: unsafe extern "C" fn(*mut JNIEnv) -> jboolean,
    pub NewDirectByteBuffer: unsafe extern "C" fn(*mut JNIEnv, *mut c_void, jlong) -> jobject,
}

// MARK: JavaVM
#[repr(C)]
pub struct JNIInvokeInterface {
    _reserved0: [Reserved; 4],
    pub AttachCurrentThread:
        unsafe extern "C" fn(*mut JavaVM, *mut *mut JNIEnv, *mut c_void) -> jint,
    pub DetachCurrentThread: unsafe extern "C" fn(*mut JavaVM) -> jint,
    pub GetEnv: unsafe extern "C" fn(*mut JavaVM, *mut *mut c_void, jint) -> jint,
}

// Verify every used slot against the function indices of the NDK jni.h.
macro_rules! assert_slots {
    ($table:ty { $($field:ident = $index:literal),* $(,)? }) => {
        $(const _: () = assert!(offset_of!($table, $field) == $index * size_of::<Reserved>());)*
    };
}

assert_slots!(JNINativeInterface {
    FindClass = 6,
    ExceptionDescribe = 16,
    ExceptionClear = 17,
    PushLocalFrame = 19,
    PopLocalFrame = 20,
    NewGlobalRef = 21,
    DeleteGlobalRef = 22,
    DeleteLocalRef = 23,
    NewObjectA = 30,
    GetObjectClass = 31,
    GetMethodID = 33,
    CallObjectMethodA = 36,
    CallBooleanMethodA = 39,
    CallIntMethodA = 51,
    CallFloatMethodA = 57,
    CallVoidMethodA = 63,
    GetFieldID = 94,
    GetIntField = 100,
    GetStaticMethodID = 113,
    CallStaticObjectMethodA = 116,
    CallStaticIntMethodA = 131,
    CallStaticFloatMethodA = 137,
    CallStaticVoidMethodA = 143,
    GetStaticFieldID = 144,
    GetStaticObjectField = 145,
    NewString = 163,
    GetStringLength = 164,
    GetArrayLength = 171,
    NewObjectArray = 172,
    GetObjectArrayElement = 173,
    SetObjectArrayElement = 174,
    NewByteArray = 176,
    NewFloatArray = 181,
    GetByteArrayRegion = 200,
    SetByteArrayRegion = 208,
    SetFloatArrayRegion = 213,
    RegisterNatives = 215,
    GetStringRegion = 220,
    ExceptionCheck = 228,
    NewDirectByteBuffer = 229,
});
assert_slots!(JNIInvokeInterface {
    AttachCurrentThread = 4,
    DetachCurrentThread = 5,
    GetEnv = 6,
});

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
