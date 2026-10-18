/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use super::CleanupResult;
use crate::catalog::PathRule;
use crate::pattern;

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
    let cutoff = cutoff_for_days(days);
    let mut seen = HashSet::new();
    for root in roots {
        if !root.exists() || !seen.insert(root.clone()) {
            continue;
        }
        delete_old_path(root, cutoff, false, result);
    }
}

fn delete_old_path(
    path: &Path,
    cutoff: SystemTime,
    remove_empty: bool,
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
        match metadata.modified() {
            Ok(modified) if modified <= cutoff => delete_file(path, &metadata, result),
            Ok(_) => {}
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
                Ok(entry) => delete_old_path(&entry.path(), cutoff, true, result),
                Err(error) => {
                    result.skipped += 1;
                    result.errors.push(error.to_string());
                }
            }
        }
        if remove_empty
            && fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none())
            && let Err(error) = fs::remove_dir(path)
        {
            result.errors.push(format!("{}: {error}", path.display()));
        }
    }
}

pub(super) fn clean_roots(roots: &[PathBuf], result: &mut CleanupResult) {
    let mut seen = HashSet::new();
    for root in roots {
        if !root.exists() || !seen.insert(root.clone()) {
            continue;
        }
        let metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(error) => {
                result.errors.push(format!("{}: {error}", root.display()));
                continue;
            }
        };
        if is_reparse(&metadata) {
            result.skipped += 1;
            result
                .errors
                .push(format!("Skipped reparse point {}", root.display()));
            continue;
        }
        if metadata.is_file() {
            delete_file(root, &metadata, result);
            continue;
        }
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(error) => {
                result.errors.push(format!("{}: {error}", root.display()));
                continue;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => delete_tree(&entry.path(), result),
                Err(error) => {
                    result.skipped += 1;
                    result.errors.push(error.to_string());
                }
            }
        }
    }
}

fn delete_tree(path: &Path, result: &mut CleanupResult) {
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
        delete_file(path, &metadata, result);
    } else if metadata.is_dir() {
        let mut before = ScanStats::default();
        scan_path(path, &AtomicBool::new(false), &mut before);

        // On Windows, std::fs::remove_dir_all opens children relative to their
        // parent handles with FILE_OPEN_REPARSE_POINT and OBJ_DONT_REPARSE. This
        // prevents a concurrent junction swap from redirecting elevated cleanup.
        match fs::remove_dir_all(path) {
            Ok(()) => {
                result.cleaned_files += before.files;
                result.cleaned_bytes += before.bytes;
            }
            Err(error) => {
                let mut remaining = ScanStats::default();
                scan_path(path, &AtomicBool::new(false), &mut remaining);
                result.cleaned_files += before.files.saturating_sub(remaining.files);
                result.cleaned_bytes += before.bytes.saturating_sub(remaining.bytes);
                result.skipped += remaining.files.max(1);
                result.errors.push(format!("{}: {error}", path.display()));
            }
        }
    }
}

fn delete_file(path: &Path, metadata: &fs::Metadata, result: &mut CleanupResult) {
    match fs::remove_file(path) {
        Ok(()) => {
            result.cleaned_files += 1;
            result.cleaned_bytes += metadata.len();
        }
        Err(error) => {
            result.skipped += 1;
            result.errors.push(format!("{}: {error}", path.display()));
        }
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
