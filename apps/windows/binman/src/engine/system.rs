/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use std::{fs, thread};

use super::filesystem::{clean_roots, is_reparse};
use super::{CleanupResult, cancellable_output, command};
use crate::catalog::PathRoot;

#[allow(unsafe_code)]
fn system_directory() -> Option<PathBuf> {
    use crate::headers::GetSystemDirectoryW;

    let mut buffer = vec![0_u16; 260];
    loop {
        // SAFETY: `buffer` is writable for the length passed to the API.
        let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
        if length == 0 {
            return None;
        }
        if (length as usize) < buffer.len() {
            buffer.truncate(length as usize);
            return Some(PathBuf::from(String::from_utf16_lossy(&buffer)));
        }
        buffer.resize(length as usize + 1, 0);
    }
}

pub(super) fn clean_windows_update(result: &mut CleanupResult) {
    let Some(windows) = PathRoot::Windows.resolve() else {
        result
            .errors
            .push("Windows directory is unavailable".to_string());
        return;
    };
    let target = windows.join("SoftwareDistribution").join("Download");
    let mut stopped = Vec::new();
    let mut ready = true;
    for name in ["bits", "wuauserv"] {
        match service_state(name) {
            Some(1) => {}
            Some(4) => {
                if service_control("stop", name) && wait_for_service_state(name, 1) {
                    stopped.push(name);
                } else {
                    result
                        .errors
                        .push(format!("Could not stop the {name} service"));
                    ready = false;
                    break;
                }
            }
            Some(_) => {
                result.errors.push(format!("The {name} service is busy"));
                ready = false;
                break;
            }
            None => {
                result
                    .errors
                    .push(format!("Could not query the {name} service"));
                ready = false;
                break;
            }
        }
    }
    if ready {
        clean_roots(&[target], result);
    }
    for name in stopped {
        if !service_control("start", name) || !wait_for_service_state(name, 4) {
            result
                .errors
                .push(format!("Could not restart the {name} service"));
        }
    }
}

pub(crate) fn system_executable(relative_path: &str) -> Option<PathBuf> {
    if Path::new(relative_path).is_absolute()
        || Path::new(relative_path)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return None;
    }
    let path = system_directory()?.join(relative_path);
    trusted_executable(path)
}

pub(super) fn docker_executable() -> Option<PathBuf> {
    trusted_executable(
        PathRoot::ProgramFiles
            .resolve()?
            .join("Docker/Docker/resources/bin/docker.exe"),
    )
}

pub(super) fn trusted_executable(path: PathBuf) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(&path).ok()?;
    (metadata.is_file() && !is_reparse(&metadata)).then_some(path)
}

pub(super) fn process_is_running(name: &str) -> Option<bool> {
    let executable = system_executable("tasklist.exe")?;
    let output = command(executable)
        .args(["/FI", &format!("IMAGENAME eq {name}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().any(|line| {
        line.trim_start_matches('\u{feff}')
            .trim_start_matches('"')
            .split('"')
            .next()
            .is_some_and(|image| image.eq_ignore_ascii_case(name))
    }))
}

pub(super) fn current_user_sid() -> Option<String> {
    let output = command(system_executable("whoami.exe")?)
        .args(["/user", "/fo", "csv", "/nh"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.split('"')
        .find(|field| field.starts_with("S-1-"))
        .map(str::to_string)
}

pub(crate) fn disk_free_space() -> Option<u64> {
    let output = command(system_executable("WindowsPowerShell/v1.0/powershell.exe")?)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "[Console]::Write((Get-PSDrive -Name $env:SystemDrive.TrimEnd(':')).Free)",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

pub(super) fn service_state(name: &str) -> Option<u32> {
    let output = command(system_executable("sc.exe")?)
        .args(["query", name])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_service_state(&String::from_utf8_lossy(&output.stdout))
}

pub(super) fn parse_service_state(output: &str) -> Option<u32> {
    output.lines().find_map(|line| {
        let value = line
            .split_once(':')?
            .1
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        (1..=7).contains(&value).then_some(value)
    })
}

pub(super) fn service_control(action: &str, name: &str) -> bool {
    let Some(executable) = system_executable("sc.exe") else {
        return false;
    };
    command(executable)
        .args([action, name])
        .status()
        .is_ok_and(|status| status.success())
}

pub(super) fn wait_for_service_state(name: &str, expected: u32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if service_state(name) == Some(expected) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

pub(super) fn clean_delivery_optimization(result: &mut CleanupResult) {
    let Some(executable) = system_executable("WindowsPowerShell/v1.0/powershell.exe") else {
        result
            .errors
            .push("Windows PowerShell is unavailable".to_string());
        return;
    };
    let status = command(executable)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Delete-DeliveryOptimizationCache -Force",
        ])
        .status();
    if !status.is_ok_and(|status| status.success()) {
        result
            .errors
            .push("Delivery Optimization cleanup failed".to_string());
    }
}

pub(super) fn scan_dism(cancelled: &AtomicBool, result: &mut CleanupResult) {
    result.available = true;
    result.unknown_size = true;
    let Some(executable) = system_executable("dism.exe") else {
        result.errors.push("DISM is unavailable".to_string());
        return;
    };
    let mut command = command(executable);
    command.args(["/Online", "/Cleanup-Image", "/AnalyzeComponentStore"]);
    match cancellable_output(&mut command, cancelled) {
        Ok(Some(output)) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            result.files = parse_reclaimable_packages(&text).unwrap_or_default();
        }
        Ok(Some(output)) => result
            .errors
            .push(format!("DISM exited with {}", output.status)),
        Ok(None) => {}
        Err(error) => result.errors.push(format!("Could not run DISM: {error}")),
    }
}

pub(super) fn run_dism_cleanup(result: &mut CleanupResult) {
    result.unknown_size = true;
    let Some(executable) = system_executable("dism.exe") else {
        result.errors.push("DISM is unavailable".to_string());
        return;
    };
    match command(executable)
        .args(["/Online", "/Cleanup-Image", "/StartComponentCleanup"])
        .status()
    {
        Ok(status) if status.success() => {}
        Ok(status) => result.errors.push(format!("DISM exited with {status}")),
        Err(error) => result.errors.push(format!("Could not run DISM: {error}")),
    }
}

pub(super) fn parse_reclaimable_packages(output: &str) -> Option<u64> {
    output.lines().find_map(|line| {
        line.strip_prefix("Number of Reclaimable Packages :")
            .and_then(|value| value.trim().parse().ok())
    })
}
