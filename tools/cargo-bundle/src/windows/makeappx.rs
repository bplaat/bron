/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use crate::windows::ARCH;

pub(crate) fn pack(layout: &str, msix: &str) {
    let output = Command::new(find_makeappx())
        .args(["pack", "/o", "/d", layout, "/p", msix])
        .output()
        .expect("Failed to run MakeAppx");
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("MakeAppx failed:\n{stdout}{stderr}");
        exit(1);
    }
}

fn find_makeappx() -> PathBuf {
    if Command::new("makeappx.exe").arg("/?").output().is_ok() {
        return PathBuf::from("makeappx.exe");
    }

    let program_files =
        std::env::var_os("ProgramFiles(x86)").unwrap_or_else(|| r"C:\Program Files (x86)".into());
    let bin = Path::new(&program_files).join(r"Windows Kits\10\bin");
    let mut versions: Vec<_> = fs::read_dir(&bin)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.join(ARCH).join("makeappx.exe").exists())
        .collect();
    versions.sort_by_key(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(|name| {
                name.split('.')
                    .map(|part| part.parse::<u32>().unwrap_or(0))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    let Some(version) = versions.pop() else {
        eprintln!("MakeAppx not found, install the Windows SDK");
        exit(1);
    };
    version.join(ARCH).join("makeappx.exe")
}
