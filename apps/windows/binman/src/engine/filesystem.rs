/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use std::{fs, io};

use super::CleanupResult;
use crate::catalog::PathRule;
use crate::pattern;
use crate::win32::*;

#[derive(Default)]
pub(super) struct ScanStats {
    pub(super) files: u64,
    pub(super) bytes: u64,
    errors: Vec<String>,
}

pub(super) fn scan_rules(rules: &[PathRule], cancelled: &AtomicBool, result: &mut CleanupResult) {
    for rule in rules {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }
        match expand_rule(rule) {
            Ok(roots) => match rule.minimum_age_days {
                Some(days) => scan_old_roots(&roots, days, cancelled, result),
                None => scan_roots(&roots, cancelled, result),
            },
            Err(error) => result.errors.push(error),
        }
    }
}

pub(super) fn clean_rules(rules: &[PathRule], result: &mut CleanupResult) {
    for rule in rules {
        match expand_rule(rule) {
            Ok(roots) => match rule.minimum_age_days {
                Some(days) => clean_old_roots(&roots, days, result),
                None => clean_roots(&roots, result),
            },
            Err(error) => result.errors.push(error),
        }
    }
}

fn expand_rule(rule: &PathRule) -> Result<Vec<PathBuf>, String> {
    let base = rule
        .root
        .resolve()
        .ok_or_else(|| format!("Environment root is unavailable for {}", rule.path))?;
    let paths = pattern::expand(&base, &rule.path).map_err(|error| error.to_string())?;
    if let Some(path) = paths
        .iter()
        .find(|path| !is_safe_resolved_path(&base, path))
    {
        return Err(format!(
            "Skipped unsafe path outside or linked through {}: {}",
            base.display(),
            path.display()
        ));
    }
    Ok(paths)
}

pub(super) fn scan_roots(roots: &[PathBuf], cancelled: &AtomicBool, result: &mut CleanupResult) {
    let mut seen = HashSet::new();
    for root in roots {
        if cancelled.load(Ordering::Relaxed) || !root.exists() || !seen.insert(root.clone()) {
            continue;
        }
        result.available = true;
        let mut stats = ScanStats::default();
        scan_path(root, cancelled, &mut stats);
        result.files += stats.files;
        result.bytes += stats.bytes;
        result.errors.extend(stats.errors);
    }
}

pub(super) fn scan_path(path: &Path, cancelled: &AtomicBool, stats: &mut ScanStats) {
    scan_path_before(path, None, cancelled, stats);
}

fn scan_path_before(
    path: &Path,
    cutoff: Option<SystemTime>,
    cancelled: &AtomicBool,
    stats: &mut ScanStats,
) {
    if cancelled.load(Ordering::Relaxed) {
        return;
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            stats.errors.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    if is_reparse(&metadata) {
        stats
            .errors
            .push(format!("Skipped reparse point {}", path.display()));
        return;
    }
    if metadata.is_file() {
        match cutoff {
            Some(cutoff) => match metadata.modified() {
                Ok(modified) if modified <= cutoff => {
                    stats.files += 1;
                    stats.bytes += metadata.len();
                }
                Ok(_) => {}
                Err(error) => stats.errors.push(format!("{}: {error}", path.display())),
            },
            None => {
                stats.files += 1;
                stats.bytes += metadata.len();
            }
        }
        return;
    }
    if !metadata.is_dir() {
        return;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            stats.errors.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    for entry in entries {
        match entry {
            Ok(entry) => scan_path_before(&entry.path(), cutoff, cancelled, stats),
            Err(error) => stats.errors.push(error.to_string()),
        }
    }
}

fn cutoff_for_days(days: u64) -> SystemTime {
    SystemTime::now()
        .checked_sub(Duration::from_secs(days.saturating_mul(24 * 60 * 60)))
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn scan_old_roots(
    roots: &[PathBuf],
    days: u64,
    cancelled: &AtomicBool,
    result: &mut CleanupResult,
) {
    let cutoff = cutoff_for_days(days);
    let mut seen = HashSet::new();
    for root in roots {
        if cancelled.load(Ordering::Relaxed) || !root.exists() || !seen.insert(root.clone()) {
            continue;
        }
        result.available = true;
        let mut stats = ScanStats::default();
        scan_path_before(root, Some(cutoff), cancelled, &mut stats);
        result.files += stats.files;
        result.bytes += stats.bytes;
        result.errors.extend(stats.errors);
    }
}

fn clean_old_roots(roots: &[PathBuf], days: u64, result: &mut CleanupResult) {
    clean_paths(roots, Some(cutoff_for_days(days)), result);
}

pub(super) fn clean_roots(roots: &[PathBuf], result: &mut CleanupResult) {
    clean_paths(roots, None, result);
}

// Deletes the files in the root directories and file roots themselves, cleanup can run elevated
// in user writable folders, so every delete goes through a handle whose final path must still be
// in the canonical root, which a junction swapped into the tree during the walk can't redirect
fn clean_paths(roots: &[PathBuf], cutoff: Option<SystemTime>, result: &mut CleanupResult) {
    let mut seen = HashSet::new();
    for root in roots {
        if !root.exists() || !seen.insert(root.clone()) {
            continue;
        }
        match fs::canonicalize(root) {
            Ok(base) => delete_path(root, &base, cutoff, false, result),
            Err(error) => result.errors.push(format!("{}: {error}", root.display())),
        }
    }
}

fn delete_path(
    path: &Path,
    base: &Path,
    cutoff: Option<SystemTime>,
    remove_dir: bool,
    result: &mut CleanupResult,
) {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            result.skipped += 1;
            result.errors.push(format!("{}: {error}", path.display()));
            return;
        }
    };
    if is_reparse(&metadata) {
        result.skipped += 1;
        result
            .errors
            .push(format!("Skipped reparse point {}", path.display()));
    } else if metadata.is_file() {
        let is_old = match cutoff.map(|cutoff| metadata.modified().map(|time| time <= cutoff)) {
            Some(Ok(is_old)) => is_old,
            Some(Err(error)) => {
                result.skipped += 1;
                result.errors.push(format!("{}: {error}", path.display()));
                return;
            }
            None => true,
        };
        if !is_old {
            return;
        }
        match delete_verified(path, base) {
            Ok(()) => {
                result.cleaned_files += 1;
                result.cleaned_bytes += metadata.len();
            }
            Err(error) => {
                result.skipped += 1;
                result.errors.push(format!("{}: {error}", path.display()));
            }
        }
    } else if metadata.is_dir() {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) => {
                result.skipped += 1;
                result.errors.push(format!("{}: {error}", path.display()));
                return;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => delete_path(&entry.path(), base, cutoff, true, result),
                Err(error) => {
                    result.skipped += 1;
                    result.errors.push(error.to_string());
                }
            }
        }
        if remove_dir
            && fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
            && let Err(error) = delete_verified(path, base)
        {
            result.errors.push(format!("{}: {error}", path.display()));
        }
    }
}

// Deletes a file or empty directory through a handle that doesn't follow a final reparse point,
// after checking that the handle is still in the base
fn delete_verified(path: &Path, base: &Path) -> io::Result<()> {
    let file = fs::OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    if is_reparse(&file.metadata()?) || !is_within(base, &final_path(&file)?) {
        return Err(io::Error::other("path moved outside the cleanup root"));
    }
    delete_on_close(&file)
}

#[allow(unsafe_code)]
fn final_path(file: &fs::File) -> io::Result<PathBuf> {
    let mut buffer = vec![0; 512];
    loop {
        // SAFETY: `file` owns an open handle and the buffer is writable for its length.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
                0,
            )
        } as usize;
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length < buffer.len() {
            return Ok(PathBuf::from(OsString::from_wide(&buffer[..length])));
        }
        buffer.resize(length, 0);
    }
}

#[allow(unsafe_code)]
fn delete_on_close(file: &fs::File) -> io::Result<()> {
    // POSIX semantics remove the name at once and also delete read-only files, like remove_dir_all
    let flags = FILE_DISPOSITION_FLAG_DELETE
        | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
        | FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE;
    // SAFETY: `file` owns a handle opened with DELETE access and `flags` has the layout of
    // FILE_DISPOSITION_INFO_EX.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FILE_DISPOSITION_INFO_EX_CLASS,
            (&raw const flags).cast(),
            size_of::<u32>() as u32,
        )
    } != 0
    {
        return Ok(());
    }
    // File systems like FAT only support the plain disposition
    let delete = 1u8;
    // SAFETY: `file` owns a handle opened with DELETE access and `delete` has the layout of
    // FILE_DISPOSITION_INFO.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FILE_DISPOSITION_INFO_CLASS,
            (&raw const delete).cast(),
            size_of::<u8>() as u32,
        )
    } != 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

pub(super) fn is_within(base: &Path, candidate: &Path) -> bool {
    candidate.starts_with(base)
}

fn is_safe_resolved_path(base: &Path, candidate: &Path) -> bool {
    if !is_within(base, candidate) {
        return false;
    }
    let (Ok(canonical_base), Ok(canonical_candidate)) =
        (fs::canonicalize(base), fs::canonicalize(candidate))
    else {
        return false;
    };
    if !is_within(&canonical_base, &canonical_candidate) {
        return false;
    }
    candidate
        .ancestors()
        .take_while(|path| is_within(base, path))
        .all(|path| fs::symlink_metadata(path).is_ok_and(|metadata| !is_reparse(&metadata)))
}

pub(super) fn is_reparse(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn age_filtered_cleanup_removes_only_old_files() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("binman-age-test-{unique}"));
        let recent_file = root.join("recent.tmp");
        let old_file = root.join("old.tmp");
        let mixed_directory = root.join("mixed");
        let mixed_old_file = mixed_directory.join("old.tmp");
        let mixed_recent_file = mixed_directory.join("recent.tmp");
        fs::create_dir(&root).expect("fixture directory should be created");
        fs::create_dir(&mixed_directory).expect("nested fixture directory should be created");
        for file in [&recent_file, &old_file, &mixed_old_file, &mixed_recent_file] {
            fs::write(file, b"temporary").expect("fixture file should be created");
        }
        let old_time = SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60);
        for file in [&old_file, &mixed_old_file] {
            fs::File::options()
                .write(true)
                .open(file)
                .expect("fixture file should open")
                .set_times(fs::FileTimes::new().set_modified(old_time))
                .expect("fixture modification time should be set");
        }

        let mut scan_result = CleanupResult::default();
        scan_old_roots(
            std::slice::from_ref(&root),
            7,
            &AtomicBool::new(false),
            &mut scan_result,
        );
        assert_eq!(scan_result.files, 2);
        assert_eq!(scan_result.bytes, 18);

        let mut clean_result = CleanupResult::default();
        clean_old_roots(std::slice::from_ref(&root), 7, &mut clean_result);
        assert!(recent_file.exists());
        assert!(!old_file.exists());
        assert!(!mixed_old_file.exists());
        assert!(mixed_recent_file.exists());
        assert!(root.exists());
        fs::remove_dir_all(root).expect("fixture directory should be removable");
    }

    #[test]
    fn verified_delete_refuses_paths_outside_the_root() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("binman-root-test-{unique}"));
        let outside = std::env::temp_dir().join(format!("binman-outside-test-{unique}.tmp"));
        fs::create_dir(&root).expect("fixture directory should be created");
        fs::write(&outside, b"keep").expect("fixture file should be created");

        let base = fs::canonicalize(&root).expect("fixture directory should resolve");
        assert!(delete_verified(&outside, &base).is_err());
        assert!(outside.exists());

        fs::remove_file(outside).expect("fixture file should be removable");
        fs::remove_dir(root).expect("fixture directory should be removable");
    }

    #[test]
    fn age_filtered_scan_supports_file_roots() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let file = std::env::temp_dir().join(format!("binman-age-file-test-{unique}.tmp"));
        fs::write(&file, b"temporary").expect("fixture file should be created");
        let old_time = SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60);
        fs::File::options()
            .write(true)
            .open(&file)
            .expect("fixture file should open")
            .set_times(fs::FileTimes::new().set_modified(old_time))
            .expect("fixture modification time should be set");

        let mut result = CleanupResult::default();
        scan_old_roots(
            std::slice::from_ref(&file),
            7,
            &AtomicBool::new(false),
            &mut result,
        );

        assert!(result.available);
        assert_eq!(result.files, 1);
        assert_eq!(result.bytes, 9);
        assert!(result.errors.is_empty());
        fs::remove_file(file).expect("fixture file should be removable");
    }
}
