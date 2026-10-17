/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The gxtexconv command line tool, converts a `.scf` script to a `.tpl` texture file.

use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};

fn main() -> ExitCode {
    let (mut script, mut output) = (None, None);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-s" => script = args.next(),
            "-o" => output = args.next(),
            _ => {
                script = None;
                break;
            }
        }
    }
    let (Some(script), Some(output)) = (script, output) else {
        eprintln!("Usage: gxtexconv -s <script>.scf -o <output>.tpl");
        return ExitCode::FAILURE;
    };
    let result = gxtexconv::convert_script(Path::new(&script))
        .map_err(|err| err.to_string())
        .and_then(|tpl| {
            fs::write(&output, tpl).map_err(|err| format!("failed to write {output}: {err}"))
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("gxtexconv: {err}");
            ExitCode::FAILURE
        }
    }
}
