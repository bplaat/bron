/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::process::{ExitCode, exit};

use crate::manifest::Manifest;

mod android;
#[cfg(target_os = "macos")]
mod apple;
mod args;
mod assets;
#[cfg(any(target_os = "linux", test))]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod linux;
mod manifest;
mod support;
#[cfg(any(windows, test))]
#[cfg_attr(not(windows), allow(dead_code))]
mod windows;

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
    if (args.deb || args.rpm) && (args.ios || args.android || !cfg!(target_os = "linux")) {
        eprintln!("--deb and --rpm require Linux and can't be combined with --ios or --android");
        return ExitCode::FAILURE;
    }
    let manifest = Manifest::read(&args.path).unwrap_or_else(|error| {
        eprintln!("{error}");
        exit(1);
    });
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
        #[cfg(target_os = "linux")]
        linux::bundle(&args, &target_dir, &manifest);
        #[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
        {
            eprintln!("Only --android bundles are supported on this platform");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
