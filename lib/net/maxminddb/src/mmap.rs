/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![allow(unsafe_code)]

use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::path::Path;
use std::ptr::NonNull;

// MARK: Mmap

/// A read-only memory map of a database file.
///
/// The operating system pages the file in on demand and can evict it again, so a large database
/// does not stay resident on the heap. The file must not be modified while it is mapped; replace
/// it by renaming a new file over it instead.
pub struct Mmap {
    ptr: NonNull<u8>,
    len: usize,
}

// SAFETY: The mapping is read-only and stays valid until it is dropped.
unsafe impl Send for Mmap {}
// SAFETY: The mapping is never written, so shared access is sound.
unsafe impl Sync for Mmap {}

impl Mmap {
    /// Map a file read-only.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        let len = usize::try_from(file.metadata()?.len())
            .map_err(|_| io::Error::other("file too large to map"))?;
        if len == 0 {
            // Empty mappings are invalid, and an empty slice needs no memory.
            return Ok(Self {
                ptr: NonNull::dangling(),
                len,
            });
        }
        // The mapping keeps its own reference, so the file can be closed afterwards.
        let ptr = platform::map(&file, len)?;
        Ok(Self { ptr, len })
    }
}

impl AsRef<[u8]> for Mmap {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: The pointer maps `len` readable bytes, or is dangling for an empty slice.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.len) }
    }
}

impl Drop for Mmap {
    fn drop(&mut self) {
        if self.len != 0 {
            platform::unmap(self.ptr, self.len);
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::ffi::{c_int, c_long};
    use std::os::fd::AsRawFd;

    use super::*;

    const PROT_READ: c_int = 1;
    const MAP_PRIVATE: c_int = 2;

    unsafe extern "C" {
        fn mmap(
            addr: *mut c_void,
            len: usize,
            prot: c_int,
            flags: c_int,
            fd: c_int,
            offset: c_long,
        ) -> *mut c_void;
        fn munmap(addr: *mut c_void, len: usize) -> c_int;
    }

    pub(super) fn map(file: &File, len: usize) -> io::Result<NonNull<u8>> {
        // SAFETY: A fresh private read-only mapping of an open file descriptor.
        let ptr = unsafe {
            mmap(
                std::ptr::null_mut(),
                len,
                PROT_READ,
                MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        // MAP_FAILED is (void *)-1.
        if ptr as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        NonNull::new(ptr.cast()).ok_or_else(|| io::Error::other("mmap returned null"))
    }

    pub(super) fn unmap(ptr: NonNull<u8>, len: usize) {
        // SAFETY: The pointer and length describe a mapping created by `map`.
        unsafe { munmap(ptr.as_ptr().cast(), len) };
    }
}

#[cfg(windows)]
mod platform {
    use std::os::windows::io::AsRawHandle;

    use super::*;

    const PAGE_READONLY: u32 = 0x02;
    const FILE_MAP_READ: u32 = 0x04;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateFileMappingW(
            file: *mut c_void,
            attributes: *const c_void,
            protect: u32,
            maximum_size_high: u32,
            maximum_size_low: u32,
            name: *const u16,
        ) -> *mut c_void;
        fn MapViewOfFile(
            mapping: *mut c_void,
            desired_access: u32,
            offset_high: u32,
            offset_low: u32,
            bytes: usize,
        ) -> *mut c_void;
        fn UnmapViewOfFile(base: *const c_void) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    pub(super) fn map(file: &File, len: usize) -> io::Result<NonNull<u8>> {
        // SAFETY: A read-only mapping object of an open file handle.
        let mapping = unsafe {
            CreateFileMappingW(
                file.as_raw_handle(),
                std::ptr::null(),
                PAGE_READONLY,
                0,
                0,
                std::ptr::null(),
            )
        };
        if mapping.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: Map the whole read-only mapping object. The view keeps the mapping alive, so
        // its handle is closed right away.
        let ptr = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, len) };
        let error = io::Error::last_os_error();
        // SAFETY: The handle was created above and is not used again.
        unsafe { CloseHandle(mapping) };
        NonNull::new(ptr.cast()).ok_or(error)
    }

    pub(super) fn unmap(ptr: NonNull<u8>, _: usize) {
        // SAFETY: The pointer is a view created by `map`.
        unsafe { UnmapViewOfFile(ptr.as_ptr().cast()) };
    }
}
