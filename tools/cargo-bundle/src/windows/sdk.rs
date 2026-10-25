/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

use crate::windows::HOST_ARCH;

pub(crate) fn pack(layout: &str, msix: &str) {
    run("makeappx.exe", &["pack", "/o", "/d", layout, "/p", msix]);
}

pub(crate) fn pack_assets(manifest: &Path, mapping: &Path, msix: &str) {
    run(
        "makeappx.exe",
        &[
            "pack",
            "/o",
            "/r",
            "/m",
            manifest.to_str().expect("UTF-8 manifest path"),
            "/f",
            mapping.to_str().expect("UTF-8 mapping path"),
            "/p",
            msix,
        ],
    );
}

pub(crate) fn bundle(packages: &str, msix_bundle: &str, version: &str) {
    run(
        "makeappx.exe",
        &[
            "bundle",
            "/o",
            "/bv",
            version,
            "/d",
            packages,
            "/p",
            msix_bundle,
        ],
    );
}

// Indexes the qualified assets, like the unplated logo, so Windows can pick between them
pub(crate) fn make_pri(config: &str, layout: &str) {
    if !Path::new(config).exists() {
        run(
            "makepri.exe",
            &[
                "createconfig",
                "/o",
                "/cf",
                config,
                "/dq",
                "en-US",
                "/pv",
                "10.0.0",
            ],
        );
    }
    run(
        "makepri.exe",
        &[
            "new",
            "/o",
            "/pr",
            layout,
            "/cf",
            config,
            "/mn",
            &format!("{layout}/AppxManifest.xml"),
            "/of",
            &format!("{layout}/resources.pri"),
        ],
    );
}

fn run(tool: &str, args: &[&str]) {
    let output = Command::new(find_tool(tool))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("Failed to run {tool}: {error}"));
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{tool} failed:\n{stdout}{stderr}");
        exit(1);
    }
}

fn find_tool(tool: &str) -> PathBuf {
    if Command::new(tool).arg("/?").output().is_ok() {
        return PathBuf::from(tool);
    }

    let program_files =
        std::env::var_os("ProgramFiles(x86)").unwrap_or_else(|| r"C:\Program Files (x86)".into());
    let bin = Path::new(&program_files).join(r"Windows Kits\10\bin");
    let mut versions: Vec<_> = fs::read_dir(&bin)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.join(HOST_ARCH).join(tool).exists())
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
        eprintln!("{tool} not found, install the Windows SDK");
        exit(1);
    };
    version.join(HOST_ARCH).join(tool)
}
