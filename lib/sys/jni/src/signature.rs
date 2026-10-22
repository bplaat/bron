/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! JNI type signatures.

use std::marker::PhantomData;

use crate::strings::JNIStr;

/// A primitive Java type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    /// `boolean`, `Z`.
    Boolean,
    /// `byte`, `B`.
    Byte,
    /// `char`, `C`.
    Char,
    /// `double`, `D`.
    Double,
    /// `float`, `F`.
    Float,
    /// `int`, `I`.
    Int,
    /// `long`, `J`.
    Long,
    /// `short`, `S`.
    Short,
    /// `void`, `V`.
    Void,
}

/// A Java type as JNI passes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JavaType {
    /// A primitive type.
    Primitive(Primitive),
    /// An object reference.
    Object,
    /// An array reference.
    Array,
}

/// The return type of a method.
pub type ReturnType = JavaType;

impl JavaType {
    /// Parse the type that starts a JNI type signature.
    const fn parse(sig: &[u8], index: usize) -> Self {
        assert!(index < sig.len(), "JNI signature is missing a type");
        match sig[index] {
            b'Z' => Self::Primitive(Primitive::Boolean),
            b'B' => Self::Primitive(Primitive::Byte),
            b'C' => Self::Primitive(Primitive::Char),
            b'D' => Self::Primitive(Primitive::Double),
            b'F' => Self::Primitive(Primitive::Float),
            b'I' => Self::Primitive(Primitive::Int),
            b'J' => Self::Primitive(Primitive::Long),
            b'S' => Self::Primitive(Primitive::Short),
            b'V' => Self::Primitive(Primitive::Void),
            b'L' => Self::Object,
            b'[' => Self::Array,
            _ => panic!("invalid JNI signature type"),
        }
    }
}

/// A method signature like `(ILjava/lang/String;)V`, create it with [`crate::jni_sig!`].
#[derive(Debug, Clone, Copy)]
pub struct MethodSignature<'sig, 'args> {
    sig: &'sig JNIStr,
    ret: JavaType,
    _args: PhantomData<&'args [JavaType]>,
}

impl<'sig> MethodSignature<'sig, '_> {
    /// Parse the return type of a raw method signature.
    pub const fn from_jni_str(sig: &'sig JNIStr) -> Self {
        let bytes = sig.to_bytes();
        assert!(
            !bytes.is_empty() && bytes[0] == b'(',
            "JNI method signature must start with '('"
        );
        let mut index = 1;
        while index < bytes.len() && bytes[index] != b')' {
            index += 1;
        }
        Self {
            sig,
            ret: JavaType::parse(bytes, index + 1),
            _args: PhantomData,
        }
    }

    /// The raw signature.
    pub const fn sig(&self) -> &JNIStr {
        self.sig
    }

    /// The return type.
    pub const fn ret(&self) -> JavaType {
        self.ret
    }
}

impl<'sig, 'args> AsRef<Self> for MethodSignature<'sig, 'args> {
    fn as_ref(&self) -> &Self {
        self
    }
}

/// A field signature like `Landroid/graphics/Paint$Style;`, create it with [`crate::jni_sig!`].
#[derive(Debug, Clone, Copy)]
pub struct FieldSignature<'sig> {
    sig: &'sig JNIStr,
    ty: JavaType,
}

impl<'sig> FieldSignature<'sig> {
    /// Parse the type of a raw field signature.
    pub const fn from_jni_str(sig: &'sig JNIStr) -> Self {
        Self {
            sig,
            ty: JavaType::parse(sig.to_bytes(), 0),
        }
    }

    /// The raw signature.
    pub const fn sig(&self) -> &JNIStr {
        self.sig
    }

    /// The field type.
    pub const fn ty(&self) -> JavaType {
        self.ty
    }
}

impl<'sig> AsRef<Self> for FieldSignature<'sig> {
    fn as_ref(&self) -> &Self {
        self
    }
}

/// The output of [`crate::jni_sig!`], usable as a method or a field signature.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub enum Signature {
    Method(MethodSignature<'static, 'static>),
    Field(FieldSignature<'static>),
}

impl Signature {
    pub const fn new(sig: &'static JNIStr) -> Self {
        match sig.to_bytes() {
            [b'(', ..] => Self::Method(MethodSignature::from_jni_str(sig)),
            _ => Self::Field(FieldSignature::from_jni_str(sig)),
        }
    }
}

impl AsRef<MethodSignature<'static, 'static>> for Signature {
    fn as_ref(&self) -> &MethodSignature<'static, 'static> {
        match self {
            Self::Method(method) => method,
            Self::Field(_) => panic!("expected a method signature"),
        }
    }
}

impl AsRef<FieldSignature<'static>> for Signature {
    fn as_ref(&self) -> &FieldSignature<'static> {
        match self {
            Self::Field(field) => field,
            Self::Method(_) => panic!("expected a field signature"),
        }
    }
}
