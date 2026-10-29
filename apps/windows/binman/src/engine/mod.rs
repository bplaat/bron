/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::OsStr;
use std::io::{self, Read};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::catalog::{CleanupDefinition, CleanupKind, PathRoot};
use crate::engine::filesystem::{clean_roots, clean_rules, scan_roots, scan_rules};
pub(crate) use crate::engine::system::disk_free_space;
use crate::engine::system::{
    clean_delivery_optimization, clean_windows_update, current_user_sid, process_is_running,
    run_dism_cleanup, scan_dism,
};
use crate::engine::tools::{
    clean_docker_build_cache, clean_uv_cache, scan_docker_build_cache, scan_uv_cache,
};

mod filesystem;
mod rustup;
mod system;
mod tools;

pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut command = Command::new(program);
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

pub(crate) fn cancellable_output(
    command: &mut Command,
    cancelled: &AtomicBool,
) -> io::Result<Option<Output>> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let mut stdout = child.stdout.take().expect("stdout should be piped");
    let mut stderr = child.stderr.take().expect("stderr should be piped");
    let stdout_reader = thread::Builder::new()
        .name("process-stdout-reader".to_string())
        .spawn(move || {
            let mut bytes = Vec::new();
            stdout.read_to_end(&mut bytes).map(|_| bytes)
        })?;
    let stderr_reader = thread::Builder::new()
        .name("process-stderr-reader".to_string())
        .spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).map(|_| bytes)
        })?;

    loop {
        if cancelled.load(Ordering::Acquire) {
            _ = child.kill();
            _ = child.wait();
            _ = stdout_reader.join();
            _ = stderr_reader.join();
            return Ok(None);
        }
        if let Some(status) = child.try_wait()? {
            let stdout = stdout_reader
                .join()
                .map_err(|_| io::Error::other("stdout reader panicked"))??;
            let stderr = stderr_reader
                .join()
                .map_err(|_| io::Error::other("stderr reader panicked"))??;
            return Ok(Some(Output {
                status,
                stdout,
                stderr,
            }));
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CleanupResult {
    pub(crate) id: String,
    pub(crate) available: bool,
    pub(crate) files: u64,
    pub(crate) bytes: u64,
    pub(crate) unknown_size: bool,
    pub(crate) cleaned_files: u64,
    pub(crate) cleaned_bytes: u64,
    pub(crate) skipped: u64,
    pub(crate) errors: Vec<String>,
}

pub(crate) fn scan(
    cleanup: &CleanupDefinition,
    cancelled: &AtomicBool,
    is_elevated: bool,
) -> CleanupResult {
    let mut result = CleanupResult {
        id: cleanup.id.clone(),
        ..CleanupResult::default()
    };

    match &cleanup.kind {
        CleanupKind::Paths { rules } => scan_rules(rules, cancelled, &mut result),
        CleanupKind::RecycleBin => {
            if let (Some(root), Some(sid)) = (PathRoot::SystemDrive.resolve(), current_user_sid()) {
                scan_roots(
                    &[root.join("$Recycle.Bin").join(sid)],
                    cancelled,
                    &mut result,
                );
            }
        }
        CleanupKind::WindowsUpdate => {
            if let Some(root) = PathRoot::Windows.resolve() {
                scan_roots(
                    &[root.join("SoftwareDistribution").join("Download")],
                    cancelled,
                    &mut result,
                );
            }
        }
        CleanupKind::DeliveryOptimization => {
            if let Some(root) = PathRoot::Windows.resolve() {
                scan_roots(
                    &[root.join("ServiceProfiles/NetworkService/AppData/Local/Microsoft/Windows/DeliveryOptimization/Cache")],
                    cancelled,
                    &mut result,
                );
            }
        }
        CleanupKind::DismComponentStore => scan_dism(cancelled, &mut result),
        CleanupKind::DockerBuildCache if !is_elevated => {
            scan_docker_build_cache(cancelled, &mut result);
        }
        CleanupKind::RustToolchains if !is_elevated => rustup::scan(cancelled, &mut result),
        CleanupKind::UvCache if !is_elevated => scan_uv_cache(cancelled, &mut result),
        CleanupKind::DockerBuildCache | CleanupKind::RustToolchains | CleanupKind::UvCache => {}
    }
    result
}

pub(crate) fn clean(cleanup: &CleanupDefinition, is_elevated: bool) -> CleanupResult {
    let mut result = CleanupResult {
        id: cleanup.id.clone(),
        available: true,
        ..CleanupResult::default()
    };

    let mut running = Vec::new();
    for name in &cleanup.process_names {
        match process_is_running(name) {
            Some(true) => running.push(name),
            Some(false) => {}
            None => {
                result.skipped = 1;
                result.errors.push(format!(
                    "Could not verify whether {name} is running; cleanup was skipped"
                ));
                return result;
            }
        }
    }
    if !running.is_empty() {
        result.skipped = 1;
        result.errors.push(format!(
            "Close {} before cleaning this cache",
            running
                .iter()
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        return result;
    }

    match &cleanup.kind {
        CleanupKind::Paths { rules } => clean_rules(rules, &mut result),
        CleanupKind::RecycleBin => {
            if let (Some(root), Some(sid)) = (PathRoot::SystemDrive.resolve(), current_user_sid()) {
                clean_roots(&[root.join("$Recycle.Bin").join(sid)], &mut result);
            }
        }
        CleanupKind::WindowsUpdate => clean_windows_update(&mut result),
        CleanupKind::DeliveryOptimization => clean_delivery_optimization(&mut result),
        CleanupKind::DismComponentStore => run_dism_cleanup(&mut result),
        CleanupKind::DockerBuildCache if !is_elevated => clean_docker_build_cache(&mut result),
        CleanupKind::RustToolchains if !is_elevated => rustup::clean(&mut result),
        CleanupKind::DockerBuildCache | CleanupKind::RustToolchains => result
            .errors
            .push("User-installed tool cleanup is disabled while Binman is elevated".to_string()),
        CleanupKind::UvCache if !is_elevated => clean_uv_cache(&mut result),
        CleanupKind::UvCache => result
            .errors
            .push("uv cleanup is disabled while Binman is elevated".to_string()),
    }
    result
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::filesystem::{is_within, scan_roots};
    use super::system::{parse_reclaimable_packages, parse_service_state, system_executable};
    use super::tools::parse_reclaimed_size;
    use super::*;

    #[test]
    fn parses_dism_package_count() {
        let output = "Number of Reclaimable Packages : 10\r\n";
        assert_eq!(parse_reclaimable_packages(output), Some(10));
    }

    #[test]
    fn parses_external_reclaimed_sizes() {
        assert_eq!(
            parse_reclaimed_size("Total reclaimed space: 1.25GB\n"),
            Some(1_250_000_000)
        );
        assert_eq!(parse_reclaimed_size("Total: 42.5MB\n"), Some(42_500_000));
        assert_eq!(parse_reclaimed_size("No size here"), None);
    }

    #[test]
    fn parses_service_state_after_service_type() {
        let output =
            "TYPE : 20 WIN32_SHARE_PROCESS\r\nSTATE : 4 RUNNING\r\nWIN32_EXIT_CODE : 0\r\n";
        assert_eq!(parse_service_state(output), Some(4));
    }

    #[test]
    fn rust_toolchain_cleanup_keeps_latest_stable_and_nightly() {
        let installed = [
            "1.84.1-x86_64-pc-windows-msvc",
            "1.90.0-x86_64-pc-windows-msvc",
            "nightly-2025-01-01-x86_64-pc-windows-msvc",
            "nightly-2026-08-20-x86_64-pc-windows-msvc",
            "beta-x86_64-pc-windows-msvc",
        ]
        .map(str::to_string);
        assert_eq!(
            rustup::toolchains_to_remove_except(&installed, &HashSet::new()),
            [
                "1.84.1-x86_64-pc-windows-msvc",
                "nightly-2025-01-01-x86_64-pc-windows-msvc",
                "beta-x86_64-pc-windows-msvc",
            ]
        );
    }

    #[test]
    fn rust_toolchain_cleanup_prefers_tracking_channels() {
        let installed = [
            "stable-x86_64-pc-windows-msvc",
            "1.90.0-x86_64-pc-windows-msvc",
            "nightly-x86_64-pc-windows-msvc",
            "nightly-2026-08-20-x86_64-pc-windows-msvc",
        ]
        .map(str::to_string);
        assert_eq!(
            rustup::toolchains_to_remove_except(&installed, &HashSet::new()),
            [
                "1.90.0-x86_64-pc-windows-msvc",
                "nightly-2026-08-20-x86_64-pc-windows-msvc",
            ]
        );
    }

    #[test]
    fn rust_toolchain_cleanup_never_removes_active_or_default_toolchains() {
        let installed = [
            "stable-x86_64-pc-windows-msvc",
            "nightly-x86_64-pc-windows-msvc",
            "1.84.1-x86_64-pc-windows-msvc",
        ]
        .map(str::to_string);
        let protected = HashSet::from(["1.84.1-x86_64-pc-windows-msvc".to_string()]);
        assert!(rustup::toolchains_to_remove_except(&installed, &protected).is_empty());
    }

    #[test]
    fn rust_toolchain_cleanup_supports_fixed_project_toolchains() {
        let installed = [
            "stable-x86_64-pc-windows-msvc",
            "1.98-aarch64-pc-windows-msvc",
        ]
        .map(str::to_string);
        let mut protected = HashSet::new();
        rustup::protect_channel("1.98", &installed, &mut protected);
        assert!(rustup::toolchains_to_remove_except(&installed, &protected).is_empty());
        assert_eq!(
            rustup::toolchain_channel("[toolchain]\nprofile = \"default\"\nchannel = \"1.98\"\n"),
            Some("1.98".to_string())
        );
        assert_eq!(
            rustup::toolchain_channel("# pinned\n[toolchain]\nchannel = \"nightly\" # comment\n"),
            Some("nightly".to_string())
        );
        assert_eq!(
            rustup::toolchain_channel("stable\n"),
            Some("stable".to_string())
        );
        assert_eq!(
            rustup::toolchain_channel("[other]\nchannel = \"1.98\"\n"),
            None
        );
    }

    #[test]
    fn trusted_system_tool_paths_reject_parent_components() {
        assert!(system_executable("../evil.exe").is_none());
    }

    #[test]
    fn containment_rejects_siblings() {
        assert!(is_within(
            Path::new("C:\\Root"),
            Path::new("C:\\Root\\Cache")
        ));
        assert!(!is_within(Path::new("C:\\Root"), Path::new("C:\\Other")));
    }

    #[test]
    fn scanning_is_read_only_and_cleaning_keeps_the_root() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("binman-test-{unique}"));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("fixture directory should be created");
        fs::write(nested.join("cache.bin"), b"cache").expect("fixture file should be created");

        let mut scan_result = CleanupResult::default();
        scan_roots(
            std::slice::from_ref(&root),
            &AtomicBool::new(false),
            &mut scan_result,
        );
        assert_eq!(scan_result.files, 1);
        assert_eq!(scan_result.bytes, 5);
        assert!(nested.join("cache.bin").exists());

        let mut clean_result = CleanupResult::default();
        clean_roots(std::slice::from_ref(&root), &mut clean_result);
        assert!(root.exists());
        assert!(
            fs::read_dir(&root)
                .expect("fixture root should remain")
                .next()
                .is_none()
        );
        assert_eq!(clean_result.cleaned_files, 1);
        fs::remove_dir(&root).expect("empty fixture root should be removable");
    }
}
