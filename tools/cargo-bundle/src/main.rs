/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::fs;
use std::process::{ExitCode, exit};

use crate::manifest::Manifest;

mod android;
#[cfg(target_os = "macos")]
mod apple;
mod args;
mod assets;
mod manifest;
mod support;
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod windows;

fn read_manifest(path: &str) -> Manifest {
    let manifest_path = format!("{path}/Cargo.toml");
    basic_toml::from_str(&fs::read_to_string(&manifest_path).unwrap_or_else(|error| {
        eprintln!("Can't read {manifest_path} file: {error}");
        exit(1);
    }))
    .unwrap_or_else(|error| {
        eprintln!("Can't parse {manifest_path} file: {error}");
        exit(1);
    })
}

fn main() -> ExitCode {
    let args = args::parse_args();
    if args.help {
        args::help();
        return ExitCode::SUCCESS;
    }
    if args.version {
        println!("cargo-bundle {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }

    if args.ios && args.android {
        eprintln!("--ios and --android can't be combined");
        return ExitCode::FAILURE;
    }
    if args.run && !args.ios && !args.android && !cfg!(target_os = "macos") {
        eprintln!("--run requires --ios or --android on this platform");
        return ExitCode::FAILURE;
    }
    if (args.device.is_some() || args.release) && !args.ios {
        eprintln!("--device and --release require --ios");
        return ExitCode::FAILURE;
    }
    if args.device.is_some() && !args.run {
        eprintln!("--device requires --run");
        return ExitCode::FAILURE;
    }
    if args.ios && !cfg!(target_os = "macos") {
        eprintln!("iOS bundles require macOS with Xcode and an iOS Simulator runtime");
        return ExitCode::FAILURE;
    }
    let manifest = read_manifest(&args.path);
    let bundle_metadata = &manifest.package.metadata.bundle;
    println!(
        "Bundling {} v{} ({})",
        bundle_metadata.name, manifest.package.version, args.path
    );

    let target_dir = format!("target/bundle/{}", manifest.package.name);
    if args.android {
        android::bundle(&args, &target_dir, &manifest);
    } else {
        #[cfg(target_os = "macos")]
        apple::bundle(&args, &target_dir, &manifest);
        #[cfg(windows)]
        windows::bundle(&args, &target_dir, &manifest);
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            eprintln!("Only --android bundles are supported on this platform");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
