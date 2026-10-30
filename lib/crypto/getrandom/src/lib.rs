/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [getrandom](https://crates.io/crates/getrandom) crate

use std::fmt;
use std::num::NonZeroI32;

// MARK: Error
/// Error type
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Error(NonZeroI32);

impl Error {
    /// The platform-specific `errno` returned a non-positive value.
    pub const ERRNO_NOT_POSITIVE: Error = Self::new_internal(1);
    /// Encountered an unexpected situation which should not happen in practice.
    pub const UNEXPECTED: Error = Self::new_internal(2);
    /// Call to `CCRandomGenerateBytes` failed.
    #[cfg(target_os = "ios")]
    const IOS_RANDOM_GEN: Error = Self::new_internal(10);

    const fn new_internal(n: u16) -> Self {
        match NonZeroI32::new((1 << 16) + n as i32) {
            Some(code) => Self(code),
            None => unreachable!(),
        }
    }

    #[allow(dead_code)]
    const fn from_errno(errno: i32) -> Self {
        match NonZeroI32::new(errno.wrapping_neg()) {
            Some(code) if errno > 0 => Self(code),
            _ => Self::ERRNO_NOT_POSITIVE,
        }
    }

    #[allow(dead_code)]
    fn from_io(error: &std::io::Error) -> Self {
        error
            .raw_os_error()
            .map_or(Self::UNEXPECTED, Self::from_errno)
    }

    /// Extract the raw OS error code (if this error came from the OS)
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn raw_os_error(self) -> Option<i32> {
        let code = self.0.get();
        if code < 0 { code.checked_neg() } else { None }
    }

    fn internal_desc(self) -> Option<&'static str> {
        match self {
            Self::ERRNO_NOT_POSITIVE => Some("errno: did not return a positive value"),
            Self::UNEXPECTED => Some("unexpected situation"),
            #[cfg(target_os = "ios")]
            Self::IOS_RANDOM_GEN => Some("SecRandomCopyBytes: iOS Security framework failure"),
            _ => None,
        }
    }
}

impl std::error::Error for Error {}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut dbg = f.debug_struct("Error");
        if let Some(errno) = self.raw_os_error() {
            dbg.field("os_error", &errno);
            #[cfg(feature = "std")]
            dbg.field("description", &std::io::Error::from_raw_os_error(errno));
        } else if let Some(desc) = self.internal_desc() {
            dbg.field("internal_code", &self.0.get());
            dbg.field("description", &desc);
        } else {
            dbg.field("unknown_code", &self.0.get());
        }
        dbg.finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(errno) = self.raw_os_error() {
            if cfg!(feature = "std") {
                std::io::Error::from_raw_os_error(errno).fmt(f)
            } else {
                write!(f, "OS Error: {errno}")
            }
        } else if let Some(desc) = self.internal_desc() {
            f.write_str(desc)
        } else {
            write!(f, "Unknown Error: {}", self.0.get())
        }
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::Error {
    fn from(err: Error) -> Self {
        match err.raw_os_error() {
            Some(errno) => Self::from_raw_os_error(errno),
            None => Self::other(err),
        }
    }
}

// MARK: Fill
/// Fill buffer with crypto random bytes
#[allow(unsafe_code)]
pub fn fill(buf: &mut [u8]) -> Result<(), Error> {
    cfg_select! {
        any(target_os = "macos", target_os = "openbsd") => {
            unsafe extern "C" {
                fn getentropy(buf: *mut u8, buflen: usize) -> i32;
            }
            for chunk in buf.chunks_mut(256) {
                // SAFETY: chunk is a valid mutable byte slice with length <= 256, satisfying getentropy's requirements.
                if unsafe { getentropy(chunk.as_mut_ptr(), chunk.len()) } != 0 {
                    return Err(Error::from_io(&std::io::Error::last_os_error()));
                }
            }
        }
        target_os = "ios" => {
            unsafe extern "C" {
                fn CCRandomGenerateBytes(bytes: *mut u8, count: usize) -> i32;
            }
            // SAFETY: buf is a valid mutable byte slice; CCRandomGenerateBytes fills it.
            if unsafe { CCRandomGenerateBytes(buf.as_mut_ptr(), buf.len()) } != 0 {
                return Err(Error::IOS_RANDOM_GEN);
            }
        }
        any(
            all(target_os = "linux", target_env = "musl"),
            target_os = "freebsd",
            target_os = "dragonfly",
        ) => {
            unsafe extern "C" {
                fn getrandom(buf: *mut u8, buflen: usize, flags: u32) -> isize;
            }
            let mut buf = buf;
            while !buf.is_empty() {
                // SAFETY: buf is a valid mutable byte slice.
                let n = unsafe { getrandom(buf.as_mut_ptr(), buf.len(), 0) };
                match n {
                    1.. => buf = &mut buf[n as usize..],
                    -1 => {
                        let error = std::io::Error::last_os_error();
                        if error.kind() != std::io::ErrorKind::Interrupted {
                            return Err(Error::from_io(&error));
                        }
                    }
                    _ => return Err(Error::UNEXPECTED),
                }
            }
        }
        unix => {
            type GetrandomFn = unsafe extern "C" fn(*mut u8, usize, u32) -> isize;
            fn resolve_getrandom() -> Option<GetrandomFn> {
                const RTLD_DEFAULT: *mut std::ffi::c_void = std::ptr::null_mut();
                unsafe extern "C" {
                    fn dlsym(
                        handle: *mut std::ffi::c_void,
                        symbol: *const std::ffi::c_char,
                    ) -> *mut std::ffi::c_void;
                }
                // SAFETY: RTLD_DEFAULT and the c-string literal are valid arguments to dlsym.
                let ptr = unsafe { dlsym(RTLD_DEFAULT, c"getrandom".as_ptr()) };
                if ptr.is_null() {
                    None
                } else {
                    // SAFETY: dlsym returned a non-null pointer for "getrandom", which has the GetrandomFn signature.
                    Some(unsafe { std::mem::transmute::<*mut std::ffi::c_void, GetrandomFn>(ptr) })
                }
            }
            static GETRANDOM: std::sync::LazyLock<Option<GetrandomFn>> =
                std::sync::LazyLock::new(resolve_getrandom);
            if let Some(getrandom) = *GETRANDOM {
                // SAFETY: buf is a valid mutable byte slice; getrandom was resolved and has the correct signature.
                let n = unsafe { getrandom(buf.as_mut_ptr(), buf.len(), 0) };
                if n >= 0 && n as usize == buf.len() {
                    return Ok(());
                }
                // Fall through to /dev/urandom if getrandom fails or returns ENOSYS
            }

            use std::io::Read;
            std::fs::File::open("/dev/urandom")
                .and_then(|mut file| file.read_exact(buf))
                .map_err(|error| Error::from_io(&error))?;
        }
        windows => {
            #[cfg_attr(
                target_arch = "x86",
                link(
                    name = "bcryptprimitives",
                    kind = "raw-dylib",
                    import_name_type = "undecorated"
                )
            )]
            #[cfg_attr(
                not(target_arch = "x86"),
                link(name = "bcryptprimitives", kind = "raw-dylib")
            )]
            unsafe extern "system" {
                fn ProcessPrng(pbData: *mut u8, cbData: usize) -> i32;
            }
            // SAFETY: buf is a valid mutable byte slice; ProcessPrng is a documented Windows API that fills it.
            if unsafe { ProcessPrng(buf.as_mut_ptr(), buf.len()) } == 0 {
                return Err(Error::UNEXPECTED);
            }
        }
        _ => compile_error!("Unsupported platform"),
    }
    Ok(())
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_randomness() {
        let mut buf1 = [0u8; 32];
        fill(&mut buf1).unwrap();

        let mut buf2 = [0u8; 32];
        fill(&mut buf2).unwrap();

        assert_ne!(buf1, buf2);
    }

    #[test]
    fn test_empty() {
        fill(&mut []).unwrap();
    }

    #[test]
    fn test_large() {
        let mut buf = vec![0u8; 1024 * 1024];
        fill(&mut buf).unwrap();
        assert!(buf.chunks(256).all(|chunk| chunk.iter().any(|&b| b != 0)));
    }

    #[test]
    fn test_error() {
        let error = Error::from_errno(2);
        assert_eq!(error.raw_os_error(), Some(2));
        assert_eq!(Error::from_errno(0), Error::ERRNO_NOT_POSITIVE);
        assert_eq!(Error::UNEXPECTED.raw_os_error(), None);
        assert_eq!(Error::UNEXPECTED.to_string(), "unexpected situation");
        assert_eq!(
            format!("{:?}", Error::UNEXPECTED),
            "Error { internal_code: 65538, description: \"unexpected situation\" }"
        );
        #[cfg(feature = "std")]
        assert_eq!(std::io::Error::from(error).raw_os_error(), Some(2));
    }
}
