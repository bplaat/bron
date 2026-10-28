/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use regex::regex;
use serde_json::Value;

use crate::metadata::{
    add_excludes, features_without_swap, has_target_kind, packages, platform_excludes,
};
use crate::process::{capture, npm_program, npx_program, run, run_named};
use crate::utils::{collect_files, collect_source_files, relative_slash};
use crate::{Os, Xtask};

pub(crate) const BACKEND_SWAP_PAIRS: [(&str, &str); 2] =
    [("native-tls", "vendored"), ("bsql", "sqlite-bundled")];

impl Xtask {
    pub(crate) fn check(&self) -> Result<()> {
        self.check_copyright()?;
        self.check_formatting()?;
        self.check_docker()?;
        self.check_rust()?;
        self.check_rust_deps()?;
        self.check_e2e()?;
        if self.os == Os::Macos {
            self.check_macos_examples()?;
        }
        self.check_others()
    }

    pub(crate) fn check_shared(&self) -> Result<()> {
        self.check_copyright()?;
        self.check_formatting()?;
        self.check_docker()?;
        self.check_rust_deps()
    }

    pub(crate) fn check_editor(&self) -> Result<()> {
        let metadata = self.cargo_metadata()?;
        let excludes = platform_excludes(&metadata, self.os);
        let mut command = editor_check_command(&excludes);
        let status = command.status().context("failed to start cargo check")?;
        if !status.success() {
            bail!("cargo check failed with {status}");
        }
        Ok(())
    }

    pub(crate) fn check_copyright(&self) -> Result<()> {
        println!("Checking copyright headers...");
        let extensions = [
            "rs", "wgsl", "html", "css", "js", "jsx", "ts", "tsx", "cc", "hh",
        ];
        let mut bad = Vec::new();
        for file in self.source_files(&extensions)? {
            let relative = relative_slash(&self.root, &file);
            if relative.starts_with("tools/bob/examples/")
                || relative.ends_with(".min.js")
                || (relative.starts_with("tools/ccontinue/tests/")
                    && file.extension().is_some_and(|ext| ext == "cc"))
            {
                continue;
            }
            let contents = fs::read_to_string(&file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            if !regex!(r"Copyright \(c\) 20[0-9]{2}(-20[0-9]{2})? \w+").is_match(&contents) {
                bad.push(relative);
            }
        }
        if !bad.is_empty() {
            for file in bad {
                eprintln!("Bad copyright header in: {file}");
            }
            bail!("copyright header check failed");
        }
        Ok(())
    }

    pub(crate) fn check_formatting(&self) -> Result<()> {
        self.ensure_npm_deps()?;
        println!("Checking Prettier formatting...");
        let extensions = [
            "md", "json", "yml", "yaml", "html", "css", "js", "jsx", "ts", "tsx", "xml", "svg",
        ];
        let files = self
            .source_files(&extensions)?
            .into_iter()
            .filter(|file| {
                let relative = relative_slash(&self.root, file);
                !relative.starts_with(".vscode/")
                    && !relative.contains("/playwright/")
                    && !relative.contains("/test-results/")
                    && !relative.ends_with(".min.js")
            })
            .collect::<Vec<_>>();
        let mut command = Command::new(npx_program(self.os));
        command.args(["--no-install", "prettier", "--check"]);
        for files in command_batches(&files, command_argument_budget(&command, self.os)?, self.os)?
        {
            let mut command = Command::new(npx_program(self.os));
            command.args(["--no-install", "prettier", "--check"]);
            command.args(files);
            run_named(
                &mut command,
                &format!(
                    "{} prettier --check ({} files)",
                    npx_program(self.os),
                    files.len()
                ),
            )?;
        }

        println!("Checking clang-format formatting...");
        let mut files = Vec::new();
        for directory in [
            "tools/bob/examples",
            "examples/macos",
            "examples/wii",
            "games/empires",
            "lib/ui/bwebview/src/platforms/android",
            "tools/nds-toolchain",
            "tools/wii-toolchain",
        ] {
            collect_source_files(&self.root.join(directory), &mut files)?;
        }
        files.retain(|file| {
            file.file_name().is_none_or(|name| name != "stb_image.h")
                && file
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        matches!(ext, "c" | "h" | "cpp" | "hpp" | "m" | "mm" | "java")
                    })
        });
        files.sort();
        let mut command = Command::new("clang-format");
        command.args(["--dry-run", "--Werror"]);
        for files in command_batches(&files, command_argument_budget(&command, self.os)?, self.os)?
        {
            let mut command = Command::new("clang-format");
            command.args(["--dry-run", "--Werror"]);
            command.args(files);
            run_named(
                &mut command,
                &format!("clang-format --dry-run --Werror ({} files)", files.len()),
            )?;
        }

        println!("Checking Swift formatting...");
        let files = self.source_files(&["swift"])?;
        let mut command = Command::new("swift");
        command.args(["format", "lint", "--strict"]);
        for files in command_batches(&files, command_argument_budget(&command, self.os)?, self.os)?
        {
            let mut command = Command::new("swift");
            command.args(["format", "lint", "--strict"]);
            command.args(files);
            run_named(
                &mut command,
                &format!("swift format lint --strict ({} files)", files.len()),
            )?;
        }

        println!("Checking TOML formatting...");
        run(Command::new("taplo").args(["fmt", "--check"]))
    }

    pub(crate) fn check_docker(&self) -> Result<()> {
        println!("Checking Dockerfiles...");
        let mut files = self.source_files(&[])?;
        files.retain(|file| {
            let name = file
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            name == "Dockerfile" || name.ends_with(".Dockerfile")
        });
        for file in files {
            run(Command::new("hadolint").arg(file))?;
        }
        Ok(())
    }

    pub(crate) fn check_rust(&self) -> Result<()> {
        let metadata = self.cargo_metadata()?;
        let excludes = platform_excludes(&metadata, self.os);

        println!("Checking Rust formatting...");
        run(Command::new("cargo").args(["+nightly", "fmt", "--", "--check"]))?;

        println!("Linting Rust code...");
        let mut command = Command::new("cargo");
        command.args(["clippy", "--workspace"]);
        add_excludes(&mut command, &excludes);
        command.args([
            "--locked",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
            "-W",
            "clippy::uninlined_format_args",
        ]);
        run(&mut command)?;

        println!("Running Rust tests...");
        let mut command = Command::new("cargo");
        command.args(["test", "--doc", "--all-features", "--locked", "--workspace"]);
        add_excludes(&mut command, &excludes);
        run(&mut command)?;

        let mut command = Command::new("cargo");
        command.args([
            "nextest",
            "run",
            "--all-features",
            "--locked",
            "--config-file",
            "nextest.toml",
        ]);
        if env::var("CI").is_ok_and(|value| !value.is_empty()) {
            command.args(["--profile", "ci"]);
        }
        command.arg("--workspace");
        add_excludes(&mut command, &excludes);
        run(&mut command)?;

        for (package, feature) in BACKEND_SWAP_PAIRS {
            if excludes.contains(package) {
                continue;
            }
            println!("Running Rust tests for {package} without {feature} feature...");
            let feature_args = features_without_swap(&metadata, package, feature)?;
            let mut command = Command::new("cargo");
            command.args(["test", "--doc", "--locked", "-p", package]);
            command.args(&feature_args);
            run(&mut command)?;

            let mut command = Command::new("cargo");
            command.args([
                "nextest",
                "run",
                "--locked",
                "--config-file",
                "nextest.toml",
            ]);
            if env::var("CI").is_ok_and(|value| !value.is_empty()) {
                command.args(["--profile", "ci"]);
            }
            command.args(["-p", package]);
            command.args(&feature_args);
            run(&mut command)?;
        }

        for feature in ["sqlite", "mysql"] {
            println!("Checking bsql with only the {feature} backend...");
            run(Command::new("cargo").args([
                "clippy",
                "--locked",
                "-p",
                "bsql",
                "--all-targets",
                "--no-default-features",
                "--features",
                feature,
                "--",
                "-D",
                "warnings",
                "-W",
                "clippy::uninlined_format_args",
            ]))?;
            run(Command::new("cargo").args([
                "test",
                "--locked",
                "-p",
                "bsql",
                "--no-default-features",
                "--features",
                feature,
            ]))?;
        }

        if self.os != Os::Windows {
            self.check_address_sanitizer(&metadata)?;
        }
        Ok(())
    }

    fn check_address_sanitizer(&self, metadata: &Value) -> Result<()> {
        println!("Running Rust tests with address sanitizer on unsafe libs...");
        let excludes = platform_excludes(metadata, self.os);
        let output = capture(Command::new("rustc").args(["+nightly", "-vV"]))?;
        let rustc_info = String::from_utf8(output.stdout)?;
        let target = rustc_info
            .lines()
            .find_map(|line| line.strip_prefix("host: "))
            .context("rustc did not report its host target")?;

        for package in packages(metadata)? {
            let name = package
                .get("name")
                .and_then(Value::as_str)
                .context("Cargo package has no name")?;
            if !has_target_kind(package, &["lib", "proc-macro"]) || name == "bwebview" {
                continue;
            }
            let crate_dir = package
                .get("manifest_path")
                .and_then(Value::as_str)
                .and_then(|manifest| Path::new(manifest).parent())
                .context("Cargo package has no manifest directory")?;
            let mut rust_files = Vec::new();
            collect_files(crate_dir, &mut rust_files)?;
            rust_files.retain(|file| file.extension().is_some_and(|ext| ext == "rs"));
            let has_unsafe =
                rust_files
                    .into_iter()
                    .try_fold(false, |found, file| -> Result<bool> {
                        Ok(found || contains_unsafe_block(&fs::read_to_string(file)?))
                    })?;
            if !has_unsafe {
                continue;
            }
            let package = name;
            if excludes.contains(package) {
                continue;
            }
            println!("Testing {package} with address sanitizer...");
            let swap = BACKEND_SWAP_PAIRS
                .iter()
                .find_map(|(candidate, feature)| (*candidate == package).then_some(*feature));
            let feature_args = match swap {
                Some(feature) => features_without_swap(metadata, package, feature)?,
                None => vec!["--all-features".to_owned()],
            };
            let mut command = Command::new("cargo");
            let rustflags = env::var("RUSTFLAGS").unwrap_or_default();
            command.env(
                "RUSTFLAGS",
                format!(
                    "{rustflags}{}-Zsanitizer=address",
                    if rustflags.is_empty() { "" } else { " " }
                ),
            );
            if self.os == Os::Linux && package == "bcanvas" {
                let options = env::var("LSAN_OPTIONS").unwrap_or_default();
                let suppressions = self.root.join("tools/xtask/lsan.supp");
                command.env(
                    "LSAN_OPTIONS",
                    format!("{options}:suppressions={}", suppressions.display()),
                );
            }
            command.args([
                "+nightly", "test", "-p", package, "--lib", "--tests", "--locked",
            ]);
            command.args(feature_args);
            command.args(["--target", target, "-Zbuild-std"]);
            if target == "aarch64-unknown-linux-gnu" {
                // Unoptimized outlined atomics call ASan-instrumented core code
                // before ASan initializes. Inline those calls in compiler_builtins.
                command.args([
                    "--config",
                    "profile.dev.package.compiler_builtins.opt-level=3",
                ]);
            }
            run(&mut command)?;
        }
        Ok(())
    }

    pub(crate) fn check_rust_deps(&self) -> Result<()> {
        println!("Checking Rust dependencies...");
        run(Command::new("cargo").args(["deny", "check", "--hide-inclusion-graph"]))
    }

    pub(crate) fn check_e2e(&self) -> Result<()> {
        println!("Running end-to-end tests...");
        self.ensure_npm_deps()?;
        run(Command::new(npx_program(self.os)).args([
            "--no-install",
            "--workspace",
            "plaatui-showcase",
            "playwright",
            "install",
            "--with-deps",
        ]))?;

        run(Command::new(npm_program(self.os)).args(["test", "--workspace", "plaatui-showcase"]))?;

        run(Command::new("cargo").args(["build", "-p", "plaatnotes", "--locked"]))?;
        run(Command::new(npm_program(self.os)).args(["test", "--workspace", "plaatnotes"]))
    }

    pub(crate) fn check_macos_examples(&self) -> Result<()> {
        println!("Building macOS application examples...");
        run(Command::new("sh").args(["examples/macos/build.sh", "native"]))
    }

    pub(crate) fn check_others(&self) -> Result<()> {
        println!("Building Windows examples...");
        run(Command::new("sh").arg("examples/windows/build.sh"))?;

        println!("Building macOS assembly examples...");
        run(Command::new("sh").arg("examples/macos/build.sh"))?;

        println!("Building Empires and the DRS viewer...");
        run(Command::new("make").args(["-C", "games/empires"]))?;

        println!("Running Nintendo DS toolchain tests...");
        run(Command::new("bash").args(["tools/nds-toolchain/build.sh", "test"]))?;

        println!("Running Wii toolchain tests...");
        run(Command::new("bash").args(["tools/wii-toolchain/build.sh", "test"]))?;

        // The port selects its nightly toolchain itself, so drop the one cargo xtask runs with
        println!("Building the PlaatCraft Wii port...");
        run(Command::new("cargo")
            .args(["build", "--release", "--locked"])
            .current_dir("games/plaatcraft-wii")
            .env_remove("RUSTUP_TOOLCHAIN"))
    }

    pub(crate) fn coverage(&self) -> Result<()> {
        let metadata = self.cargo_metadata()?;
        let excludes = platform_excludes(&metadata, self.os);
        let mut command = Command::new("cargo");
        command.args([
            "llvm-cov",
            "nextest",
            "--all-features",
            "--locked",
            "--config-file",
            "nextest.toml",
            "--no-fail-fast",
            "--workspace",
        ]);
        add_excludes(&mut command, &excludes);
        run(&mut command)
    }
}

fn command_argument_budget(command: &Command, os: Os) -> Result<usize> {
    // Batch scripts run through cmd.exe; native programs use CreateProcessW.
    #[cfg(windows)]
    let limit = if Path::new(command.get_program())
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        }) {
        8191usize
    } else {
        32_767usize
    };
    #[cfg(unix)]
    let limit = unix_arg_max()?;
    let environment = if os == Os::Windows {
        0
    } else {
        env::vars_os()
            .map(|(key, value)| key.to_string_lossy().len() + value.to_string_lossy().len() + 18)
            .sum()
    };
    let fixed_args = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|arg| command_argument_size(Path::new(arg), os))
        .sum::<usize>();
    // Leave room for the command interpreter on Windows and argv overhead on Unix.
    let reserve = if os == Os::Windows { 1024 } else { 4096 };
    limit
        .checked_sub(environment + fixed_args + reserve)
        .context("environment leaves no room for command arguments")
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn unix_arg_max() -> Result<usize> {
    // SAFETY: sysconf only reads the requested process configuration value.
    let limit = unsafe { crate::headers::sysconf(crate::headers::SC_ARG_MAX) };
    usize::try_from(limit)
        .ok()
        .filter(|limit| *limit > 0)
        .context("sysconf failed to read ARG_MAX")
}

fn command_batches(files: &[PathBuf], budget: usize, os: Os) -> Result<Vec<&[PathBuf]>> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (index, file) in files.iter().enumerate() {
        let size = command_argument_size(file, os);
        if size > budget {
            bail!("path is too long for a command: {}", file.display());
        }
        if used + size > budget {
            batches.push(&files[start..index]);
            start = index;
            used = 0;
        }
        used += size;
    }
    if start < files.len() {
        batches.push(&files[start..]);
    }
    Ok(batches)
}

fn command_argument_size(path: &Path, os: Os) -> usize {
    if os == Os::Windows {
        // Quoting can double backslashes; count UTF-16 code units for cmd.exe.
        path.to_string_lossy().encode_utf16().count() * 2 + 4
    } else {
        // Include the terminating NUL and argv pointer, with some margin.
        path.to_string_lossy().len() + 16
    }
}

fn editor_check_command(excludes: &std::collections::BTreeSet<String>) -> Command {
    let mut command = Command::new("cargo");
    command.args([
        "check",
        "--workspace",
        "--all-targets",
        "--all-features",
        "--locked",
        "--message-format=json",
    ]);
    add_excludes(&mut command, excludes);
    command
}

fn contains_unsafe_block(contents: &str) -> bool {
    regex!(r"unsafe\s*\{").is_match(contents)
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn batches_fit_the_command_budget() -> Result<()> {
        let files = [
            PathBuf::from("short.json"),
            PathBuf::from("a/deeply/nested/file.json"),
            PathBuf::from("another/deeply/nested/file.json"),
        ];
        for os in [Os::Linux, Os::Macos, Os::Windows] {
            let budget =
                command_argument_size(&files[0], os) + command_argument_size(&files[1], os);
            let batches = command_batches(&files, budget, os)?;
            assert_eq!(batches, [&files[..2], &files[2..]]);
            assert!(batches.iter().all(|batch| {
                batch
                    .iter()
                    .map(|file| command_argument_size(file, os))
                    .sum::<usize>()
                    <= budget
            }));
            assert!(command_batches(&files, 1, os).is_err());
            assert!(command_batches(&[], budget, os)?.is_empty());
        }
        Ok(())
    }

    #[test]
    fn editor_check_emits_cargo_json_and_excludes_unsupported_packages() {
        let excludes = ["unsupported".to_owned()]
            .into_iter()
            .collect::<BTreeSet<_>>();
        let command = editor_check_command(&excludes);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "check",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--locked",
                "--message-format=json",
                "--exclude",
                "unsupported",
            ]
        );
    }
}
