/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Descriptors of Java values that can be looked up through an [`Env`].

use crate::Env;
use crate::errors::Result;
use crate::objects::{Global, JObject, Reference};

/// A descriptor of a `T`, like a reference to it.
///
/// Unlike upstream, only `T`, `&T` and `&Global<T>` are descriptors, lookups by name are not
/// supported.
///
/// # Safety
/// [`Desc::lookup`] must return a valid `T`.
pub unsafe trait Desc<'local, T> {
    /// The type that this `Desc` returns.
    type Output: AsRef<T>;

    /// Look up the concrete value.
    fn lookup(self, _: &mut Env<'local>) -> Result<Self::Output>;
}

// SAFETY: The value is returned as is.
unsafe impl<'local, T> Desc<'local, T> for T
where
    T: AsRef<T>,
{
    type Output = Self;

    fn lookup(self, _: &mut Env<'local>) -> Result<T> {
        Ok(self)
    }
}

// SAFETY: The reference is returned as is.
unsafe impl<'local, T> Desc<'local, T> for &T
where
    T: AsRef<T>,
{
    type Output = Self;

    fn lookup(self, _: &mut Env<'local>) -> Result<Self::Output> {
        Ok(self)
    }
}

// SAFETY: The global reference stays valid while it is borrowed.
unsafe impl<'local, 'obj_ref, T> Desc<'local, T> for &'obj_ref Global<T>
where
    T: Reference
        + AsRef<T>
        + AsRef<JObject<'static>>
        + Into<JObject<'static>>
        + Default
        + Send
        + Sync
        + 'static,
{
    type Output = &'obj_ref T;

    fn lookup(self, _: &mut Env<'local>) -> Result<Self::Output> {
        Ok(&**self)
    }
}
