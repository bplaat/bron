/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, c_void};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

use super::headers::*;

static JAVA_VM: AtomicPtr<JavaVM> = AtomicPtr::new(null_mut());
static APP_CLASS_LOADER: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

// Initialize from ANativeActivity_onCreate with the activity of the application.
pub(super) unsafe fn init(vm: *mut JavaVM, env: JniEnv, activity: jobject) {
    JAVA_VM.store(vm, Ordering::Release);
    unsafe {
        let class = env.object_class(activity);
        let get_class_loader = env.method(class, c"getClassLoader", c"()Ljava/lang/ClassLoader;");
        let loader = env.call_object(activity, get_class_loader, &[]);
        APP_CLASS_LOADER.store(env.new_global_ref(loader), Ordering::Release);
        env.delete_local_ref(loader);
        env.delete_local_ref(class);
    }
}

// MARK: JniEnv
/// The JNI environment of the current thread. Every call checks for and
/// panics on a pending Java exception after logging it.
#[derive(Clone, Copy)]
pub struct JniEnv(*mut JNIEnv);

struct AttachGuard;

impl Drop for AttachGuard {
    fn drop(&mut self) {
        let vm = JAVA_VM.load(Ordering::Acquire);
        unsafe { ((**vm).DetachCurrentThread)(vm) };
    }
}

thread_local! {
    #[allow(clippy::missing_const_for_thread_local, reason = "false positive without native TLS")]
    static ATTACHED: AttachGuard = const { AttachGuard };
}

impl JniEnv {
    /// Wrap a JNI environment received by a native method.
    ///
    /// # Safety
    /// `env` must be the valid JNI environment of the current thread.
    pub const unsafe fn from_raw(env: *mut JNIEnv) -> Self {
        Self(env)
    }

    /// Get the environment of the current thread, attaching the thread to the JVM if needed.
    pub fn current() -> Self {
        let vm = JAVA_VM.load(Ordering::Acquire);
        assert!(!vm.is_null(), "the activity has not started");
        let mut env = null_mut();
        unsafe {
            if ((**vm).GetEnv)(vm, &mut env, JNI_VERSION_1_6) != JNI_OK {
                let result = ((**vm).AttachCurrentThread)(vm, (&raw mut env).cast(), null_mut());
                assert_eq!(result, JNI_OK, "can't attach thread to the Java VM");
                // Detach again when the thread exits.
                ATTACHED.with(|_| {});
            }
        }
        Self(env.cast())
    }

    /// The raw JNI environment pointer.
    pub const fn as_raw(self) -> *mut JNIEnv {
        self.0
    }

    fn check_exception(self) {
        unsafe {
            if ((**self.0).ExceptionCheck)(self.0) != 0 {
                ((**self.0).ExceptionDescribe)(self.0);
                ((**self.0).ExceptionClear)(self.0);
                panic!("unexpected Java exception");
            }
        }
    }

    /// Find a framework class by its JNI name, like `android/graphics/Canvas`.
    pub fn find_class(self, name: &CStr) -> jclass {
        let class = unsafe { ((**self.0).FindClass)(self.0, name.as_ptr()) };
        self.check_exception();
        class
    }

    /// Run `f` in a new local reference frame, so its local references are freed afterwards.
    pub fn with_local_frame<R>(self, f: impl FnOnce() -> R) -> R {
        unsafe {
            assert_eq!(((**self.0).PushLocalFrame)(self.0, 16), JNI_OK);
            let result = f();
            ((**self.0).PopLocalFrame)(self.0, null_mut());
            result
        }
    }

    /// Get the class of an object.
    ///
    /// # Safety
    /// `object` must be a valid reference.
    pub unsafe fn object_class(self, object: jobject) -> jclass {
        unsafe { ((**self.0).GetObjectClass)(self.0, object) }
    }

    /// Look up an instance method.
    ///
    /// # Safety
    /// `class` must be a valid class reference.
    pub unsafe fn method(self, class: jclass, name: &CStr, signature: &CStr) -> jmethodID {
        let method =
            unsafe { ((**self.0).GetMethodID)(self.0, class, name.as_ptr(), signature.as_ptr()) };
        self.check_exception();
        method
    }

    /// Look up a static method.
    ///
    /// # Safety
    /// `class` must be a valid class reference.
    pub unsafe fn static_method(self, class: jclass, name: &CStr, signature: &CStr) -> jmethodID {
        let method = unsafe {
            ((**self.0).GetStaticMethodID)(self.0, class, name.as_ptr(), signature.as_ptr())
        };
        self.check_exception();
        method
    }

    /// Read an int instance field.
    ///
    /// # Safety
    /// `object` must be a valid reference with an int field `name`.
    pub unsafe fn int_field(self, object: jobject, name: &CStr) -> jint {
        unsafe {
            let class = self.object_class(object);
            let field = ((**self.0).GetFieldID)(self.0, class, name.as_ptr(), c"I".as_ptr());
            self.check_exception();
            self.delete_local_ref(class);
            ((**self.0).GetIntField)(self.0, object, field)
        }
    }

    /// Read an object static field, like an enum constant.
    ///
    /// # Safety
    /// `class` must be a valid class reference with a static field `name` of type `signature`.
    pub unsafe fn static_object_field(
        self,
        class: jclass,
        name: &CStr,
        signature: &CStr,
    ) -> jobject {
        unsafe {
            let field =
                ((**self.0).GetStaticFieldID)(self.0, class, name.as_ptr(), signature.as_ptr());
            self.check_exception();
            ((**self.0).GetStaticObjectField)(self.0, class, field)
        }
    }

    /// Construct a new object.
    ///
    /// # Safety
    /// `constructor` must be a constructor of `class` that matches `args`.
    pub unsafe fn new_object(
        self,
        class: jclass,
        constructor: jmethodID,
        args: &[jvalue],
    ) -> jobject {
        let object = unsafe { ((**self.0).NewObjectA)(self.0, class, constructor, args.as_ptr()) };
        self.check_exception();
        object
    }

    /// Call a void instance method.
    ///
    /// # Safety
    /// `method` must be a void method of `object` that matches `args`.
    pub unsafe fn call_void(self, object: jobject, method: jmethodID, args: &[jvalue]) {
        unsafe { ((**self.0).CallVoidMethodA)(self.0, object, method, args.as_ptr()) };
        self.check_exception();
    }

    /// Look up and call a void instance method by name.
    ///
    /// # Safety
    /// `object` must be a valid reference with a void method `name` that matches `args`.
    pub unsafe fn call_void_method(
        self,
        object: jobject,
        name: &CStr,
        signature: &CStr,
        args: &[jvalue],
    ) {
        unsafe {
            let class = self.object_class(object);
            let method = self.method(class, name, signature);
            self.call_void(object, method, args);
            self.delete_local_ref(class);
        }
    }

    /// Look up and call an object returning instance method by name.
    ///
    /// # Safety
    /// `object` must be a valid reference with an object method `name` that matches `args`.
    pub unsafe fn call_object_method(
        self,
        object: jobject,
        name: &CStr,
        signature: &CStr,
        args: &[jvalue],
    ) -> jobject {
        unsafe {
            let class = self.object_class(object);
            let method = self.method(class, name, signature);
            let result = self.call_object(object, method, args);
            self.delete_local_ref(class);
            result
        }
    }

    /// Call an object returning instance method.
    ///
    /// # Safety
    /// `method` must be an object returning method of `object` that matches `args`.
    pub unsafe fn call_object(
        self,
        object: jobject,
        method: jmethodID,
        args: &[jvalue],
    ) -> jobject {
        let result =
            unsafe { ((**self.0).CallObjectMethodA)(self.0, object, method, args.as_ptr()) };
        self.check_exception();
        result
    }

    /// Call a boolean returning instance method.
    ///
    /// # Safety
    /// `method` must be a boolean returning method of `object` that matches `args`.
    pub unsafe fn call_bool(self, object: jobject, method: jmethodID, args: &[jvalue]) -> bool {
        let result =
            unsafe { ((**self.0).CallBooleanMethodA)(self.0, object, method, args.as_ptr()) };
        self.check_exception();
        result != 0
    }

    /// Call an int returning instance method.
    ///
    /// # Safety
    /// `method` must be an int returning method of `object` that matches `args`.
    pub unsafe fn call_int(self, object: jobject, method: jmethodID, args: &[jvalue]) -> jint {
        let result = unsafe { ((**self.0).CallIntMethodA)(self.0, object, method, args.as_ptr()) };
        self.check_exception();
        result
    }

    /// Call a float returning instance method.
    ///
    /// # Safety
    /// `method` must be a float returning method of `object` that matches `args`.
    pub unsafe fn call_float(self, object: jobject, method: jmethodID, args: &[jvalue]) -> f32 {
        let result =
            unsafe { ((**self.0).CallFloatMethodA)(self.0, object, method, args.as_ptr()) };
        self.check_exception();
        result
    }

    /// Call a void static method.
    ///
    /// # Safety
    /// `method` must be a static void method of `class` that matches `args`.
    pub unsafe fn call_static_void(self, class: jclass, method: jmethodID, args: &[jvalue]) {
        unsafe { ((**self.0).CallStaticVoidMethodA)(self.0, class, method, args.as_ptr()) };
        self.check_exception();
    }

    /// Call an object returning static method.
    ///
    /// # Safety
    /// `method` must be a static object returning method of `class` that matches `args`.
    pub unsafe fn call_static_object(
        self,
        class: jclass,
        method: jmethodID,
        args: &[jvalue],
    ) -> jobject {
        let result =
            unsafe { ((**self.0).CallStaticObjectMethodA)(self.0, class, method, args.as_ptr()) };
        self.check_exception();
        result
    }

    /// Call an int returning static method.
    ///
    /// # Safety
    /// `method` must be a static int returning method of `class` that matches `args`.
    pub unsafe fn call_static_int(self, class: jclass, method: jmethodID, args: &[jvalue]) -> jint {
        let result =
            unsafe { ((**self.0).CallStaticIntMethodA)(self.0, class, method, args.as_ptr()) };
        self.check_exception();
        result
    }

    /// Create a global reference that stays valid across threads and native calls.
    ///
    /// # Safety
    /// `object` must be a valid reference.
    pub unsafe fn new_global_ref(self, object: jobject) -> jobject {
        unsafe { ((**self.0).NewGlobalRef)(self.0, object) }
    }

    /// Delete a local reference.
    ///
    /// # Safety
    /// `object` must be a local reference of this thread or null.
    pub unsafe fn delete_local_ref(self, object: jobject) {
        if !object.is_null() {
            unsafe { ((**self.0).DeleteLocalRef)(self.0, object) }
        }
    }

    /// Create a Java string from UTF-8 text.
    pub fn new_string(self, value: &str) -> jstring {
        // UTF-16 never has more units than UTF-8 has bytes, so short text is encoded on the stack.
        let mut buffer = [0u16; 256];
        let heap;
        let chars = if value.len() <= buffer.len() {
            let mut length = 0;
            for unit in value.encode_utf16() {
                buffer[length] = unit;
                length += 1;
            }
            &buffer[..length]
        } else {
            heap = value.encode_utf16().collect::<Vec<_>>();
            &heap[..]
        };
        let string =
            unsafe { ((**self.0).NewString)(self.0, chars.as_ptr(), chars.len() as jsize) };
        self.check_exception();
        string
    }

    /// Read a Java string, null reads as an empty string.
    ///
    /// # Safety
    /// `string` must be a valid `java.lang.String` reference or null.
    pub unsafe fn string(self, string: jstring) -> String {
        if string.is_null() {
            return String::new();
        }
        unsafe {
            let length = ((**self.0).GetStringLength)(self.0, string);
            let mut chars = vec![0; length as usize];
            ((**self.0).GetStringRegion)(self.0, string, 0, length, chars.as_mut_ptr());
            String::from_utf16_lossy(&chars)
        }
    }

    /// Create a Java string array.
    pub fn new_string_array(self, values: &[&str]) -> jobjectArray {
        unsafe {
            let class = self.find_class(c"java/lang/String");
            let array =
                ((**self.0).NewObjectArray)(self.0, values.len() as jsize, class, null_mut());
            self.check_exception();
            for (index, value) in values.iter().enumerate() {
                let string = self.new_string(value);
                ((**self.0).SetObjectArrayElement)(self.0, array, index as jsize, string);
                self.delete_local_ref(string);
            }
            self.delete_local_ref(class);
            array
        }
    }

    /// Read a Java string array.
    ///
    /// # Safety
    /// `array` must be a valid `java.lang.String[]` reference.
    pub unsafe fn string_array(self, array: jobjectArray) -> Vec<String> {
        unsafe {
            let length = ((**self.0).GetArrayLength)(self.0, array);
            (0..length)
                .map(|index| {
                    let string = ((**self.0).GetObjectArrayElement)(self.0, array, index);
                    let value = self.string(string);
                    self.delete_local_ref(string);
                    value
                })
                .collect()
        }
    }

    /// Create a Java byte array.
    pub fn new_byte_array(self, bytes: &[u8]) -> jbyteArray {
        unsafe {
            let array = ((**self.0).NewByteArray)(self.0, bytes.len() as jsize);
            self.check_exception();
            ((**self.0).SetByteArrayRegion)(
                self.0,
                array,
                0,
                bytes.len() as jsize,
                bytes.as_ptr().cast(),
            );
            array
        }
    }

    /// Read a Java byte array.
    ///
    /// # Safety
    /// `array` must be a valid `byte[]` reference.
    pub unsafe fn byte_array(self, array: jbyteArray) -> Vec<u8> {
        unsafe {
            let length = ((**self.0).GetArrayLength)(self.0, array);
            let mut bytes = vec![0u8; length as usize];
            ((**self.0).GetByteArrayRegion)(self.0, array, 0, length, bytes.as_mut_ptr().cast());
            bytes
        }
    }

    /// Create a Java float array.
    pub fn new_float_array(self, length: usize) -> jfloatArray {
        let array = unsafe { ((**self.0).NewFloatArray)(self.0, length as jsize) };
        self.check_exception();
        array
    }

    /// Copy floats into the start of a Java float array.
    ///
    /// # Safety
    /// `array` must be a valid `float[]` reference with room for `values`.
    pub unsafe fn set_float_array(self, array: jfloatArray, values: &[f32]) {
        unsafe {
            ((**self.0).SetFloatArrayRegion)(
                self.0,
                array,
                0,
                values.len() as jsize,
                values.as_ptr(),
            );
        }
        self.check_exception();
    }

    /// Wrap native memory in a direct `java.nio.ByteBuffer` without copying.
    ///
    /// # Safety
    /// The memory must stay valid and unmoved while Java can access the buffer.
    pub unsafe fn new_direct_byte_buffer(self, data: *mut u8, length: usize) -> jobject {
        let buffer =
            unsafe { ((**self.0).NewDirectByteBuffer)(self.0, data.cast(), length as jlong) };
        self.check_exception();
        buffer
    }

    /// Register native methods of a class.
    ///
    /// # Safety
    /// `class` must be a valid class reference and every function pointer must match its
    /// JNI signature.
    pub unsafe fn register_natives(self, class: jclass, methods: &[JNINativeMethod]) {
        let result = unsafe {
            ((**self.0).RegisterNatives)(self.0, class, methods.as_ptr(), methods.len() as jint)
        };
        self.check_exception();
        assert_eq!(result, JNI_OK, "can't register native methods");
    }
}

/// Describe a native method for [`JniEnv::register_natives`].
pub const fn native_method(
    name: &'static CStr,
    signature: &'static CStr,
    function: *mut c_void,
) -> JNINativeMethod {
    JNINativeMethod {
        name: name.as_ptr(),
        signature: signature.as_ptr(),
        fnPtr: function,
    }
}

// MARK: GlobalRef
/// An owned JNI global reference that can be used from any thread.
pub struct GlobalRef(jobject);

// SAFETY: JNI global references are valid on every attached thread.
unsafe impl Send for GlobalRef {}
// SAFETY: See above, the reference itself is immutable.
unsafe impl Sync for GlobalRef {}

impl GlobalRef {
    /// Create a global reference to `object`.
    ///
    /// # Safety
    /// `object` must be a valid reference.
    pub unsafe fn new(env: JniEnv, object: jobject) -> Self {
        Self(unsafe { env.new_global_ref(object) })
    }

    /// Create a global reference to a local reference and delete the local one.
    ///
    /// # Safety
    /// `object` must be a valid local reference of this thread.
    pub unsafe fn from_local(env: JniEnv, object: jobject) -> Self {
        unsafe {
            let global = Self::new(env, object);
            env.delete_local_ref(object);
            global
        }
    }

    /// The raw reference, valid while this value lives.
    pub const fn as_raw(&self) -> jobject {
        self.0
    }
}

impl Drop for GlobalRef {
    fn drop(&mut self) {
        let env = JniEnv::current();
        unsafe { ((**env.0).DeleteGlobalRef)(env.0, self.0) };
    }
}

// MARK: DexClassLoader
/// Loads Java classes from dex code embedded in a crate, so crates can ship small Java
/// helpers while the APK itself has no code. Native methods of those classes must be
/// registered with [`JniEnv::register_natives`].
pub struct DexClassLoader {
    loader: GlobalRef,
    load_class: jmethodID,
}

// SAFETY: The loader is a global reference and method IDs are valid on every thread.
unsafe impl Send for DexClassLoader {}
// SAFETY: See above.
unsafe impl Sync for DexClassLoader {}

impl DexClassLoader {
    /// Load dex code with the application class loader as parent.
    pub fn new(dex: &'static [u8]) -> Self {
        let env = JniEnv::current();
        unsafe {
            let buffer = env.new_direct_byte_buffer(dex.as_ptr().cast_mut(), dex.len());
            let class = env.find_class(c"dalvik/system/InMemoryDexClassLoader");
            let constructor = env.method(
                class,
                c"<init>",
                c"(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V",
            );
            let loader = env.new_object(
                class,
                constructor,
                &[
                    jvalue { l: buffer },
                    jvalue {
                        l: APP_CLASS_LOADER.load(Ordering::Acquire),
                    },
                ],
            );
            let load_class = env.method(
                class,
                c"loadClass",
                c"(Ljava/lang/String;)Ljava/lang/Class;",
            );
            env.delete_local_ref(buffer);
            env.delete_local_ref(class);
            Self {
                loader: GlobalRef::from_local(env, loader),
                load_class,
            }
        }
    }

    /// Find a class of the dex code by its binary name, like `nl.bplaat.bwebview.Bwebview`.
    pub fn find_class(&self, env: JniEnv, name: &str) -> jclass {
        unsafe {
            let name = env.new_string(name);
            let class =
                env.call_object(self.loader.as_raw(), self.load_class, &[jvalue { l: name }]);
            env.delete_local_ref(name);
            class
        }
    }
}
