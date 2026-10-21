/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use crate::compile::compile_shader;

mod compile;
mod hlsl;
mod metal;
mod output;

/// Result type returned by shader build operations.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Compiles named WGSL files using Cargo's target and output directory.
/// Source paths are relative to CARGO_MANIFEST_DIR; names match include_wgsl!.
#[derive(Default)]
pub struct Builder {
    shaders: Vec<(String, PathBuf)>,
}

impl Builder {
    /// Creates an empty shader builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a source path under a unique, relative output name.
    /// Names may contain forward-slash directories, but not `.` or `..` components.
    pub fn shader(mut self, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.shaders.push((name.into(), path.into()));
        self
    }

    /// Translate every registered shader and compile it to the target's native format.
    /// Returns source-aware diagnostics instead of panicking on invalid shaders or I/O errors.
    pub fn compile(self) -> Result<()> {
        let manifest = env::var_os("CARGO_MANIFEST_DIR")
            .ok_or("Missing CARGO_MANIFEST_DIR; run from a Cargo build script")?;
        let out = env::var_os("OUT_DIR").ok_or("Missing OUT_DIR; run from a Cargo build script")?;
        let target = env::var("CARGO_CFG_TARGET_OS")?;
        println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
        println!("cargo:rerun-if-env-changed=IPHONEOS_DEPLOYMENT_TARGET");
        self.compile_to(Path::new(&manifest), Path::new(&out), &target)
    }

    fn compile_to(self, manifest: &Path, out: &Path, target: &str) -> Result<()> {
        if !matches!(target, "macos" | "ios" | "linux" | "windows" | "android") {
            return Err(format!("Unsupported graphics target: {target}").into());
        }
        if self.shaders.is_empty() {
            return Err("No shaders registered".into());
        }
        let mut names = HashSet::new();
        // Validate all output names before writing anything under OUT_DIR.
        for (name, _) in &self.shaders {
            if name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
                || name
                    .chars()
                    .any(|c| c == '\\' || c == ':' || c.is_control())
            {
                return Err(format!("Invalid shader output name: {name:?}").into());
            }
            if !names.insert(name.to_lowercase()) {
                return Err(format!("Duplicate shader output name: {name}").into());
            }
        }
        for (name, source) in self.shaders {
            let path = manifest.join(source);
            // Cargo directives are line-based, so reject paths that cannot be represented.
            let display = path.to_str().ok_or("Shader source path must be UTF-8")?;
            if display.chars().any(char::is_control) {
                return Err("Shader source path contains control characters".into());
            }
            println!("cargo:rerun-if-changed={display}");
            compile_shader(&path, out, &name, target)?;
        }
        Ok(())
    }
}
