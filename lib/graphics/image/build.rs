/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Generates sRGB lookup tables using host standard library math.

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

const TABLE_STEPS: usize = 4096;

fn decode(x: f64) -> f64 {
    if x <= 0.040_45 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let mut code = format!("const TABLE_STEPS: usize = {TABLE_STEPS};\n");
    for (name, length) in [
        ("SRGB_ENCODE", TABLE_STEPS + 1),
        ("SRGB_TO_LINEAR", 256),
        ("SRGB_ROUNDING", 255),
    ] {
        writeln!(code, "static {name}: [f32; {length}] = [").expect("write table header");
        for i in 0..length {
            let value = match name {
                "SRGB_ENCODE" => {
                    let x = i as f64 / (length - 1) as f64;
                    if x <= 0.003_130_8 {
                        12.92 * x
                    } else {
                        1.055 * x.powf(1.0 / 2.4) - 0.055
                    }
                }
                "SRGB_TO_LINEAR" => decode(i as f64 / 255.0),
                _ => decode((i as f64 + 0.5) / 255.0),
            } as f32;
            writeln!(code, "    {value:?},").expect("write table sample");
        }
        code.push_str("];\n");
    }
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    fs::write(out_dir.join("srgb.rs"), code).expect("write sRGB tables");
}
