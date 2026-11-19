/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../../README.md")]

use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

fn main() -> ExitCode {
    let (mut config, mut output) = (None, None);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" => config = args.next(),
            "-o" => output = args.next(),
            _ => {
                config = None;
                break;
            }
        }
    }
    let (Some(config), Some(output)) = (config, output) else {
        eprintln!("Usage: gtxconv -c <config>.toml -o <output>.gtx");
        return ExitCode::FAILURE;
    };
    let result = gtxconv::convert_config(Path::new(&config))
        .map_err(|err| err.to_string())
        .and_then(|gtx| {
            fs::write(&output, gtx).map_err(|err| format!("failed to write {output}: {err}"))
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("gtxconv: {err}");
            ExitCode::FAILURE
        }
    }
}
