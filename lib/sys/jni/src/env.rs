/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::any::Any;
use std::ffi::{c_char, c_void};
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::null_mut;

use crate::descriptors::Desc;
use crate::errors::{Error, ErrorPolicy, JniError, Result};
use crate::objects::{
    Global, JByteArray, JByteBuffer, JClass, JFieldID, JFloatArray, JIntArray, JLongArray,
    JMethodID, JObject, JStaticFieldID, JStaticMethodID, JString, JThrowable, JValue, JValueOwned,
    Reference,
};
use crate::signature::{FieldSignature, JavaType, MethodSignature, Primitive, ReturnType};
use crate::strings::JNIStr;
use crate::sys::{self, JNI_OK, JNINativeInterface_, JNINativeMethod, jint, jlong, jsize, jvalue};
use crate::{jni_sig, jni_str};

// Call the JNI function variant that matches a Java type, `none` marks a missing void variant.
macro_rules! typed_call {
    ($env:ident, $ty:expr, ($($arg:expr),*), $object:ident, $bool:ident, $byte:ident,
     $char:ident, $short:ident, $int:ident, $long:ident, $float:ident, $double:ident,
     $void:tt) => {{
        let (table, raw) = ($env.table(), $env.get_raw());
        let value = unsafe {
            match $ty {
                JavaType::Object | JavaType::Array => {
                    JValueOwned::Object(JObject::from_raw($env, (table.$object)(raw, $($arg),*)))
                }
                JavaType::Primitive(Primitive::Boolean) => JValueOwned::Bool((table.$bool)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Byte) => JValueOwned::Byte((table.$byte)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Char) => JValueOwned::Char((table.$char)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Short) => JValueOwned::Short((table.$short)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Int) => JValueOwned::Int((table.$int)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Long) => JValueOwned::Long((table.$long)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Float) => JValueOwned::Float((table.$float)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Double) => JValueOwned::Double((table.$double)(raw, $($arg),*)),
                JavaType::Primitive(Primitive::Void) => typed_call!(@void table, raw, ($($arg),*), $void),
            }
        };
        $env.check_exception()?;
        Ok(value)
    }};
    (@void $table:ident, $raw:ident, ($($arg:expr),*), none) => {
        return Err(Error::WrongJValueType("field", "void"))
    };
    (@void $table:ident, $raw:ident, ($($arg:expr),*), $void:ident) => {{
        ($table.$void)($raw, $($arg),*);
        JValueOwned::Void
    }};
}

type MemberLookup<T> = unsafe extern "system" fn(
    *mut sys::JNIEnv,
    sys::jclass,
    *const c_char,
    *const c_char,
) -> *mut T;

// MARK: Env
/// The JNI environment of an attached thread, only lent out for one JNI stack frame by
/// [`crate::vm::JavaVM::attach_current_thread`] or [`EnvUnowned::with_env`]. Local references
/// created with it can't outlive that frame.
#[derive(Debug)]
pub struct Env<'local> {
    raw: *mut sys::JNIEnv,
    lifetime: PhantomData<&'local ()>,
}

impl<'local> Env<'local> {
    pub(crate) const unsafe fn from_raw(raw: *mut sys::JNIEnv) -> Self {
        Self {
            raw,
            lifetime: PhantomData,
        }
    }

    pub(crate) const fn table(&self) -> &JNINativeInterface_ {
        unsafe { &**self.raw }
    }

    pub(crate) fn check_exception(&self) -> Result<()> {
        if self.exception_check() {
            Err(Error::JavaException)
        } else {
            Ok(())
        }
    }

    // Log and clear a pending exception, describing it with its class name and message.
    pub(crate) fn catch_exception(&self) -> Option<Error> {
        if !self.exception_check() {
            return None;
        }
        let throwable = unsafe { (self.table().ExceptionOccurred)(self.raw) };
        self.exception_describe();
        self.exception_clear();
        let describe = self.with_local_frame(8, |env| -> Result<(String, String)> {
            let throwable = unsafe { JThrowable::from_raw(env, throwable) };
            let class = env.get_object_class(&throwable)?;
            let name = env
                .call_method(
                    &class,
                    jni_str!("getName"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let msg = env
                .call_method(
                    &throwable,
                    jni_str!("getMessage"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let string = |env: &Env<'_>, object: JObject<'_>| {
                unsafe { JString::from_raw(env, object.into_raw()) }
                    .try_to_string(env)
                    .unwrap_or_default()
            };
            Ok((string(env, name), string(env, msg)))
        });
        self.delete_local_ref(unsafe { JObject::from_raw(self, throwable) });
        self.exception_clear();
        let (name, msg) = describe.unwrap_or_default();
        Some(Error::CaughtJavaException { name, msg })
    }

    /// The raw environment pointer.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn get_raw(&self) -> *mut sys::JNIEnv {
        self.raw
    }

    /// Whether a Java exception is pending.
    pub fn exception_check(&self) -> bool {
        unsafe { (self.table().ExceptionCheck)(self.raw) }
    }

    /// Log the pending Java exception with its stack trace and clear it.
    pub fn exception_describe(&self) {
        unsafe { (self.table().ExceptionDescribe)(self.raw) }
    }

    /// Clear the pending Java exception.
    pub fn exception_clear(&self) {
        unsafe { (self.table().ExceptionClear)(self.raw) }
    }

    /// Throw a new exception of `class` with the message `msg`.
    pub fn throw_new<'other_local, S, C>(&mut self, class: C, msg: S) -> Result<()>
    where
        S: AsRef<JNIStr>,
        C: Desc<'local, JClass<'other_local>>,
    {
        let class = class.lookup(self)?;
        let class = non_null(class.as_ref().as_raw(), "throw_new class")?;
        let result = unsafe { (self.table().ThrowNew)(self.raw, class, msg.as_ref().as_ptr()) };
        if result == JNI_OK {
            Ok(())
        } else {
            Err(Error::JniCall(JniError::from_code(result)))
        }
    }

    /// Run `f` in a new JNI stack frame with room for `capacity` local references, which
    /// are freed when it returns.
    pub fn with_local_frame<F, T, E>(&self, capacity: usize, f: F) -> std::result::Result<T, E>
    where
        F: FnOnce(&mut Env) -> std::result::Result<T, E>,
        E: From<Error>,
    {
        let capacity =
            jint::try_from(capacity).map_err(|_| Error::JniCall(JniError::InvalidArguments))?;
        let result = unsafe { (self.table().PushLocalFrame)(self.raw, capacity) };
        if result != JNI_OK {
            return Err(Error::JniCall(JniError::from_code(result)).into());
        }
        // Pop the frame also when `f` panics, like a native method that catches it.
        struct PopFrame(*mut sys::JNIEnv);
        impl Drop for PopFrame {
            fn drop(&mut self) {
                unsafe { ((**self.0).PopLocalFrame)(self.0, null_mut()) };
            }
        }
        let _frame = PopFrame(self.raw);
        f(&mut unsafe { Env::from_raw(self.raw) })
    }

    /// Create a global reference to `obj`, valid on every thread until it is dropped.
    pub fn new_global_ref<'any_local, O>(&self, obj: O) -> Result<Global<O::GlobalKind>>
    where
        O: Reference + AsRef<JObject<'any_local>>,
    {
        let object = obj.as_raw();
        let global = unsafe { (self.table().NewGlobalRef)(self.raw, object) };
        if global.is_null() && !object.is_null() {
            return Err(Error::JniCall(JniError::NoMemory));
        }
        Ok(Global::new(unsafe { O::global_kind_from_raw(global) }))
    }

    /// Delete a local reference before its JNI stack frame is popped.
    pub fn delete_local_ref<'other_local, O>(&self, obj: O)
    where
        O: Into<JObject<'other_local>>,
    {
        let object = obj.into().into_raw();
        if !object.is_null() {
            unsafe { (self.table().DeleteLocalRef)(self.raw, object) }
        }
    }

    // MARK: Classes
    /// Find a class by its JNI name, like `android/graphics/Canvas`.
    pub fn find_class<S>(&mut self, name: S) -> Result<JClass<'local>>
    where
        S: AsRef<JNIStr>,
    {
        let class = unsafe { (self.table().FindClass)(self.raw, name.as_ref().as_ptr()) };
        self.check_exception()?;
        Ok(unsafe { JClass::from_raw(self, class) })
    }

    /// Get the class of an object.
    pub fn get_object_class<'other_local, O>(&mut self, obj: O) -> Result<JClass<'local>>
    where
        O: AsRef<JObject<'other_local>>,
    {
        let object = non_null(obj.as_ref().as_raw(), "get_object_class object")?;
        let class = unsafe { (self.table().GetObjectClass)(self.raw, object) };
        Ok(unsafe { JClass::from_raw(self, class) })
    }

    fn member_id<T>(
        &self,
        class: &JClass<'_>,
        name: &JNIStr,
        sig: &JNIStr,
        lookup: MemberLookup<T>,
    ) -> Result<*mut T> {
        let class = non_null(class.as_raw(), "member lookup class")?;
        let id = unsafe { lookup(self.raw, class, name.as_ptr(), sig.as_ptr()) };
        self.check_exception()?;
        if id.is_null() {
            Err(Error::NullPtr("member lookup id"))
        } else {
            Ok(id)
        }
    }

    /// Look up an instance method.
    pub fn get_method_id<'other_local, 'sig, 'sig_args, C, N, S>(
        &mut self,
        class: C,
        name: N,
        sig: S,
    ) -> Result<JMethodID>
    where
        C: Desc<'local, JClass<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<MethodSignature<'sig, 'sig_args>>,
    {
        let lookup = self.table().GetMethodID;
        let class = class.lookup(self)?;
        self.member_id(class.as_ref(), name.as_ref(), sig.as_ref().sig(), lookup)
            .map(|id| unsafe { JMethodID::from_raw(id) })
    }

    /// Look up a static method.
    pub fn get_static_method_id<'other_local, 'sig, 'sig_args, C, N, S>(
        &mut self,
        class: C,
        name: N,
        sig: S,
    ) -> Result<JStaticMethodID>
    where
        C: Desc<'local, JClass<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<MethodSignature<'sig, 'sig_args>>,
    {
        let lookup = self.table().GetStaticMethodID;
        let class = class.lookup(self)?;
        self.member_id(class.as_ref(), name.as_ref(), sig.as_ref().sig(), lookup)
            .map(|id| unsafe { JStaticMethodID::from_raw(id) })
    }

    /// Look up an instance field.
    pub fn get_field_id<'other_local, 'sig, C, N, S>(
        &mut self,
        class: C,
        name: N,
        sig: S,
    ) -> Result<JFieldID>
    where
        C: Desc<'local, JClass<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<FieldSignature<'sig>>,
    {
        let lookup = self.table().GetFieldID;
        let class = class.lookup(self)?;
        self.member_id(class.as_ref(), name.as_ref(), sig.as_ref().sig(), lookup)
            .map(|id| unsafe { JFieldID::from_raw(id) })
    }

    /// Look up a static field.
    pub fn get_static_field_id<'other_local, 'sig, C, N, S>(
        &mut self,
        class: C,
        name: N,
        sig: S,
    ) -> Result<JStaticFieldID>
    where
        C: Desc<'local, JClass<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<FieldSignature<'sig>>,
    {
        let lookup = self.table().GetStaticFieldID;
        let class = class.lookup(self)?;
        self.member_id(class.as_ref(), name.as_ref(), sig.as_ref().sig(), lookup)
            .map(|id| unsafe { JStaticFieldID::from_raw(id) })
    }

    // MARK: Methods
    /// Call an instance method by ID.
    ///
    /// # Safety
    /// `method_id` must be a method of `obj` that returns `ret` and takes `args`.
    pub unsafe fn call_method_unchecked<'other_local, O>(
        &mut self,
        obj: O,
        method_id: JMethodID,
        ret: ReturnType,
        args: &[jvalue],
    ) -> Result<JValueOwned<'local>>
    where
        O: AsRef<JObject<'other_local>>,
    {
        let object = non_null(obj.as_ref().as_raw(), "call_method_unchecked object")?;
        let (method, args) = (method_id.into_raw(), args.as_ptr());
        typed_call!(
            self,
            ret,
            (object, method, args),
            CallObjectMethodA,
            CallBooleanMethodA,
            CallByteMethodA,
            CallCharMethodA,
            CallShortMethodA,
            CallIntMethodA,
            CallLongMethodA,
            CallFloatMethodA,
            CallDoubleMethodA,
            CallVoidMethodA
        )
    }

    /// Call a static method by ID.
    ///
    /// # Safety
    /// `method_id` must be a static method of `class` that returns `ret` and takes `args`.
    pub unsafe fn call_static_method_unchecked<'other_local, C>(
        &mut self,
        class: C,
        method_id: JStaticMethodID,
        ret: ReturnType,
        args: &[jvalue],
    ) -> Result<JValueOwned<'local>>
    where
        C: Desc<'local, JClass<'other_local>>,
    {
        let class = class.lookup(self)?;
        let class = non_null(
            class.as_ref().as_raw(),
            "call_static_method_unchecked class",
        )?;
        let (method, args) = (method_id.into_raw(), args.as_ptr());
        typed_call!(
            self,
            ret,
            (class, method, args),
            CallStaticObjectMethodA,
            CallStaticBooleanMethodA,
            CallStaticByteMethodA,
            CallStaticCharMethodA,
            CallStaticShortMethodA,
            CallStaticIntMethodA,
            CallStaticLongMethodA,
            CallStaticFloatMethodA,
            CallStaticDoubleMethodA,
            CallStaticVoidMethodA
        )
    }

    /// Look up and call an instance method by name.
    pub fn call_method<'other_local, 'sig, 'sig_args, O, N, S>(
        &mut self,
        obj: O,
        name: N,
        sig: S,
        args: &[JValue],
    ) -> Result<JValueOwned<'local>>
    where
        O: AsRef<JObject<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<MethodSignature<'sig, 'sig_args>>,
    {
        let sig = sig.as_ref();
        let class = self.get_object_class(obj.as_ref())?;
        let method = self.get_method_id(&class, name, sig);
        self.delete_local_ref(class);
        let args = args.iter().map(JValue::as_jni).collect::<Vec<_>>();
        unsafe { self.call_method_unchecked(obj, method?, sig.ret(), &args) }
    }

    /// Look up and call a static method by name.
    pub fn call_static_method<'other_local, 'sig, 'sig_args, C, N, S>(
        &mut self,
        class: C,
        name: N,
        sig: S,
        args: &[JValue],
    ) -> Result<JValueOwned<'local>>
    where
        C: Desc<'local, JClass<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<MethodSignature<'sig, 'sig_args>>,
    {
        let (class, sig) = (class.lookup(self)?, sig.as_ref());
        let class = class.as_ref();
        let method = self.get_static_method_id(class, name, sig)?;
        let args = args.iter().map(JValue::as_jni).collect::<Vec<_>>();
        unsafe { self.call_static_method_unchecked(class, method, sig.ret(), &args) }
    }

    /// Construct an object with a constructor ID.
    ///
    /// # Safety
    /// `ctor_id` must be a constructor of `class` that takes `ctor_args`.
    pub unsafe fn new_object_unchecked<'other_local, C>(
        &mut self,
        class: C,
        ctor_id: JMethodID,
        ctor_args: &[jvalue],
    ) -> Result<JObject<'local>>
    where
        C: Desc<'local, JClass<'other_local>>,
    {
        let class = class.lookup(self)?;
        let class = non_null(class.as_ref().as_raw(), "new_object class")?;
        let object = unsafe {
            (self.table().NewObjectA)(self.raw, class, ctor_id.into_raw(), ctor_args.as_ptr())
        };
        self.check_exception()?;
        Ok(unsafe { JObject::from_raw(self, object) })
    }

    /// Look up a constructor by signature and construct an object.
    pub fn new_object<'other_local, 'sig, 'sig_args, C, S>(
        &mut self,
        class: C,
        ctor_sig: S,
        ctor_args: &[JValue],
    ) -> Result<JObject<'local>>
    where
        C: Desc<'local, JClass<'other_local>>,
        S: AsRef<MethodSignature<'sig, 'sig_args>>,
    {
        let class = class.lookup(self)?;
        let class = class.as_ref();
        let ctor = self.get_method_id(class, jni_str!("<init>"), ctor_sig)?;
        let args = ctor_args.iter().map(JValue::as_jni).collect::<Vec<_>>();
        unsafe { self.new_object_unchecked(class, ctor, &args) }
    }

    // MARK: Fields
    /// Look up and read an instance field by name.
    pub fn get_field<'other_local, 'sig, O, N, S>(
        &mut self,
        obj: O,
        name: N,
        sig: S,
    ) -> Result<JValueOwned<'local>>
    where
        O: AsRef<JObject<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<FieldSignature<'sig>>,
    {
        let sig = sig.as_ref();
        let class = self.get_object_class(obj.as_ref())?;
        let field = self.get_field_id(&class, name, sig);
        self.delete_local_ref(class);
        let (object, field) = (obj.as_ref().as_raw(), field?.into_raw());
        typed_call!(
            self,
            sig.ty(),
            (object, field),
            GetObjectField,
            GetBooleanField,
            GetByteField,
            GetCharField,
            GetShortField,
            GetIntField,
            GetLongField,
            GetFloatField,
            GetDoubleField,
            none
        )
    }

    /// Look up and read a static field by name.
    pub fn get_static_field<'other_local, 'sig, C, N, S>(
        &mut self,
        class: C,
        name: N,
        sig: S,
    ) -> Result<JValueOwned<'local>>
    where
        C: Desc<'local, JClass<'other_local>>,
        N: AsRef<JNIStr>,
        S: AsRef<FieldSignature<'sig>>,
    {
        let (class, sig) = (class.lookup(self)?, sig.as_ref());
        let class = class.as_ref();
        let field = self.get_static_field_id(class, name, sig)?.into_raw();
        let class = class.as_raw();
        typed_call!(
            self,
            sig.ty(),
            (class, field),
            GetStaticObjectField,
            GetStaticBooleanField,
            GetStaticByteField,
            GetStaticCharField,
            GetStaticShortField,
            GetStaticIntField,
            GetStaticLongField,
            GetStaticFloatField,
            GetStaticDoubleField,
            none
        )
    }

    // MARK: Strings and arrays
    /// Create a Java string from UTF-8 text.
    pub fn new_string(&mut self, from: impl AsRef<str>) -> Result<JString<'local>> {
        JString::from_str(self, from)
    }

    /// Create a `byte[]` with a copy of `buf`.
    pub fn byte_array_from_slice(&mut self, buf: &[u8]) -> Result<JByteArray<'local>> {
        let array = JByteArray::new(self, buf.len())?;
        array.set_region(
            self,
            0,
            &buf.iter()
                .map(|byte| byte.cast_signed())
                .collect::<Vec<_>>(),
        )?;
        Ok(array)
    }

    /// Create an `int[]` of `length` zero elements.
    pub fn new_int_array(&mut self, length: usize) -> Result<JIntArray<'local>> {
        JIntArray::new(self, length)
    }

    /// Copy `buf` into the elements of an `int[]` from `start`.
    pub fn set_int_array_region<'other_local>(
        &self,
        array: impl AsRef<JIntArray<'other_local>>,
        start: jsize,
        buf: &[i32],
    ) -> Result<()> {
        array.as_ref().set_region(self, start, buf)
    }

    /// Create a `long[]` of `length` zero elements.
    pub fn new_long_array(&mut self, length: usize) -> Result<JLongArray<'local>> {
        JLongArray::new(self, length)
    }

    /// Copy `buf` into the elements of a `long[]` from `start`.
    pub fn set_long_array_region<'other_local>(
        &self,
        array: impl AsRef<JLongArray<'other_local>>,
        start: jsize,
        buf: &[i64],
    ) -> Result<()> {
        array.as_ref().set_region(self, start, buf)
    }

    /// Create a `float[]` of `length` zero elements.
    pub fn new_float_array(&mut self, length: usize) -> Result<JFloatArray<'local>> {
        JFloatArray::new(self, length)
    }

    /// Copy `buf` into the elements of a `float[]` from `start`.
    pub fn set_float_array_region<'other_local>(
        &self,
        array: impl AsRef<JFloatArray<'other_local>>,
        start: jsize,
        buf: &[f32],
    ) -> Result<()> {
        array.as_ref().set_region(self, start, buf)
    }

    /// Wrap native memory in a direct `java.nio.ByteBuffer` without copying.
    ///
    /// # Safety
    /// The memory must stay valid and unmoved while Java can access the buffer.
    pub unsafe fn new_direct_byte_buffer(
        &mut self,
        data: *mut u8,
        len: usize,
    ) -> Result<JByteBuffer<'local>> {
        let len = jlong::try_from(len).map_err(|_| Error::JniCall(JniError::InvalidArguments))?;
        let buffer = unsafe { (self.table().NewDirectByteBuffer)(self.raw, data.cast(), len) };
        self.check_exception()?;
        Ok(unsafe { JByteBuffer::from_raw(self, non_null(buffer, "new_direct_byte_buffer")?) })
    }

    // MARK: Native methods
    /// Register native methods of a class.
    ///
    /// # Safety
    /// Every function pointer must match its JNI signature.
    pub unsafe fn register_native_methods<'other_local, C>(
        &mut self,
        class: C,
        methods: &[NativeMethod],
    ) -> Result<()>
    where
        C: Desc<'local, JClass<'other_local>>,
    {
        let class = class.lookup(self)?;
        let class = non_null(class.as_ref().as_raw(), "register_native_methods class")?;
        let count = jint::try_from(methods.len())
            .map_err(|_| Error::JniCall(JniError::InvalidArguments))?;
        let result = unsafe {
            (self.table().RegisterNatives)(self.raw, class, methods.as_ptr().cast(), count)
        };
        self.check_exception()?;
        if result == JNI_OK {
            Ok(())
        } else {
            Err(Error::JniCall(JniError::from_code(result)))
        }
    }
}

pub(crate) const fn non_null(object: sys::jobject, context: &'static str) -> Result<sys::jobject> {
    if object.is_null() {
        Err(Error::NullPtr(context))
    } else {
        Ok(object)
    }
}

// MARK: NativeMethod
/// A native method for [`Env::register_native_methods`].
#[repr(transparent)]
#[derive(Debug, Clone, Copy)]
pub struct NativeMethod<'desc> {
    desc: JNINativeMethod,
    lifetime: PhantomData<&'desc JNIStr>,
}

impl<'desc> NativeMethod<'desc> {
    /// Describe a native method by its name, JNI signature, and function.
    ///
    /// # Safety
    /// `fn_ptr` must be an `extern "system"` function that matches `sig`.
    pub const unsafe fn from_raw_parts(
        name: &'desc JNIStr,
        sig: &'desc JNIStr,
        fn_ptr: *mut c_void,
    ) -> Self {
        Self {
            desc: JNINativeMethod {
                name: name.as_ptr().cast_mut(),
                signature: sig.as_ptr().cast_mut(),
                fnPtr: fn_ptr,
            },
            lifetime: PhantomData,
        }
    }
}

// MARK: EnvUnowned
// The outcome of a native method closure: a value, an error, or a panic.
#[derive(Debug)]
enum Outcome<T, E> {
    Ok(T),
    Err(E),
    Panic(Box<dyn Any + Send + 'static>),
}

/// The raw JNI environment a native method receives, FFI safe as its first argument.
#[repr(transparent)]
#[derive(Debug)]
pub struct EnvUnowned<'local> {
    ptr: *mut sys::JNIEnv,
    lifetime: PhantomData<&'local ()>,
}

impl<'local> EnvUnowned<'local> {
    /// Wrap a raw JNI environment.
    ///
    /// # Safety
    /// `ptr` must be the valid JNI environment of the current thread.
    pub unsafe fn from_raw(ptr: *mut sys::JNIEnv) -> Self {
        assert!(!ptr.is_null(), "EnvUnowned pointer must not be null");
        Self {
            ptr,
            lifetime: PhantomData,
        }
    }

    /// The raw environment pointer.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn as_raw(&self) -> *mut sys::JNIEnv {
        self.ptr
    }

    /// Run `f` with the environment, catching a panic, resolve the outcome to the native
    /// method return value with [`EnvOutcome::resolve`].
    pub fn with_env<F, T, E>(&mut self, f: F) -> EnvOutcome<'local, T, E>
    where
        F: FnOnce(&mut Env<'local>) -> std::result::Result<T, E>,
        E: From<Error>,
    {
        let mut env = unsafe { Env::from_raw(self.ptr) };
        let outcome = match catch_unwind(AssertUnwindSafe(|| f(&mut env))) {
            Ok(Ok(value)) => Outcome::Ok(value),
            Ok(Err(err)) => Outcome::Err(err),
            Err(payload) => Outcome::Panic(payload),
        };
        EnvOutcome {
            raw: self.ptr,
            outcome,
            lifetime: PhantomData,
        }
    }
}

/// The outcome of [`EnvUnowned::with_env`].
#[must_use = "the outcome must be resolved to a value that can be returned to Java"]
#[derive(Debug)]
pub struct EnvOutcome<'local, T, E> {
    raw: *mut sys::JNIEnv,
    outcome: Outcome<T, E>,
    lifetime: PhantomData<&'local mut ()>,
}

impl<T: Default, E> EnvOutcome<'_, T, E> {
    /// Map the outcome to a value with the error policy `P`, the default value when the
    /// policy fails itself.
    pub fn resolve<P: ErrorPolicy<T, E>>(self) -> T {
        let mut env = unsafe { Env::from_raw(self.raw) };
        match self.outcome {
            Outcome::Ok(value) => value,
            Outcome::Err(err) => P::on_error(&mut env, err).unwrap_or_default(),
            Outcome::Panic(payload) => P::on_panic(&mut env, payload).unwrap_or_default(),
        }
    }
}
