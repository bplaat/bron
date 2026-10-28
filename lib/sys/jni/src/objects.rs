/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Java object references, values, and member IDs.

use std::marker::PhantomData;
use std::ops::Deref;

use crate::env::non_null;
use crate::errors::{Error, Result};
pub use crate::refs::*;
use crate::strings::JNIStr;
use crate::sys::*;
use crate::{Env, jni_str};

// MARK: JObject
/// A local reference to a Java object, valid in the JNI stack frame of `'local`.
#[repr(transparent)]
#[derive(Debug)]
pub struct JObject<'local> {
    internal: jobject,
    lifetime: PhantomData<&'local ()>,
}

impl<'local> JObject<'local> {
    /// Wrap a raw local reference of the current JNI stack frame.
    ///
    /// # Safety
    /// `raw` must be a valid reference or null.
    pub const unsafe fn from_raw(_env: &Env<'local>, raw: jobject) -> Self {
        Self {
            internal: raw,
            lifetime: PhantomData,
        }
    }

    /// The null reference.
    pub const fn null() -> JObject<'static> {
        JObject {
            internal: std::ptr::null_mut(),
            lifetime: PhantomData,
        }
    }

    /// The raw reference.
    pub const fn as_raw(&self) -> jobject {
        self.internal
    }

    /// Whether the reference is null.
    pub const fn is_null(&self) -> bool {
        self.internal.is_null()
    }

    /// Unwrap the raw reference.
    pub const fn into_raw(self) -> jobject {
        self.internal
    }

    pub(crate) const fn from_raw_unchecked(raw: jobject) -> Self {
        Self {
            internal: raw,
            lifetime: PhantomData,
        }
    }
}

impl Default for JObject<'_> {
    fn default() -> Self {
        Self::from_raw_unchecked(std::ptr::null_mut())
    }
}

impl<'local> AsRef<JObject<'local>> for JObject<'local> {
    fn as_ref(&self) -> &JObject<'local> {
        self
    }
}

// SAFETY: A 'static object is a global reference, which is valid on every attached thread.
unsafe impl Send for JObject<'static> {}
// SAFETY: See above.
unsafe impl Sync for JObject<'static> {}

// SAFETY: JObject is a transparent jobject wrapper.
unsafe impl Reference for JObject<'_> {
    type Kind<'local> = JObject<'local>;
    type GlobalKind = JObject<'static>;

    fn as_raw(&self) -> jobject {
        self.internal
    }

    fn class_name() -> &'static JNIStr {
        jni_str!("java/lang/Object")
    }

    unsafe fn kind_from_raw<'local>(local_ref: jobject) -> Self::Kind<'local> {
        JObject::from_raw_unchecked(local_ref)
    }

    unsafe fn global_kind_from_raw(global_ref: jobject) -> Self::GlobalKind {
        JObject::from_raw_unchecked(global_ref)
    }
}

// MARK: Object types
macro_rules! object_type {
    ($(#[$doc:meta])* $name:ident($raw:ty) = $class:literal) => {
        $(#[$doc])*
        #[repr(transparent)]
        #[derive(Debug, Default)]
        pub struct $name<'local>(JObject<'local>);

        impl<'local> $name<'local> {
            /// Wrap a raw local reference of the current JNI stack frame.
            ///
            /// # Safety
            /// `raw` must be a valid reference of this type or null.
            pub const unsafe fn from_raw(env: &Env<'local>, raw: $raw) -> Self {
                Self(unsafe { JObject::from_raw(env, raw) })
            }

            /// The null reference.
            pub const fn null() -> $name<'static> {
                $name(JObject::null())
            }

            /// The raw reference.
            pub const fn as_raw(&self) -> $raw {
                self.0.as_raw()
            }

            /// Unwrap the raw reference.
            pub const fn into_raw(self) -> $raw {
                self.0.into_raw()
            }
        }

        impl<'local> Deref for $name<'local> {
            type Target = JObject<'local>;

            fn deref(&self) -> &JObject<'local> {
                &self.0
            }
        }

        impl<'local> AsRef<JObject<'local>> for $name<'local> {
            fn as_ref(&self) -> &JObject<'local> {
                &self.0
            }
        }

        impl<'local> AsRef<$name<'local>> for $name<'local> {
            fn as_ref(&self) -> &$name<'local> {
                self
            }
        }

        impl<'local> From<$name<'local>> for JObject<'local> {
            fn from(object: $name<'local>) -> Self {
                object.0
            }
        }

        // SAFETY: A 'static object is a global reference, valid on every attached thread.
        unsafe impl Send for $name<'static> {}
        // SAFETY: See above.
        unsafe impl Sync for $name<'static> {}

        // SAFETY: The type is a transparent jobject wrapper.
        unsafe impl Reference for $name<'_> {
            type Kind<'local> = $name<'local>;
            type GlobalKind = $name<'static>;

            fn as_raw(&self) -> jobject {
                self.0.as_raw()
            }

            fn class_name() -> &'static JNIStr {
                jni_str!($class)
            }

            unsafe fn kind_from_raw<'local>(local_ref: jobject) -> Self::Kind<'local> {
                $name(JObject::from_raw_unchecked(local_ref))
            }

            unsafe fn global_kind_from_raw(global_ref: jobject) -> Self::GlobalKind {
                $name(JObject::from_raw_unchecked(global_ref))
            }
        }
    };
}

object_type!(
    /// A `java.lang.Class` reference.
    JClass(jclass) = "java/lang/Class"
);
object_type!(
    /// A `java.lang.Throwable` reference.
    JThrowable(jthrowable) = "java/lang/Throwable"
);
object_type!(
    /// A `java.nio.ByteBuffer` reference.
    JByteBuffer(jobject) = "java/nio/ByteBuffer"
);
object_type!(
    /// A `java.lang.String` reference.
    JString(jstring) = "java/lang/String"
);

impl JString<'_> {
    /// Create a Java string from UTF-8 text.
    pub fn from_str<'env_local>(
        env: &mut Env<'env_local>,
        from: impl AsRef<str>,
    ) -> Result<JString<'env_local>> {
        let chars = from.as_ref().encode_utf16().collect::<Vec<_>>();
        let string =
            unsafe { (env.table().NewString)(env.get_raw(), chars.as_ptr(), chars.len() as jsize) };
        env.check_exception()?;
        Ok(unsafe { JString::from_raw(env, string) })
    }

    /// Read the string as UTF-8 text.
    pub fn try_to_string(&self, env: &Env<'_>) -> Result<String> {
        let string = self.as_raw();
        if string.is_null() {
            return Err(Error::NullPtr("try_to_string string argument"));
        }
        unsafe {
            let length = (env.table().GetStringLength)(env.get_raw(), string);
            let mut chars = vec![0; length.max(0) as usize];
            (env.table().GetStringRegion)(env.get_raw(), string, 0, length, chars.as_mut_ptr());
            env.check_exception()?;
            Ok(String::from_utf16_lossy(&chars))
        }
    }
}

// MARK: Arrays
mod private {
    pub trait Sealed {}
    impl Sealed for i8 {}
    impl Sealed for i32 {}
    impl Sealed for i64 {}
    impl Sealed for f32 {}
}

/// A primitive array element type.
pub trait TypeArray: private::Sealed + Copy + Default + Send + Sync + 'static {
    #[doc(hidden)]
    fn class_name() -> &'static JNIStr;
    #[doc(hidden)]
    unsafe fn new_array(env: &Env<'_>, length: jsize) -> jarray;
    #[doc(hidden)]
    unsafe fn get_region(env: &Env<'_>, array: jarray, start: jsize, buf: &mut [Self]);
    #[doc(hidden)]
    unsafe fn set_region(env: &Env<'_>, array: jarray, start: jsize, buf: &[Self]);
}

macro_rules! type_array {
    ($ty:ty, $class:literal, $new:ident, $get:ident, $set:ident) => {
        impl TypeArray for $ty {
            fn class_name() -> &'static JNIStr {
                jni_str!($class)
            }

            unsafe fn new_array(env: &Env<'_>, length: jsize) -> jarray {
                unsafe { (env.table().$new)(env.get_raw(), length) }
            }

            unsafe fn get_region(env: &Env<'_>, array: jarray, start: jsize, buf: &mut [Self]) {
                unsafe {
                    (env.table().$get)(
                        env.get_raw(),
                        array,
                        start,
                        buf.len() as jsize,
                        buf.as_mut_ptr(),
                    );
                }
            }

            unsafe fn set_region(env: &Env<'_>, array: jarray, start: jsize, buf: &[Self]) {
                unsafe {
                    (env.table().$set)(
                        env.get_raw(),
                        array,
                        start,
                        buf.len() as jsize,
                        buf.as_ptr(),
                    );
                }
            }
        }
    };
}

type_array!(
    jbyte,
    "[B",
    NewByteArray,
    GetByteArrayRegion,
    SetByteArrayRegion
);
type_array!(
    jint,
    "[I",
    NewIntArray,
    GetIntArrayRegion,
    SetIntArrayRegion
);
type_array!(
    jlong,
    "[J",
    NewLongArray,
    GetLongArrayRegion,
    SetLongArrayRegion
);
type_array!(
    jfloat,
    "[F",
    NewFloatArray,
    GetFloatArrayRegion,
    SetFloatArrayRegion
);

/// A Java primitive array reference.
#[repr(transparent)]
#[derive(Debug, Default)]
pub struct JPrimitiveArray<'local, T: TypeArray> {
    obj: JObject<'local>,
    lifetime: PhantomData<T>,
}

/// A `byte[]` reference.
pub type JByteArray<'local> = JPrimitiveArray<'local, jbyte>;
/// An `int[]` reference.
pub type JIntArray<'local> = JPrimitiveArray<'local, jint>;
/// A `long[]` reference.
pub type JLongArray<'local> = JPrimitiveArray<'local, jlong>;
/// A `float[]` reference.
pub type JFloatArray<'local> = JPrimitiveArray<'local, jfloat>;

impl<'local, T: TypeArray> JPrimitiveArray<'local, T> {
    /// Create an array of `length` zero elements.
    pub fn new<'env_local>(
        env: &mut Env<'env_local>,
        length: usize,
    ) -> Result<JPrimitiveArray<'env_local, T>> {
        let length = array_length(length)?;
        let array = unsafe { T::new_array(env, length) };
        env.check_exception()?;
        Ok(unsafe { JPrimitiveArray::from_raw(env, array) })
    }

    /// Wrap a raw local reference of the current JNI stack frame.
    ///
    /// # Safety
    /// `raw` must be a valid array of `T` or null.
    pub const unsafe fn from_raw(env: &Env<'local>, raw: jarray) -> Self {
        Self {
            obj: unsafe { JObject::from_raw(env, raw) },
            lifetime: PhantomData,
        }
    }

    /// The raw reference.
    pub const fn as_raw(&self) -> jarray {
        self.obj.as_raw()
    }

    /// The number of elements.
    pub fn len(&self, env: &Env<'_>) -> Result<usize> {
        array_len(env, self.as_raw())
    }

    /// Copy elements from `start` into `buf`.
    pub fn get_region(&self, env: &Env<'_>, start: jsize, buf: &mut [T]) -> Result<()> {
        unsafe {
            T::get_region(
                env,
                non_null(self.as_raw(), "get_region array")?,
                start,
                buf,
            )
        };
        env.check_exception()
    }

    /// Copy `buf` into the elements from `start`.
    pub fn set_region(&self, env: &Env<'_>, start: jsize, buf: &[T]) -> Result<()> {
        unsafe {
            T::set_region(
                env,
                non_null(self.as_raw(), "set_region array")?,
                start,
                buf,
            )
        };
        env.check_exception()
    }
}

impl<'local, T: TypeArray> Deref for JPrimitiveArray<'local, T> {
    type Target = JObject<'local>;

    fn deref(&self) -> &JObject<'local> {
        &self.obj
    }
}

impl<'local, T: TypeArray> AsRef<JObject<'local>> for JPrimitiveArray<'local, T> {
    fn as_ref(&self) -> &JObject<'local> {
        &self.obj
    }
}

impl<'local, T: TypeArray> AsRef<Self> for JPrimitiveArray<'local, T> {
    fn as_ref(&self) -> &Self {
        self
    }
}

impl<'local, T: TypeArray> From<JPrimitiveArray<'local, T>> for JObject<'local> {
    fn from(array: JPrimitiveArray<'local, T>) -> Self {
        array.obj
    }
}

// SAFETY: A 'static array is a global reference, valid on every attached thread.
unsafe impl<T: TypeArray> Send for JPrimitiveArray<'static, T> {}
// SAFETY: See above.
unsafe impl<T: TypeArray> Sync for JPrimitiveArray<'static, T> {}

// SAFETY: The type is a transparent jobject wrapper.
unsafe impl<T: TypeArray> Reference for JPrimitiveArray<'_, T> {
    type Kind<'local> = JPrimitiveArray<'local, T>;
    type GlobalKind = JPrimitiveArray<'static, T>;

    fn as_raw(&self) -> jobject {
        self.obj.as_raw()
    }

    fn class_name() -> &'static JNIStr {
        T::class_name()
    }

    unsafe fn kind_from_raw<'local>(local_ref: jobject) -> Self::Kind<'local> {
        JPrimitiveArray {
            obj: JObject::from_raw_unchecked(local_ref),
            lifetime: PhantomData,
        }
    }

    unsafe fn global_kind_from_raw(global_ref: jobject) -> Self::GlobalKind {
        JPrimitiveArray {
            obj: JObject::from_raw_unchecked(global_ref),
            lifetime: PhantomData,
        }
    }
}

/// A Java object array reference with elements of type `E`.
#[repr(transparent)]
#[derive(Debug)]
pub struct JObjectArray<'local, E: Reference + 'local = JObject<'local>> {
    obj: JObject<'local>,
    lifetime: PhantomData<E>,
}

impl<'local, E: Reference + 'local> JObjectArray<'local, E> {
    /// Create an array of `length` elements that all refer to `initial_element`.
    pub fn new<'env_local, 'any_local>(
        env: &mut Env<'env_local>,
        length: usize,
        initial_element: impl AsRef<E::Kind<'any_local>>,
    ) -> Result<JObjectArray<'env_local, E::Kind<'env_local>>> {
        let length = array_length(length)?;
        let class = env.find_class(E::class_name())?;
        let initial = initial_element.as_ref().as_raw();
        let array =
            unsafe { (env.table().NewObjectArray)(env.get_raw(), length, class.as_raw(), initial) };
        env.delete_local_ref(class);
        env.check_exception()?;
        Ok(JObjectArray {
            obj: unsafe { JObject::from_raw(env, array) },
            lifetime: PhantomData,
        })
    }

    /// Wrap a raw local reference of the current JNI stack frame.
    ///
    /// # Safety
    /// `raw` must be a valid array of `E` or null.
    pub const unsafe fn from_raw(env: &Env<'local>, raw: jobjectArray) -> Self {
        Self {
            obj: unsafe { JObject::from_raw(env, raw) },
            lifetime: PhantomData,
        }
    }

    /// The raw reference.
    pub const fn as_raw(&self) -> jobjectArray {
        self.obj.as_raw()
    }

    /// The number of elements.
    pub fn len(&self, env: &Env<'_>) -> Result<usize> {
        array_len(env, self.as_raw())
    }

    /// Read the element at `index`.
    pub fn get_element<'env_local>(
        &self,
        env: &mut Env<'env_local>,
        index: usize,
    ) -> Result<E::Kind<'env_local>> {
        let array = non_null(self.as_raw(), "get_element array")?;
        let index = array_length(index)?;
        let element = unsafe { (env.table().GetObjectArrayElement)(env.get_raw(), array, index) };
        env.check_exception()?;
        Ok(unsafe { E::kind_from_raw(element) })
    }

    /// Replace the element at `index`.
    pub fn set_element<'any_local>(
        &self,
        env: &Env<'_>,
        index: usize,
        value: impl AsRef<E::Kind<'any_local>>,
    ) -> Result<()> {
        let array = non_null(self.as_raw(), "set_element array")?;
        let index = array_length(index)?;
        let value = value.as_ref().as_raw();
        unsafe {
            (env.table().SetObjectArrayElement)(env.get_raw(), array, index, value);
        }
        env.check_exception()
    }
}

impl<'local, E: Reference + 'local> Default for JObjectArray<'local, E> {
    fn default() -> Self {
        Self {
            obj: JObject::default(),
            lifetime: PhantomData,
        }
    }
}

impl<'local, E: Reference + 'local> Deref for JObjectArray<'local, E> {
    type Target = JObject<'local>;

    fn deref(&self) -> &JObject<'local> {
        &self.obj
    }
}

impl<'local, E: Reference + 'local> AsRef<JObject<'local>> for JObjectArray<'local, E> {
    fn as_ref(&self) -> &JObject<'local> {
        &self.obj
    }
}

impl<'local, E: Reference + 'local> From<JObjectArray<'local, E>> for JObject<'local> {
    fn from(array: JObjectArray<'local, E>) -> Self {
        array.obj
    }
}

fn array_length(length: usize) -> Result<jsize> {
    jsize::try_from(length).map_err(|_| Error::JniCall(crate::errors::JniError::InvalidArguments))
}

fn array_len(env: &Env<'_>, array: jarray) -> Result<usize> {
    let array = non_null(array, "len array")?;
    let length = unsafe { (env.table().GetArrayLength)(env.get_raw(), array) };
    env.check_exception()?;
    Ok(length.max(0) as usize)
}

// MARK: Values
/// A borrowed Java value, used as a method argument.
#[derive(Debug, Clone, Copy)]
pub enum JValue<'obj_ref> {
    /// An object reference.
    Object(&'obj_ref JObject<'obj_ref>),
    /// A `byte`.
    Byte(jbyte),
    /// A `char`.
    Char(jchar),
    /// A `short`.
    Short(jshort),
    /// An `int`.
    Int(jint),
    /// A `long`.
    Long(jlong),
    /// A `boolean`.
    Bool(jboolean),
    /// A `float`.
    Float(jfloat),
    /// A `double`.
    Double(jdouble),
}

impl JValue<'_> {
    /// The raw JNI value.
    pub const fn as_jni(&self) -> jvalue {
        match *self {
            Self::Object(object) => jvalue { l: object.as_raw() },
            Self::Byte(b) => jvalue { b },
            Self::Char(c) => jvalue { c },
            Self::Short(s) => jvalue { s },
            Self::Int(i) => jvalue { i },
            Self::Long(j) => jvalue { j },
            Self::Bool(z) => jvalue { z },
            Self::Float(f) => jvalue { f },
            Self::Double(d) => jvalue { d },
        }
    }
}

impl<'obj_ref, T: AsRef<JObject<'obj_ref>>> From<&'obj_ref T> for JValue<'obj_ref> {
    fn from(object: &'obj_ref T) -> Self {
        Self::Object(object.as_ref())
    }
}

/// An owned Java value, returned by method calls and field reads.
#[derive(Debug)]
pub enum JValueOwned<'local> {
    /// An object reference.
    Object(JObject<'local>),
    /// A `byte`.
    Byte(jbyte),
    /// A `char`.
    Char(jchar),
    /// A `short`.
    Short(jshort),
    /// An `int`.
    Int(jint),
    /// A `long`.
    Long(jlong),
    /// A `boolean`.
    Bool(jboolean),
    /// A `float`.
    Float(jfloat),
    /// A `double`.
    Double(jdouble),
    /// No value.
    Void,
}

macro_rules! value_accessor {
    ($(#[$doc:meta])* $name:ident, $variant:ident, $ty:ty, $type_name:literal) => {
        $(#[$doc])*
        pub const fn $name(self) -> Result<$ty> {
            match self {
                Self::$variant(value) => Ok(value),
                other => Err(Error::WrongJValueType($type_name, other.type_name())),
            }
        }
    };
}

impl<'local> JValueOwned<'local> {
    /// The Java type name of this value.
    pub const fn type_name(&self) -> &'static str {
        match self {
            Self::Object(_) => "object",
            Self::Byte(_) => "byte",
            Self::Char(_) => "char",
            Self::Short(_) => "short",
            Self::Int(_) => "int",
            Self::Long(_) => "long",
            Self::Bool(_) => "bool",
            Self::Float(_) => "float",
            Self::Double(_) => "double",
            Self::Void => "void",
        }
    }

    value_accessor!(
        /// Unwrap an object.
        l,
        Object,
        JObject<'local>,
        "object"
    );
    value_accessor!(
        /// Unwrap a `boolean`.
        z,
        Bool,
        bool,
        "bool"
    );
    value_accessor!(
        /// Unwrap a `byte`.
        b,
        Byte,
        jbyte,
        "byte"
    );
    value_accessor!(
        /// Unwrap a `char`.
        c,
        Char,
        jchar,
        "char"
    );
    value_accessor!(
        /// Unwrap a `short`.
        s,
        Short,
        jshort,
        "short"
    );
    value_accessor!(
        /// Unwrap an `int`.
        i,
        Int,
        jint,
        "int"
    );
    value_accessor!(
        /// Unwrap a `long`.
        j,
        Long,
        jlong,
        "long"
    );
    value_accessor!(
        /// Unwrap a `float`.
        f,
        Float,
        jfloat,
        "float"
    );
    value_accessor!(
        /// Unwrap a `double`.
        d,
        Double,
        jdouble,
        "double"
    );

    /// Unwrap no value.
    pub const fn v(self) -> Result<()> {
        match self {
            Self::Void => Ok(()),
            other => Err(Error::WrongJValueType("void", other.type_name())),
        }
    }
}

// MARK: IDs
macro_rules! member_id {
    ($(#[$doc:meta])* $name:ident($raw:ty)) => {
        $(#[$doc])*
        #[repr(transparent)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name($raw);

        impl $name {
            /// Wrap a raw ID.
            ///
            /// # Safety
            /// `raw` must be a valid, non-null ID.
            pub const unsafe fn from_raw(raw: $raw) -> Self {
                Self(raw)
            }

            /// Unwrap the raw ID.
            pub const fn into_raw(self) -> $raw {
                self.0
            }
        }

        // SAFETY: Member IDs stay valid while their class is loaded, on every thread.
        unsafe impl Send for $name {}
        // SAFETY: See above.
        unsafe impl Sync for $name {}
    };
}

member_id!(
    /// An instance method ID.
    JMethodID(jmethodID)
);
member_id!(
    /// A static method ID.
    JStaticMethodID(jmethodID)
);
member_id!(
    /// An instance field ID.
    JFieldID(jfieldID)
);
member_id!(
    /// A static field ID.
    JStaticFieldID(jfieldID)
);
