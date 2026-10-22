/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Raw declarations of the JNI types and function tables from `jni.h`, named like the
//! [jni-sys](https://crates.io/crates/jni-sys) crate. Only the used table slots are callable,
//! the others are kept as padding to preserve the C layout.

#![allow(
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    reason = "Raw JNI declarations"
)]

use std::ffi::{c_char, c_void};
use std::mem::{offset_of, size_of};

// MARK: Types
pub type jboolean = bool;
pub type jbyte = i8;
pub type jchar = u16;
pub type jshort = i16;
pub type jint = i32;
pub type jlong = i64;
pub type jfloat = f32;
pub type jdouble = f64;
pub type jsize = jint;

pub enum _jobject {}
pub type jobject = *mut _jobject;
pub type jclass = jobject;
pub type jthrowable = jobject;
pub type jstring = jobject;
pub type jarray = jobject;
pub type jbyteArray = jarray;
pub type jfloatArray = jarray;
pub type jobjectArray = jarray;

pub enum _jfieldID {}
pub type jfieldID = *mut _jfieldID;
pub enum _jmethodID {}
pub type jmethodID = *mut _jmethodID;

#[repr(C)]
#[derive(Clone, Copy)]
pub union jvalue {
    pub z: jboolean,
    pub b: jbyte,
    pub c: jchar,
    pub s: jshort,
    pub i: jint,
    pub j: jlong,
    pub f: jfloat,
    pub d: jdouble,
    pub l: jobject,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct JNINativeMethod {
    pub name: *mut c_char,
    pub signature: *mut c_char,
    pub fnPtr: *mut c_void,
}

pub const JNI_OK: jint = 0;
pub const JNI_ERR: jint = -1;
pub const JNI_EDETACHED: jint = -2;
pub const JNI_EVERSION: jint = -3;
pub const JNI_ENOMEM: jint = -4;
pub const JNI_EEXIST: jint = -5;
pub const JNI_EINVAL: jint = -6;
pub const JNI_VERSION_1_6: jint = 0x0001_0006;

pub type JNIEnv = *const JNINativeInterface_;
pub type JavaVM = *const JNIInvokeInterface_;

type Reserved = *const c_void;

// MARK: JNIEnv
/// The JNI function table, `A` call variants take their arguments as a `jvalue` array.
#[repr(C)]
pub struct JNINativeInterface_ {
    _reserved0: [Reserved; 6],
    pub FindClass: unsafe extern "system" fn(*mut JNIEnv, *const c_char) -> jclass,
    _reserved7: [Reserved; 6],
    pub Throw: unsafe extern "system" fn(*mut JNIEnv, jthrowable) -> jint,
    pub ThrowNew: unsafe extern "system" fn(*mut JNIEnv, jclass, *const c_char) -> jint,
    pub ExceptionOccurred: unsafe extern "system" fn(*mut JNIEnv) -> jthrowable,
    pub ExceptionDescribe: unsafe extern "system" fn(*mut JNIEnv),
    pub ExceptionClear: unsafe extern "system" fn(*mut JNIEnv),
    _reserved18: Reserved,
    pub PushLocalFrame: unsafe extern "system" fn(*mut JNIEnv, jint) -> jint,
    pub PopLocalFrame: unsafe extern "system" fn(*mut JNIEnv, jobject) -> jobject,
    pub NewGlobalRef: unsafe extern "system" fn(*mut JNIEnv, jobject) -> jobject,
    pub DeleteGlobalRef: unsafe extern "system" fn(*mut JNIEnv, jobject),
    pub DeleteLocalRef: unsafe extern "system" fn(*mut JNIEnv, jobject),
    _reserved24: [Reserved; 6],
    pub NewObjectA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jobject,
    pub GetObjectClass: unsafe extern "system" fn(*mut JNIEnv, jobject) -> jclass,
    _reserved32: Reserved,
    pub GetMethodID:
        unsafe extern "system" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jmethodID,
    _reserved34: [Reserved; 2],
    pub CallObjectMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jobject,
    _reserved37: [Reserved; 2],
    pub CallBooleanMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jboolean,
    _reserved40: [Reserved; 2],
    pub CallByteMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jbyte,
    _reserved43: [Reserved; 2],
    pub CallCharMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jchar,
    _reserved46: [Reserved; 2],
    pub CallShortMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jshort,
    _reserved49: [Reserved; 2],
    pub CallIntMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jint,
    _reserved52: [Reserved; 2],
    pub CallLongMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jlong,
    _reserved55: [Reserved; 2],
    pub CallFloatMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jfloat,
    _reserved58: [Reserved; 2],
    pub CallDoubleMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue) -> jdouble,
    _reserved61: [Reserved; 2],
    pub CallVoidMethodA: unsafe extern "system" fn(*mut JNIEnv, jobject, jmethodID, *const jvalue),
    _reserved64: [Reserved; 30],
    pub GetFieldID:
        unsafe extern "system" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jfieldID,
    pub GetObjectField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jobject,
    pub GetBooleanField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jboolean,
    pub GetByteField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jbyte,
    pub GetCharField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jchar,
    pub GetShortField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jshort,
    pub GetIntField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jint,
    pub GetLongField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jlong,
    pub GetFloatField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jfloat,
    pub GetDoubleField: unsafe extern "system" fn(*mut JNIEnv, jobject, jfieldID) -> jdouble,
    _reserved104: [Reserved; 9],
    pub GetStaticMethodID:
        unsafe extern "system" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jmethodID,
    _reserved114: [Reserved; 2],
    pub CallStaticObjectMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jobject,
    _reserved117: [Reserved; 2],
    pub CallStaticBooleanMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jboolean,
    _reserved120: [Reserved; 2],
    pub CallStaticByteMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jbyte,
    _reserved123: [Reserved; 2],
    pub CallStaticCharMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jchar,
    _reserved126: [Reserved; 2],
    pub CallStaticShortMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jshort,
    _reserved129: [Reserved; 2],
    pub CallStaticIntMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jint,
    _reserved132: [Reserved; 2],
    pub CallStaticLongMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jlong,
    _reserved135: [Reserved; 2],
    pub CallStaticFloatMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jfloat,
    _reserved138: [Reserved; 2],
    pub CallStaticDoubleMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue) -> jdouble,
    _reserved141: [Reserved; 2],
    pub CallStaticVoidMethodA:
        unsafe extern "system" fn(*mut JNIEnv, jclass, jmethodID, *const jvalue),
    pub GetStaticFieldID:
        unsafe extern "system" fn(*mut JNIEnv, jclass, *const c_char, *const c_char) -> jfieldID,
    pub GetStaticObjectField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jobject,
    pub GetStaticBooleanField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jboolean,
    pub GetStaticByteField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jbyte,
    pub GetStaticCharField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jchar,
    pub GetStaticShortField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jshort,
    pub GetStaticIntField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jint,
    pub GetStaticLongField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jlong,
    pub GetStaticFloatField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jfloat,
    pub GetStaticDoubleField: unsafe extern "system" fn(*mut JNIEnv, jclass, jfieldID) -> jdouble,
    _reserved154: [Reserved; 9],
    pub NewString: unsafe extern "system" fn(*mut JNIEnv, *const jchar, jsize) -> jstring,
    pub GetStringLength: unsafe extern "system" fn(*mut JNIEnv, jstring) -> jsize,
    _reserved165: [Reserved; 6],
    pub GetArrayLength: unsafe extern "system" fn(*mut JNIEnv, jarray) -> jsize,
    pub NewObjectArray:
        unsafe extern "system" fn(*mut JNIEnv, jsize, jclass, jobject) -> jobjectArray,
    pub GetObjectArrayElement:
        unsafe extern "system" fn(*mut JNIEnv, jobjectArray, jsize) -> jobject,
    pub SetObjectArrayElement: unsafe extern "system" fn(*mut JNIEnv, jobjectArray, jsize, jobject),
    _reserved175: Reserved,
    pub NewByteArray: unsafe extern "system" fn(*mut JNIEnv, jsize) -> jbyteArray,
    _reserved177: [Reserved; 4],
    pub NewFloatArray: unsafe extern "system" fn(*mut JNIEnv, jsize) -> jfloatArray,
    _reserved182: [Reserved; 18],
    pub GetByteArrayRegion:
        unsafe extern "system" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *mut jbyte),
    _reserved201: [Reserved; 4],
    pub GetFloatArrayRegion:
        unsafe extern "system" fn(*mut JNIEnv, jfloatArray, jsize, jsize, *mut jfloat),
    _reserved206: [Reserved; 2],
    pub SetByteArrayRegion:
        unsafe extern "system" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *const jbyte),
    _reserved209: [Reserved; 4],
    pub SetFloatArrayRegion:
        unsafe extern "system" fn(*mut JNIEnv, jfloatArray, jsize, jsize, *const jfloat),
    _reserved214: Reserved,
    pub RegisterNatives:
        unsafe extern "system" fn(*mut JNIEnv, jclass, *const JNINativeMethod, jint) -> jint,
    _reserved216: [Reserved; 3],
    pub GetJavaVM: unsafe extern "system" fn(*mut JNIEnv, *mut *mut JavaVM) -> jint,
    pub GetStringRegion: unsafe extern "system" fn(*mut JNIEnv, jstring, jsize, jsize, *mut jchar),
    _reserved221: [Reserved; 7],
    pub ExceptionCheck: unsafe extern "system" fn(*mut JNIEnv) -> jboolean,
    pub NewDirectByteBuffer: unsafe extern "system" fn(*mut JNIEnv, *mut c_void, jlong) -> jobject,
}

// MARK: JavaVM
#[repr(C)]
pub struct JNIInvokeInterface_ {
    _reserved0: [Reserved; 4],
    pub AttachCurrentThread:
        unsafe extern "system" fn(*mut JavaVM, *mut *mut c_void, *mut c_void) -> jint,
    pub DetachCurrentThread: unsafe extern "system" fn(*mut JavaVM) -> jint,
    pub GetEnv: unsafe extern "system" fn(*mut JavaVM, *mut *mut c_void, jint) -> jint,
}

// Verify every used slot against the function indices of jni.h.
macro_rules! assert_slots {
    ($table:ty { $($field:ident = $index:literal),* $(,)? }) => {
        $(const _: () = assert!(offset_of!($table, $field) == $index * size_of::<Reserved>());)*
    };
}

assert_slots!(JNINativeInterface_ {
    FindClass = 6,
    Throw = 13,
    ThrowNew = 14,
    ExceptionOccurred = 15,
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
    CallByteMethodA = 42,
    CallCharMethodA = 45,
    CallShortMethodA = 48,
    CallIntMethodA = 51,
    CallLongMethodA = 54,
    CallFloatMethodA = 57,
    CallDoubleMethodA = 60,
    CallVoidMethodA = 63,
    GetFieldID = 94,
    GetObjectField = 95,
    GetDoubleField = 103,
    GetStaticMethodID = 113,
    CallStaticObjectMethodA = 116,
    CallStaticBooleanMethodA = 119,
    CallStaticByteMethodA = 122,
    CallStaticCharMethodA = 125,
    CallStaticShortMethodA = 128,
    CallStaticIntMethodA = 131,
    CallStaticLongMethodA = 134,
    CallStaticFloatMethodA = 137,
    CallStaticDoubleMethodA = 140,
    CallStaticVoidMethodA = 143,
    GetStaticFieldID = 144,
    GetStaticObjectField = 145,
    GetStaticDoubleField = 153,
    NewString = 163,
    GetStringLength = 164,
    GetArrayLength = 171,
    NewObjectArray = 172,
    GetObjectArrayElement = 173,
    SetObjectArrayElement = 174,
    NewByteArray = 176,
    NewFloatArray = 181,
    GetByteArrayRegion = 200,
    GetFloatArrayRegion = 205,
    SetByteArrayRegion = 208,
    SetFloatArrayRegion = 213,
    RegisterNatives = 215,
    GetJavaVM = 219,
    GetStringRegion = 220,
    ExceptionCheck = 228,
    NewDirectByteBuffer = 229,
});
assert_slots!(JNIInvokeInterface_ {
    AttachCurrentThread = 4,
    DetachCurrentThread = 5,
    GetEnv = 6,
});
