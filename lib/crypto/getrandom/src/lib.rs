/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [getrandom](https://crates.io/crates/getrandom) crate

use std::io::Error;

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
                    return Err(Error::last_os_error());
                }
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
                    0 => return Err(Error::other("getrandom returned no bytes")),
                    _ => {
                        let error = Error::last_os_error();
                        if error.kind() != std::io::ErrorKind::Interrupted {
                            return Err(error);
                        }
                    }
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
            std::fs::File::open("/dev/urandom")?.read_exact(buf)?;
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
                return Err(Error::other("ProcessPrng failed"));
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
}
