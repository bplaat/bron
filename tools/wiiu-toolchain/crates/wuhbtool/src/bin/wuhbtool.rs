/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../../README.md")]

use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, fs};

use wuhbtool::Options;

const USAGE: &str = "Usage: wuhbtool <rpx-file> <output> [options]

Options:
  --name=<name>          Long name of the application
  --short-name=<name>    Short name of the application
  --author=<author>      Author of the application
  --icon=<image>         Application icon (128x128)
  --tv-image=<image>     Splash Screen image shown on the TV (1280x720)
  --drc-image=<image>    Splash Screen image shown on the DRC (854x480)
  --content=<directory>  Path to the /content directory
  -H, --help             Show help.";

/// Parses the arguments like wuhbtool, options as `--name=value` or `--name value`.
fn parse(mut args: impl Iterator<Item = String>) -> Result<Option<(Options, PathBuf)>, String> {
    let mut options = Options::default();
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        let Some(option) = arg.strip_prefix("--") else {
            match arg.as_str() {
                "-H" | "-h" => return Ok(None),
                _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
                _ => positional.push(arg),
            }
            continue;
        };
        let (key, value) = match option.split_once('=') {
            Some((key, value)) => (key, value.to_owned()),
            None if option == "help" => return Ok(None),
            None => (
                option,
                args.next()
                    .ok_or_else(|| format!("missing value for --{option}"))?,
            ),
        };
        match key {
            "name" => options.name = Some(value),
            "short-name" => options.short_name = Some(value),
            "author" => options.author = Some(value),
            "icon" => options.icon = Some(value.into()),
            "tv-image" => options.tv_image = Some(value.into()),
            "drc-image" => options.drc_image = Some(value.into()),
            "content" => options.content = Some(value.into()),
            _ => return Err(format!("unknown option: --{key}")),
        }
    }
    match <[String; 2]>::try_from(positional) {
        Ok([rpx, output]) => {
            options.rpx = rpx.into();
            Ok(Some((options, output.into())))
        }
        Err(positional) if positional.is_empty() => Ok(None),
        Err(_) => Err("expected <rpx-file> and <output>".to_owned()),
    }
}

fn main() -> ExitCode {
    let (options, output) = match parse(env::args().skip(1)) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("wuhbtool: {err}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    let result = wuhbtool::build(&options)
        .map_err(|err| err.to_string())
        .and_then(|bundle| {
            for warning in &bundle.warnings {
                eprintln!("Warning: {warning}");
            }
            fs::write(&output, bundle.data)
                .map_err(|err| format!("failed to write {}: {err}", output.display()))
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("wuhbtool: {err}");
            ExitCode::FAILURE
        }
    }
}
