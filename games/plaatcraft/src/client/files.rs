/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! SD card and desktop files through the standard library.

use std::fs;
use std::io::ErrorKind;
use std::time::{SystemTime, UNIX_EPOCH};

/// A file of a directory, `modified` is when it was last written in seconds since 1970.
pub(super) struct Entry {
    pub(super) name: String,
    pub(super) modified: i64,
}

fn seconds(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs() as i64)
}

pub(super) fn unix_time() -> i64 {
    seconds(SystemTime::now())
}

pub(super) fn create_directories(directories: &[&str]) -> bool {
    directories
        .iter()
        .all(|directory| fs::create_dir_all(directory).is_ok())
}

pub(super) fn entries(directory: &str) -> Vec<Entry> {
    let Ok(directory) = fs::read_dir(directory) else {
        return Vec::new();
    };
    directory
        .flatten()
        .filter_map(|entry| {
            Some(Entry {
                name: entry.file_name().into_string().ok()?,
                modified: entry.metadata().ok()?.modified().map_or(0, seconds),
            })
        })
        .collect()
}

pub(super) fn exists(file: &str) -> bool {
    fs::exists(file).unwrap_or(false)
}

pub(super) fn move_file(from: &str, to: &str) -> bool {
    fs::rename(from, to).is_ok()
}

pub(super) fn remove(file: &str) -> bool {
    fs::remove_file(file).map_or_else(|error| error.kind() == ErrorKind::NotFound, |()| true)
}
