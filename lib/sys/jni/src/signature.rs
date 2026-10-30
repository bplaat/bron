/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! JNI type signatures.

use crate::strings::JNIStr;

/// A primitive Java type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// A method signature like `(ILjava/lang/String;)V`, create it with [`crate::jni_sig!`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSignature<'sig, 'args> {
    sig: &'sig JNIStr,
    args: &'args [JavaType],
    ret: JavaType,
}

impl<'sig, 'args> MethodSignature<'sig, 'args> {
    /// Create a method signature from its raw parts.
    ///
    /// # Safety
    /// The argument and return types must match the signature string.
    pub const unsafe fn from_raw_parts(
        sig: &'sig JNIStr,
        args: &'args [JavaType],
        ret: ReturnType,
    ) -> Self {
        Self { sig, args, ret }
    }

    /// The raw signature.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn sig(&self) -> &JNIStr {
        self.sig
    }

    /// The argument types.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn args(&self) -> &[JavaType] {
        self.args
    }

    /// The return type.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn ret(&self) -> JavaType {
        self.ret
    }
}

impl<'sig, 'args> AsRef<Self> for MethodSignature<'sig, 'args> {
    fn as_ref(&self) -> &Self {
        self
    }
}

/// A field signature like `Landroid/graphics/Paint$Style;`, create it with [`crate::jni_sig!`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSignature<'sig> {
    sig: &'sig JNIStr,
    ty: JavaType,
}

impl<'sig> FieldSignature<'sig> {
    /// Create a field signature from its raw parts.
    ///
    /// # Safety
    /// The field type must match the signature string.
    pub const unsafe fn from_raw_parts(sig: &'sig JNIStr, ty: JavaType) -> Self {
        Self { sig, ty }
    }

    /// The raw signature.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn sig(&self) -> &JNIStr {
        self.sig
    }

    /// The field type.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn ty(&self) -> JavaType {
        self.ty
    }
}

impl<'sig> AsRef<Self> for FieldSignature<'sig> {
    fn as_ref(&self) -> &Self {
        self
    }
}

// MARK: jni_sig! support
// The `jni_sig!` macro selects the signature type at compile time: it implements `SigLiteral` for
// a local type and projects `<Select<IS_METHOD> as Parse<Literal>>::VALUE`.

#[doc(hidden)]
pub trait __SigLiteral {
    const SIG: &'static JNIStr;
    const ARGS: &'static [JavaType];
    const TYPE: JavaType;
}

#[doc(hidden)]
pub struct __Select<const METHOD: bool>;

#[doc(hidden)]
pub trait __Parse<L> {
    type Output;
    const VALUE: Self::Output;
}

impl<L: __SigLiteral> __Parse<L> for __Select<true> {
    type Output = MethodSignature<'static, 'static>;
    // SAFETY: `L` was parsed from the same literal by the functions below.
    const VALUE: Self::Output =
        unsafe { MethodSignature::from_raw_parts(L::SIG, L::ARGS, L::TYPE) };
}

impl<L: __SigLiteral> __Parse<L> for __Select<false> {
    type Output = FieldSignature<'static>;
    // SAFETY: `L` was parsed from the same literal by the functions below.
    const VALUE: Self::Output = unsafe { FieldSignature::from_raw_parts(L::SIG, L::TYPE) };
}

// Parse the type at `index`, returning it and the index after it.
const fn parse_type(sig: &[u8], index: usize) -> (JavaType, usize) {
    assert!(index < sig.len(), "JNI signature is missing a type");
    let ty = match sig[index] {
        b'Z' => Primitive::Boolean,
        b'B' => Primitive::Byte,
        b'C' => Primitive::Char,
        b'D' => Primitive::Double,
        b'F' => Primitive::Float,
        b'I' => Primitive::Int,
        b'J' => Primitive::Long,
        b'S' => Primitive::Short,
        b'V' => Primitive::Void,
        b'L' => {
            let mut end = index + 1;
            while end < sig.len() && sig[end] != b';' {
                end += 1;
            }
            assert!(
                end > index + 1 && end < sig.len(),
                "invalid JNI object type"
            );
            return (JavaType::Object, end + 1);
        }
        b'[' => {
            let (element, end) = parse_type(sig, index + 1);
            assert!(
                !matches!(element, JavaType::Primitive(Primitive::Void)),
                "invalid JNI array type"
            );
            return (JavaType::Array, end);
        }
        _ => panic!("invalid JNI signature type"),
    };
    (JavaType::Primitive(ty), index + 1)
}

#[doc(hidden)]
pub const fn __is_method(sig: &str) -> bool {
    matches!(sig.as_bytes(), [b'(', ..])
}

#[doc(hidden)]
pub const fn __arg_count(sig: &str) -> usize {
    let sig = sig.as_bytes();
    if !matches!(sig, [b'(', ..]) {
        return 0;
    }
    let (mut count, mut index) = (0, 1);
    while index < sig.len() && sig[index] != b')' {
        let (ty, end) = parse_type(sig, index);
        assert!(
            !matches!(ty, JavaType::Primitive(Primitive::Void)),
            "JNI method arguments can't be void"
        );
        (count, index) = (count + 1, end);
    }
    count
}

#[doc(hidden)]
pub const fn __args<const N: usize>(sig: &str) -> [JavaType; N] {
    let sig = sig.as_bytes();
    let mut args = [JavaType::Object; N];
    let (mut count, mut index) = (0, 1);
    while count < N {
        let (ty, end) = parse_type(sig, index);
        (args[count], count, index) = (ty, count + 1, end);
    }
    args
}

#[doc(hidden)]
pub const fn __type(sig: &str) -> JavaType {
    let sig = sig.as_bytes();
    let mut index = 0;
    if matches!(sig, [b'(', ..]) {
        while index < sig.len() && sig[index] != b')' {
            index += 1;
        }
        index += 1;
    }
    let (ty, end) = parse_type(sig, index);
    assert!(end == sig.len(), "JNI signature has trailing characters");
    assert!(
        index > 0 || !matches!(ty, JavaType::Primitive(Primitive::Void)),
        "JNI fields can't be void"
    );
    ty
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;
    use crate::jni_sig;

    #[test]
    fn test_method_signature() {
        let sig = jni_sig!("(I[Ljava/lang/String;[[J)Landroid/graphics/Paint$Style;");
        assert_eq!(
            sig.sig().to_bytes(),
            b"(I[Ljava/lang/String;[[J)Landroid/graphics/Paint$Style;"
        );
        assert_eq!(
            sig.args(),
            [
                JavaType::Primitive(Primitive::Int),
                JavaType::Array,
                JavaType::Array
            ]
        );
        assert_eq!(sig.ret(), JavaType::Object);
        assert_eq!(jni_sig!("()V").ret(), JavaType::Primitive(Primitive::Void));
    }

    #[test]
    fn test_field_signature() {
        let sig: FieldSignature<'static> = jni_sig!("Landroid/graphics/Paint$Style;");
        assert_eq!(sig.ty(), JavaType::Object);
        assert_eq!(jni_sig!("F").ty(), JavaType::Primitive(Primitive::Float));
        assert_eq!(jni_sig!("[I").ty(), JavaType::Array);
    }

    #[test]
    fn test_signature_in_generic_fn() {
        fn generic<T: Default>() -> MethodSignature<'static, 'static> {
            let _ = T::default();
            jni_sig!("(J)Z")
        }
        assert_eq!(
            generic::<u8>().ret(),
            JavaType::Primitive(Primitive::Boolean)
        );
    }
}
