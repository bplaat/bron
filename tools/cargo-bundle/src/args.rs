/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::env;
use std::process::exit;

pub(crate) struct Args {
    pub help: bool,
    pub version: bool,
    pub path: String,
    pub zip: bool,
    pub dmg: bool,
    pub ios: bool,
}

impl Default for Args {
    fn default() -> Self {
        Args {
            help: true,
            version: false,
            path: ".".to_string(),
            zip: false,
            dmg: false,
            ios: false,
        }
    }
}

pub(crate) fn parse_args() -> Args {
    let mut args = Args::default();
    let mut args_iter = env::args().skip(1);
    while let Some(arg) = args_iter.next() {
        match arg.as_str() {
            "bundle" => continue,
            "-h" | "--help" => {
                args.help = true;
            }
            "-v" | "--version" => {
                args.help = false;
                args.version = true;
            }
            "-p" | "--path" => {
                args.help = false;
                if let Some(dir) = args_iter.next() {
                    args.path = dir;
                } else {
                    eprintln!("Expected directory after {arg}");
                    exit(1);
                }
            }
            "--zip" => {
                args.zip = true;
            }
            "--dmg" => {
                args.dmg = true;
            }
            "--ios" => {
                args.ios = true;
            }
            _ => {
                eprintln!("Unknown argument: {arg}");
                exit(1);
            }
        }
    }
    args
}

pub(crate) fn help() {
    println!(
        r"Usage: cargo bundle [OPTIONS]

Options:
  -p <dir>, --path <dir>    Build crate in <dir>
  --zip                     Also create a zip archive (macOS)
  --dmg                     Also create a DMG installer (macOS)
  --ios                     Build an iOS app instead (macOS)
  -h, --help                Print this help message
  -v, --version             Print the version number
"
    );
}
