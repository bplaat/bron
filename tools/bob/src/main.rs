/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::Instant;
use std::{env, fs};

use crate::args::{Profile, Subcommand, parse_args, subcommand_help};
use crate::bobje::Bobje;
use crate::executor::ExecutorBuilder;
use crate::tasks::android::{detect_android, run_android_apk};
#[cfg(target_os = "macos")]
use crate::tasks::bundle::{detect_bundle, run_bundle, sign_bundle};
use crate::tasks::cx::{detect_cx, run_ld, run_ld_cunit_tests};
use crate::tasks::jvm::{detect_jar, detect_java_kotlin, run_jar, run_java_class, run_junit_tests};
use crate::utils::{cache_dir, format_bytes, index_files};

mod args;
mod artifacts;
mod bobje;
mod executor;
mod log;
mod manifest;
mod tasks;
mod utils;
mod version;

// MARK: Subcommands
fn print_dir_remove_stats(path: &str) {
    let files = index_files(path);
    let total_size: u64 = files
        .iter()
        .map(|file| fs::metadata(file).expect("Can't read file metadata").len())
        .sum();
    println!(
        "Removed {} files, {} total",
        files.len(),
        format_bytes(total_size)
    );
}

fn subcommand_clean(target_dir: &str, print: bool) {
    if !Path::new(target_dir).exists() {
        if print {
            println!("Removed 0 files");
        }
        return;
    }

    if print {
        print_dir_remove_stats(target_dir);
    }
    fs::remove_dir_all(target_dir).expect("Can't remove target directory");
}

fn subcommand_clean_cache() {
    let cache_dir = cache_dir();
    if !Path::new(&cache_dir).exists() {
        println!("Removed 0 files");
        return;
    }

    print_dir_remove_stats(&cache_dir.display().to_string());
    fs::remove_dir_all(cache_dir).expect("Can't remove bob directory");
}

fn subcommand_version() {
    println!("bob v{}", env!("CARGO_PKG_VERSION"));
}

// MARK: Main
fn main() {
    #[cfg(windows)]
    {
        _ = enable_ansi_support::enable_ansi_support();
    }

    let start_time = Instant::now();

    let args = parse_args();
    if args.subcommand == Subcommand::CleanCache {
        subcommand_clean_cache();
        return;
    }
    if args.subcommand == Subcommand::Help {
        subcommand_help();
        return;
    }
    if args.subcommand == Subcommand::Version {
        subcommand_version();
        return;
    }

    // Find bob.toml and change directory to its location
    let mut bob_dir = PathBuf::from(&args.manifest_dir)
        .canonicalize()
        .unwrap_or_else(|_| {
            eprintln!(
                "Can't find or access manifest directory: {}",
                args.manifest_dir
            );
            exit(1);
        });
    while !bob_dir.join("bob.toml").exists() {
        if let Some(parent) = bob_dir.parent() {
            bob_dir = parent.to_path_buf();
        } else {
            eprintln!(
                "Can't find bob.toml in current or any parent directory (starting from {})",
                args.manifest_dir
            );
            exit(1);
        }
    }
    env::set_current_dir(bob_dir).expect("Failed to change working directory");

    // Read .env file
    _ = dotenvy::dotenv();

    // Clean build artifacts
    if args.subcommand == Subcommand::Clean {
        subcommand_clean(&args.target_dir, true);
        return;
    }

    // Clean first if requested
    if args.clean_first {
        subcommand_clean(&args.target_dir, false);
    }

    // Check target directory
    if !Path::new(&args.target_dir).exists() {
        fs::create_dir(&args.target_dir).expect("Failed to create target directory");
    }

    // Build main bobje
    let mut executor = ExecutorBuilder::new();
    let bobje = Bobje::new(&args, ".", &mut executor, true);
    let mut executor = executor.build(&format!("{}/bob.log", args.target_dir));

    executor.execute(args.verbose, args.thread_count);

    // Ad-hoc codesign the macOS bundle after it is (re)built
    #[cfg(target_os = "macos")]
    if executor.total_tasks() > 0 && detect_bundle(&bobje) {
        sign_bundle(&bobje);
    }

    // Show time taken
    if executor.total_tasks() > 0 && args.show_time {
        println!(
            "[{}/{}] Execute time: {:.2?}",
            executor.total_tasks(),
            executor.total_tasks(),
            Instant::now().duration_since(start_time)
        );
    }

    // Run build artifact
    if args.subcommand == Subcommand::Run {
        #[cfg(target_os = "macos")]
        if detect_bundle(&bobje) {
            run_bundle(&bobje);
        }
        if detect_jar(&bobje) {
            run_jar(&bobje);
        }
        if detect_android(&bobje) {
            run_android_apk(&bobje);
        }
        if detect_cx(&bobje.source_files) {
            run_ld(&bobje);
        }
        if detect_java_kotlin(&bobje.source_files) {
            run_java_class(&bobje);
        }
        eprintln!("No build artifact to run");
    }

    // Run unit tests
    if args.subcommand == Subcommand::Test {
        if detect_cx(&bobje.source_files) {
            run_ld_cunit_tests(&bobje);
        }
        if detect_java_kotlin(&bobje.source_files) {
            run_junit_tests(&bobje);
        }
        eprintln!("No test artifact to run");
    }
}
