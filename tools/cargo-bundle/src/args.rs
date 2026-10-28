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
    pub android: bool,
    pub deb: bool,
    pub rpm: bool,
    pub run: bool,
    pub release: bool,
    pub device: Option<String>,
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
            android: false,
            deb: false,
            rpm: false,
            run: false,
            release: false,
            device: None,
        }
    }
}

pub(crate) fn parse_args() -> Args {
    parse(env::args().skip(1))
}

fn parse(arguments: impl IntoIterator<Item = String>) -> Args {
    let mut args = Args::default();
    let mut args_iter = arguments.into_iter();
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
                args.help = false;
                args.ios = true;
            }
            "--android" => {
                args.help = false;
                args.android = true;
            }
            "--deb" => {
                args.help = false;
                args.deb = true;
            }
            "--rpm" => {
                args.help = false;
                args.rpm = true;
            }
            "--run" => {
                args.help = false;
                args.run = true;
            }
            "--release" => args.release = true,
            "--device" => {
                let value = args_iter.next().unwrap_or_else(|| {
                    eprintln!("Expected value after {arg}");
                    exit(1);
                });
                args.device = Some(value);
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
  --android                 Build an Android APK instead
  --deb                     Only build a Debian package (Linux)
  --rpm                     Only build an RPM package (Linux)
  --run                     Launch the macOS app, or install and launch on an iOS simulator or the connected Android device
  --device <name-or-UDID>    Select the simulator (with --ios --run)
  --release                 Use release mode with --ios --run (default: debug)
  -h, --help                Print this help message
  -v, --version             Print the version number
"
    );
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn simulator_run_options() {
        let args = parse(
            [
                "bundle",
                "--ios",
                "--run",
                "--device",
                "iPhone",
                "--release",
            ]
            .map(str::to_string),
        );
        assert!(!args.help);
        assert!(args.ios && args.run && args.release);
        assert_eq!(args.device.as_deref(), Some("iPhone"));
    }
}
