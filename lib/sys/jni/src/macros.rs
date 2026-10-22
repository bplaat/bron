/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

/// Encode a string literal as a `&'static JNIStr` at compile time.
///
/// Unlike the upstream macro, only a single plain string literal without supplementary
/// characters is supported.
#[macro_export]
macro_rules! jni_str {
    ($string:literal) => {
        const {
            match ::std::ffi::CStr::from_bytes_with_nul(concat!($string, "\0").as_bytes()) {
                Ok(cstr) => match $crate::strings::JNIStr::from_cstr(cstr) {
                    Some(string) => string,
                    None => panic!("not a valid JNI string"),
                },
                Err(_) => panic!("JNI strings can't contain null characters"),
            }
        }
    };
}

/// Parse a raw JNI method or field signature literal at compile time, like `"(IF)V"` or
/// `"Ljava/lang/String;"`.
///
/// Unlike the upstream macro, the Rust like `(a: jint) -> void` syntax is not supported.
#[macro_export]
macro_rules! jni_sig {
    ($sig:literal) => {
        const { $crate::signature::Signature::new($crate::jni_str!($sig)) }
    };
}
