/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Command-line behavior for the unzip binary.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn temp_directory() -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "unzip-test-{}-{unique}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("create test directory");
    path
}

#[test]
fn lists_and_extracts_without_overwriting() {
    let root = temp_directory();
    let archive = root.join("nested.zip");
    fs::write(&archive, include_bytes!("fixtures/nested.zip")).expect("write archive");
    let list = Command::new(env!("CARGO_BIN_EXE_unzip"))
        .arg("-l")
        .arg(&archive)
        .output()
        .expect("list archive");
    assert!(list.status.success());
    assert_eq!(list.stdout, b"folder/hello.txt\n");

    let destination = root.join("out");
    let extract = || {
        Command::new(env!("CARGO_BIN_EXE_unzip"))
            .arg(&archive)
            .arg("-d")
            .arg(&destination)
            .output()
            .expect("extract archive")
    };
    assert!(extract().status.success());
    assert_eq!(
        fs::read(destination.join("folder/hello.txt")).expect("extracted file"),
        b"hello world"
    );
    assert!(!extract().status.success());
    fs::remove_dir_all(root).expect("remove test directory");
}

#[test]
fn rejects_absolute_archive_paths() {
    let root = temp_directory();
    let archive = root.join("absolute.zip");
    fs::write(&archive, include_bytes!("fixtures/absolute.zip")).expect("write archive");
    let output = Command::new(env!("CARGO_BIN_EXE_unzip"))
        .arg(&archive)
        .arg("-d")
        .arg(root.join("out"))
        .output()
        .expect("extract archive");
    assert!(!output.status.success());
    fs::remove_dir_all(root).expect("remove test directory");
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_directory() {
    use std::os::unix::fs::symlink;

    let root = temp_directory();
    let archive = root.join("nested.zip");
    fs::write(&archive, include_bytes!("fixtures/nested.zip")).expect("write archive");
    let destination = root.join("out");
    fs::create_dir(&destination).expect("create destination");
    symlink(&root, destination.join("folder")).expect("create symlink");
    let output = Command::new(env!("CARGO_BIN_EXE_unzip"))
        .arg(&archive)
        .arg("-d")
        .arg(&destination)
        .output()
        .expect("extract archive");
    assert!(!output.status.success());
    assert!(!root.join("hello.txt").exists());
    fs::remove_dir_all(root).expect("remove test directory");
}

fn extract_fixture(bytes: &[u8], root: &std::path::Path) -> std::process::Output {
    let archive = root.join("archive.zip");
    fs::write(&archive, bytes).expect("write archive");
    Command::new(env!("CARGO_BIN_EXE_unzip"))
        .env("TZ", "UTC")
        .arg(archive)
        .arg("-d")
        .arg(root.join("out"))
        .output()
        .expect("extract archive")
}

#[cfg(unix)]
#[test]
fn restores_unix_modes_timestamps_and_safe_symlinks() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_directory();
    let output = extract_fixture(include_bytes!("fixtures/unix.zip"), &root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = root.join("out");
    let file = fs::metadata(out.join("bin/run.sh")).expect("file metadata");
    assert_eq!(file.permissions().mode() & 0o7777, 0o755);
    assert_eq!(
        file.modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1700000002
    );
    let directory = fs::metadata(out.join("bin")).expect("directory metadata");
    assert_eq!(directory.permissions().mode() & 0o7777, 0o500);
    assert_eq!(
        directory
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1700000001
    );
    assert_eq!(
        fs::read_link(out.join("shortcut")).unwrap(),
        PathBuf::from("bin/run.sh")
    );
    assert_eq!(
        fs::read_link(out.join("chain")).unwrap(),
        PathBuf::from("shortcut")
    );
    assert_eq!(
        fs::read(out.join("bin/parent-link")).unwrap(),
        b"#!/bin/sh\nexit 0\n"
    );
    let link = fs::symlink_metadata(out.join("shortcut")).unwrap();
    assert!(link.file_type().is_symlink());
    assert_eq!(
        link.modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        1700000003
    );
    fs::set_permissions(out.join("bin"), fs::Permissions::from_mode(0o700)).unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restores_dos_local_and_ntfs_timestamps() {
    #[cfg(unix)]
    let dos_seconds = 1582979696;
    // Windows ignores TZ and always uses the system time zone.
    #[cfg(windows)]
    let dos_seconds = {
        use chrono::{Local, TimeZone};
        let local = Local.with_ymd_and_hms(2020, 2, 29, 12, 34, 56);
        local.single().unwrap().timestamp() as u64
    };
    for (bytes, seconds, nanos) in [
        (
            include_bytes!("fixtures/dos.zip").as_slice(),
            dos_seconds,
            0,
        ),
        (
            include_bytes!("fixtures/ntfs.zip").as_slice(),
            1700000000,
            123456700,
        ),
    ] {
        let root = temp_directory();
        let output = extract_fixture(bytes, &root);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let time = fs::metadata(root.join("out/file"))
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap();
        assert_eq!(time.as_secs(), seconds);
        // Some filesystems round subsecond timestamps.
        assert!(time.subsec_nanos().abs_diff(nanos) < 1_000_000);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn rejects_escaping_links_cycles_and_special_files() {
    for bytes in [
        include_bytes!("fixtures/escape-link.zip").as_slice(),
        include_bytes!("fixtures/absolute-link.zip").as_slice(),
        include_bytes!("fixtures/cycle-links.zip").as_slice(),
        include_bytes!("fixtures/special.zip").as_slice(),
    ] {
        let root = temp_directory();
        let output = extract_fixture(bytes, &root);
        assert!(!output.status.success());
        assert!(fs::read_dir(root.join("out")).unwrap().next().is_none());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn rejects_targets_through_existing_escaping_symlinks() {
    use std::os::unix::fs::symlink;

    let root = temp_directory();
    fs::create_dir(root.join("out")).unwrap();
    symlink(&root, root.join("out/outside")).unwrap();
    let output = extract_fixture(include_bytes!("fixtures/existing-link-target.zip"), &root);
    assert!(!output.status.success());
    assert!(!root.join("out/link").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn extracts_streamed_archives_with_data_descriptors() {
    let root = temp_directory();
    let output = extract_fixture(include_bytes!("fixtures/descriptor.zip"), &root);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read(root.join("out/streamed.txt")).unwrap(),
        b"streamed data".repeat(100)
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn interprets_dos_timestamps_in_the_local_timezone() {
    let root = temp_directory();
    let archive = root.join("dos.zip");
    fs::write(&archive, include_bytes!("fixtures/dos.zip")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_unzip"))
        .env("TZ", "UTC-2")
        .arg(archive)
        .arg("-d")
        .arg(root.join("out"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let time = fs::metadata(root.join("out/file"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        time.duration_since(UNIX_EPOCH).unwrap().as_secs(),
        1582972496
    );
    fs::remove_dir_all(root).unwrap();
}
