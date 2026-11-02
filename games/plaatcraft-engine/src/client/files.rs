/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The files of the SD card: picolibc's POSIX functions on the consoles, std's elsewhere.

use alloc::string::String;

/// A file of a directory, `modified` is when it was last written in seconds since 1970.
pub(super) struct Entry {
    pub(super) name: String,
    pub(super) modified: i64,
}

#[cfg(target_os = "none")]
pub(super) use picolibc::*;
#[cfg(not(target_os = "none"))]
pub(super) use standard::*;

#[cfg(target_os = "none")]
#[allow(unsafe_code)]
mod picolibc {
    use alloc::ffi::CString;
    use alloc::vec::Vec;
    use core::ffi::{CStr, c_char, c_int, c_long, c_void};
    use core::ptr;

    use super::Entry;

    // picolibc's directory entry and file status
    #[repr(C)]
    struct Dirent {
        inode: u64,
        kind: u8,
        name: [c_char; 256],
    }

    #[repr(C)]
    #[derive(Default)]
    struct Timespec {
        seconds: i64,
        nanoseconds: c_long,
    }

    #[repr(C)]
    #[derive(Default)]
    struct Stat {
        device: u64,
        inode: u64,
        mode: u32,
        links: u64,
        user: u32,
        group: u32,
        special: u64,
        size: i64,
        block_size: i32,
        blocks: i64,
        accessed: Timespec,
        modified: Timespec,
        changed: Timespec,
    }

    unsafe extern "C" {
        fn mkdir(path: *const c_char, mode: u32) -> c_int;
        fn access(path: *const c_char, mode: c_int) -> c_int;
        fn unlink(path: *const c_char) -> c_int;
        fn rename(from: *const c_char, to: *const c_char) -> c_int;
        fn stat(path: *const c_char, status: *mut Stat) -> c_int;
        fn opendir(path: *const c_char) -> *mut c_void;
        fn readdir(directory: *mut c_void) -> *mut Dirent;
        fn closedir(directory: *mut c_void) -> c_int;
        fn time(time: *mut i64) -> i64;
        static mut errno: c_int;
    }

    const ENOENT: c_int = 2;
    const EEXIST: c_int = 17;

    fn path(path: &str) -> CString {
        CString::new(path).expect("Paths have no NUL")
    }

    /// The current time in seconds since 1970, from the console's clock.
    pub(in super::super) fn unix_time() -> i64 {
        // SAFETY: time accepts a null pointer and only returns the time.
        unsafe { time(ptr::null_mut()) }
    }

    /// Creates the directories of the path that don't exist yet.
    pub(in super::super) fn create_directories(directories: &[&str]) -> bool {
        directories.iter().all(|directory| {
            let directory = path(directory);
            // SAFETY: The path is a valid C string and errno is only read on this thread
            // right after the call.
            unsafe { mkdir(directory.as_ptr(), 0o777) == 0 || errno == EEXIST }
        })
    }

    /// The files of a directory.
    pub(in super::super) fn entries(directory: &str) -> Vec<Entry> {
        let mut entries = Vec::new();
        let directory_path = path(directory);
        // SAFETY: The path is a valid C string.
        let handle = unsafe { opendir(directory_path.as_ptr()) };
        if handle.is_null() {
            return entries;
        }
        loop {
            // SAFETY: The directory is open, readdir returns null after the last entry.
            let entry = unsafe { readdir(handle) };
            if entry.is_null() {
                break;
            }
            // SAFETY: The entry stays valid until the next readdir, its name ends with a NUL.
            let name = unsafe { CStr::from_ptr((*entry).name.as_ptr()) };
            let Ok(name) = name.to_str() else {
                continue;
            };
            let mut status = Stat::default();
            let file = path(&alloc::format!("{directory}/{name}"));
            // SAFETY: The path is a valid C string and status is a picolibc stat.
            if unsafe { stat(file.as_ptr(), &mut status) } == 0 {
                entries.push(Entry {
                    name: name.into(),
                    modified: status.modified.seconds,
                });
            }
        }
        // SAFETY: The directory is open and not used after this.
        unsafe { closedir(handle) };
        entries
    }

    pub(in super::super) fn exists(file: &str) -> bool {
        let file = path(file);
        // SAFETY: The path is a valid C string.
        unsafe { access(file.as_ptr(), 0) == 0 }
    }

    pub(in super::super) fn move_file(from: &str, to: &str) -> bool {
        let (from, to) = (path(from), path(to));
        // SAFETY: The paths are valid C strings.
        unsafe { rename(from.as_ptr(), to.as_ptr()) == 0 }
    }

    /// Removes a file, a file that doesn't exist counts as removed.
    pub(in super::super) fn remove(file: &str) -> bool {
        let file = path(file);
        // SAFETY: The path is a valid C string and errno is only read on this thread right
        // after the call.
        unsafe { unlink(file.as_ptr()) == 0 || errno == ENOENT }
    }
}

#[cfg(not(target_os = "none"))]
mod standard {
    use alloc::vec::Vec;
    use std::fs;
    use std::io::ErrorKind;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::Entry;

    fn seconds(time: SystemTime) -> i64 {
        time.duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs() as i64)
    }

    pub(in super::super) fn unix_time() -> i64 {
        seconds(SystemTime::now())
    }

    pub(in super::super) fn create_directories(directories: &[&str]) -> bool {
        directories
            .iter()
            .all(|directory| fs::create_dir_all(directory).is_ok())
    }

    pub(in super::super) fn entries(directory: &str) -> Vec<Entry> {
        let Ok(directory) = fs::read_dir(directory) else {
            return Vec::new();
        };
        directory
            .flatten()
            .filter_map(|entry| {
                Some(Entry {
                    name: entry.file_name().into_string().ok()?,
                    modified: seconds(entry.metadata().ok()?.modified().ok()?),
                })
            })
            .collect()
    }

    pub(in super::super) fn exists(file: &str) -> bool {
        fs::exists(file).unwrap_or(false)
    }

    pub(in super::super) fn move_file(from: &str, to: &str) -> bool {
        fs::rename(from, to).is_ok()
    }

    pub(in super::super) fn remove(file: &str) -> bool {
        fs::remove_file(file).map_or_else(|error| error.kind() == ErrorKind::NotFound, |()| true)
    }
}
