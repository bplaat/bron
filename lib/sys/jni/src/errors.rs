/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Error types and native method error policies.

use std::any::Any;
use std::fmt::{self, Display, Formatter};

use crate::strings::JNIString;
use crate::sys::{JNI_EDETACHED, JNI_EEXIST, JNI_EINVAL, JNI_ENOMEM, JNI_EVERSION, jint};
use crate::{Env, jni_str};

/// A result of a JNI operation.
pub type Result<T> = std::result::Result<T, Error>;

/// A JNI error.
#[derive(Debug)]
pub enum Error {
    /// The Java VM was not registered with [`crate::vm::JavaVM::from_raw`].
    UninitializedJavaVM,
    /// A value had another type than requested, the expected and the actual type.
    WrongJValueType(&'static str, &'static str),
    /// A required reference was null.
    NullPtr(&'static str),
    /// A Java exception is pending.
    JavaException,
    /// A Java exception was thrown and cleared, it is logged with its stack trace.
    CaughtJavaException {
        /// The exception class name.
        name: String,
        /// The exception message.
        msg: String,
    },
    /// A JNI function returned an error code.
    JniCall(JniError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UninitializedJavaVM => write!(f, "the Java VM is not initialized"),
            Self::WrongJValueType(expected, actual) => {
                write!(f, "invalid value type, expected {expected} got {actual}")
            }
            Self::NullPtr(context) => write!(f, "null pointer in {context}"),
            Self::JavaException => write!(f, "Java exception was thrown"),
            Self::CaughtJavaException { name, msg } => {
                write!(f, "caught Java exception {name}: {msg}")
            }
            Self::JniCall(error) => write!(f, "JNI call failed: {error}"),
        }
    }
}

impl std::error::Error for Error {}

/// A JNI function error code.
#[derive(Debug)]
pub enum JniError {
    /// An unknown error.
    Unknown,
    /// The thread is not attached to the Java VM.
    ThreadDetached,
    /// The JNI version is not supported.
    WrongVersion,
    /// Not enough memory.
    NoMemory,
    /// The Java VM already exists.
    AlreadyCreated,
    /// Invalid arguments.
    InvalidArguments,
    /// Another error code.
    Other(jint),
}

impl JniError {
    pub(crate) const fn from_code(code: jint) -> Self {
        match code {
            JNI_EDETACHED => Self::ThreadDetached,
            JNI_EVERSION => Self::WrongVersion,
            JNI_ENOMEM => Self::NoMemory,
            JNI_EEXIST => Self::AlreadyCreated,
            JNI_EINVAL => Self::InvalidArguments,
            -1 => Self::Unknown,
            code => Self::Other(code),
        }
    }
}

impl Display for JniError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown => write!(f, "unknown error"),
            Self::ThreadDetached => write!(f, "thread detached"),
            Self::WrongVersion => write!(f, "wrong version"),
            Self::NoMemory => write!(f, "out of memory"),
            Self::AlreadyCreated => write!(f, "already created"),
            Self::InvalidArguments => write!(f, "invalid arguments"),
            Self::Other(code) => write!(f, "error code {code}"),
        }
    }
}

// MARK: ErrorPolicy
/// Maps the error or panic of a native method into its return value, see
/// [`crate::EnvOutcome::resolve`].
pub trait ErrorPolicy<T, E> {
    /// Handle an error returned by the native method.
    fn on_error(env: &mut Env<'_>, err: E) -> Result<T>;

    /// Handle a panic of the native method.
    fn on_panic(env: &mut Env<'_>, payload: Box<dyn Any + Send + 'static>) -> Result<T>;
}

/// Throw a `java.lang.RuntimeException` with the error or panic message and return the
/// default value.
#[derive(Debug, Default)]
pub struct ThrowRuntimeExAndDefault;

impl ThrowRuntimeExAndDefault {
    fn throw<T: Default>(env: &mut Env<'_>, message: String) -> Result<T> {
        if !env.exception_check() {
            let class = env.find_class(jni_str!("java/lang/RuntimeException"))?;
            env.throw_new(&class, JNIString::new(message))?;
        }
        Ok(T::default())
    }
}

impl<T: Default, E: Display> ErrorPolicy<T, E> for ThrowRuntimeExAndDefault {
    fn on_error(env: &mut Env<'_>, err: E) -> Result<T> {
        Self::throw(env, format!("Rust error: {err}"))
    }

    fn on_panic(env: &mut Env<'_>, payload: Box<dyn Any + Send + 'static>) -> Result<T> {
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_string());
        Self::throw(env, format!("Rust panic: {message}"))
    }
}
