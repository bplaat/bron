/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../../README.md")]

use std::collections::HashMap;
use std::process::Command;

use ccontinue::Transpiler;

use crate::args::parse_args;
use crate::temp::TempFileManager;

mod args {
    use std::env;
    use std::process::exit;

    pub(super) struct Args {
        pub(super) files: Vec<String>,
        pub(super) output: Option<String>,
        pub(super) include_paths: Vec<String>,
        pub(super) flag_source: bool,
        pub(super) flag_compile: bool,
        pub(super) flag_run: bool,
    }

    enum Action {
        Run(Args),
        Help,
        Version,
    }

    fn parse_args_from(raw: impl IntoIterator<Item = String>) -> Result<Action, String> {
        let mut files = Vec::new();
        let mut output = None;
        let mut include_paths = Vec::new();
        let mut flag_source = false;
        let mut flag_compile = false;
        let mut flag_run = false;
        let mut help = false;
        let mut version = false;
        let mut positional_only = false;

        let mut raw = raw.into_iter();
        while let Some(arg) = raw.next() {
            if positional_only {
                files.push(arg);
                continue;
            }
            match arg.as_str() {
                "--" => positional_only = true,
                "-h" | "--help" => help = true,
                "-v" | "--version" => version = true,
                "-o" | "--output" => {
                    output = Some(
                        raw.next()
                            .ok_or_else(|| format!("expected a file after '{arg}'"))?,
                    );
                }
                "-I" | "--include" => {
                    include_paths.push(
                        raw.next()
                            .ok_or_else(|| format!("expected a path after '{arg}'"))?,
                    );
                }
                arg if arg.starts_with("-I") && arg.len() > 2 => {
                    include_paths.push(arg[2..].to_owned());
                }
                "-S" | "--source" => flag_source = true,
                "-c" | "--compile" => flag_compile = true,
                "-r" | "--run" => flag_run = true,
                arg if !arg.starts_with('-') => files.push(arg.to_owned()),
                _ => return Err(format!("unknown option '{arg}'")),
            }
        }

        if help {
            return Ok(Action::Help);
        }
        if version {
            return Ok(Action::Version);
        }
        if files.is_empty() {
            return Err("no input files provided".to_owned());
        }
        let mode_count =
            usize::from(flag_source) + usize::from(flag_compile) + usize::from(flag_run);
        if mode_count > 1 {
            return Err(
                "options '--source', '--compile', and '--run' cannot be combined".to_owned(),
            );
        }
        if files.len() > 1 && output.is_some() && (flag_source || flag_compile) {
            return Err(
                "option '--output' cannot be used with multiple inputs in this mode".to_owned(),
            );
        }

        Ok(Action::Run(Args {
            files,
            output,
            include_paths,
            flag_source,
            flag_compile,
            flag_run,
        }))
    }

    fn print_help() {
        println!(
            r"Usage: ccc [OPTIONS] <file>...

Transpile and compile cContinue source files.

Options:
  -o <file>, --output <file>  Write output to <file>
  -I <path>, --include <path> Add an include search path
  -S, --source                Transpile only and emit C source
  -c, --compile               Transpile and compile without linking
  -r, --run                   Run the executable after linking
  -h, --help                  Print this help message
  -v, --version               Print the version number

Environment:
  CC                          Override the platform C compiler
  LD                          Linker used by GNU-like compilers
  CFLAGS                      Additional C compiler flags
  LDFLAGS                     Additional linker flags"
        );
    }

    pub(super) fn parse_args() -> Args {
        match parse_args_from(env::args().skip(1)) {
            Ok(Action::Run(args)) => args,
            Ok(Action::Help) => {
                print_help();
                exit(0);
            }
            Ok(Action::Version) => {
                println!("ccc v{}", env!("CARGO_PKG_VERSION"));
                exit(0);
            }
            Err(error) => {
                eprintln!("error: {error}\n\nFor more information, try 'ccc --help'.");
                exit(1);
            }
        }
    }
}

mod temp {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Manages temporary file creation with a consistent naming scheme.
    /// Each compiler process gets its own temp_dir/ccontinue/ccc_<pid> workspace.
    pub(super) struct TempFileManager {
        base_dir: PathBuf,
    }

    impl TempFileManager {
        /// Creates a manager with a per-process workspace.
        pub(super) fn new() -> Self {
            let mut base = std::env::temp_dir();
            base.push("ccontinue");
            base.push(format!("ccc_{}", std::process::id()));
            std::fs::create_dir_all(&base).unwrap_or_else(|error| {
                eprintln!("[ERROR] Can't create temp dir: {error}");
                std::process::exit(1);
            });
            Self { base_dir: base }
        }

        /// Returns the per-process temporary directory.
        pub(super) const fn base_dir(&self) -> &PathBuf {
            &self.base_dir
        }

        /// Creates a unique temporary file path with the provided extension.
        pub(super) fn temp_file(&self, extension: &str) -> String {
            let number = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("ccc_{}_{number}{extension}", std::process::id());
            let path = self.base_dir.join(name);
            path.to_str()
                .expect("temp file path is valid UTF-8")
                .to_owned()
        }
    }

    impl Drop for TempFileManager {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base_dir);
        }
    }
}

include!(concat!(env!("OUT_DIR"), "/embedded_std.rs"));

enum SourceInput {
    Transpiled { path: String, source: String },
    Native(String),
}

fn create_transpiler(include_paths: &[String]) -> Transpiler {
    let mut embedded_includes = HashMap::new();
    for &(filename, content) in STD_INCLUDES {
        embedded_includes.insert(filename.to_owned(), content.to_owned());
    }
    let mut transpiler = Transpiler::new(include_paths.to_vec());
    transpiler.set_embedded_includes(embedded_includes);
    transpiler
}

fn transpile_sources(
    transpiler: &mut Transpiler,
    source_paths: &[String],
) -> Result<Vec<SourceInput>, String> {
    source_paths
        .iter()
        .map(|path| {
            if !path.ends_with(".hh") && !path.ends_with(".cc") {
                return Ok(SourceInput::Native(path.clone()));
            }
            transpiler.reset();
            let text = std::fs::read_to_string(path)
                .map_err(|error| format!("can't read {path}: {error}"))?;
            Ok(SourceInput::Transpiled {
                path: path.clone(),
                source: transpiler.transpile(path, path.ends_with(".hh"), &text),
            })
        })
        .collect()
}

fn write_transpiled_sources(
    sources: &[SourceInput],
    output: &Option<String>,
) -> Result<(), String> {
    for source in sources {
        let SourceInput::Transpiled { path, source } = source else {
            continue;
        };
        let output_path = output
            .clone()
            .unwrap_or_else(|| path.replace(".cc", ".c").replace(".hh", ".h"));
        std::fs::write(&output_path, source)
            .map_err(|error| format!("can't write {output_path}: {error}"))?;
    }
    Ok(())
}

// Extracts the standard library C headers for compiler access.
fn setup_std_headers(temp_mgr: &TempFileManager) -> Result<(), String> {
    for &(filename, content) in STD_FILES {
        let dest = temp_mgr.base_dir().join(filename);
        std::fs::write(&dest, content)
            .map_err(|error| format!("can't write standard library file {filename}: {error}"))?;
    }
    Ok(())
}

// Extracts the prebuilt standard library archive.
fn setup_std_archive(temp_mgr: &TempFileManager, is_like_msvc: bool) -> Result<String, String> {
    let archive = if std::env::var_os("CCONTINUE_SANITIZE_STD").is_some() {
        STD_SANITIZED_ARCHIVE.unwrap_or(STD_ARCHIVE)
    } else {
        STD_ARCHIVE
    };
    let archive_path = temp_mgr.base_dir().join(if is_like_msvc {
        "ccontinue_std.lib"
    } else {
        "libccontinue_std.a"
    });
    std::fs::write(&archive_path, archive)
        .map_err(|error| format!("can't write standard library archive: {error}"))?;

    Ok(archive_path
        .to_str()
        .expect("archive path is valid UTF-8")
        .to_owned())
}

fn compiler_build(temp_mgr: &TempFileManager, include_paths: &[String]) -> cc::Build {
    let mut build = cc::Build::new();
    build
        .cargo_metadata(false)
        .target(BUILD_TARGET)
        .host(BUILD_TARGET)
        .out_dir(temp_mgr.base_dir())
        .opt_level(0)
        .debug(false)
        .static_crt(BUILD_STATIC_CRT)
        .std("c11")
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true)
        .includes(include_paths);
    build.flag_if_supported("-Wpedantic");
    build
}

fn object_path(path: &str) -> String {
    std::path::Path::new(path)
        .with_extension(if cfg!(windows) { "obj" } else { "o" })
        .to_str()
        .expect("object path is valid UTF-8")
        .to_owned()
}

// Writes generated sources and compiles them for `-c` when requested.
fn compile_sources(
    temp_mgr: &TempFileManager,
    include_paths: &[String],
    sources: &[SourceInput],
    output: &Option<String>,
    flag_compile: bool,
) -> Result<Vec<String>, String> {
    let mut linker_inputs: Vec<String> = Vec::new();

    for source in sources {
        let path = match source {
            SourceInput::Transpiled { path, .. } | SourceInput::Native(path) => path,
        };
        if ["o", "obj", "a", "lib"].contains(
            &std::path::Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or_default(),
        ) {
            linker_inputs.push(path.clone());
            continue;
        }

        let source_path = match source {
            SourceInput::Transpiled { source, .. } => {
                let temp_path = temp_mgr.temp_file(".c");
                std::fs::write(&temp_path, source)
                    .map_err(|error| format!("can't write {temp_path}: {error}"))?;
                temp_path
            }
            SourceInput::Native(path) => path.clone(),
        };

        let mut build = compiler_build(temp_mgr, include_paths);
        build.file(&source_path);
        let compiled_objects = build
            .try_compile_intermediates()
            .map_err(|error| format!("compiler failed: {error}"))?;
        let compiled_object = compiled_objects
            .first()
            .expect("one source produces one object file");
        if flag_compile {
            let destination = output.clone().unwrap_or_else(|| object_path(path));
            std::fs::copy(compiled_object, &destination)
                .map_err(|error| format!("can't write {destination}: {error}"))?;
            continue;
        }

        linker_inputs.push(
            compiled_object
                .to_str()
                .expect("object path is valid UTF-8")
                .to_owned(),
        );
    }

    Ok(linker_inputs)
}

fn run_tool(cmd: &mut Command, description: &str) -> Result<(), String> {
    let status = cmd
        .status()
        .map_err(|error| format!("failed to run {description}: {error}"))?;
    if !status.success() {
        return Err(format!("{description} failed with {status}"));
    }
    Ok(())
}

// Compiles and links inputs, then optionally runs the resulting executable.
fn link_and_run(
    linker_inputs: &[String],
    output: &Option<String>,
    files: &[String],
    compiler: &cc::Tool,
    ld: Option<&str>,
    flag_run: bool,
    ldflags: &[String],
) -> Result<i32, String> {
    let exe_path = output.clone().unwrap_or_else(|| {
        let base = &files[0];
        format!(
            "{}{}",
            base.trim_suffix(".cc"),
            std::env::consts::EXE_SUFFIX
        )
    });

    let mut link_cmd = compiler.to_command();
    if let Some(ld) = ld {
        if compiler.is_like_msvc() {
            return Err("LD is not supported with an MSVC compiler".to_owned());
        }
        link_cmd.arg(format!("-fuse-ld={ld}"));
    }
    link_cmd.args(linker_inputs);
    if compiler.is_like_msvc() {
        link_cmd.arg(format!("/Fe{exe_path}"));
        if !ldflags.is_empty() {
            link_cmd.arg("/link").args(ldflags);
        }
    } else {
        link_cmd.args(ldflags).args(["-o", &exe_path]);
    }
    run_tool(&mut link_cmd, "linker")?;

    if flag_run {
        let executable = std::path::Path::new(&exe_path);
        let executable = if executable.is_absolute() {
            executable.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|error| format!("can't resolve executable path: {error}"))?
                .join(executable)
        };
        return Command::new(executable)
            .status()
            .map(|status| status.code().unwrap_or(1))
            .map_err(|error| format!("failed to run executable: {error}"));
    }
    Ok(0)
}

fn env_flags(name: &str) -> Vec<String> {
    std::env::var(name)
        .map(|flags| flags.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn run() -> Result<i32, String> {
    let args = parse_args();
    let ld = std::env::var("LD").ok();
    let ldflags = env_flags("LDFLAGS");

    let mut transpiler_include_paths = vec![".".to_owned()];
    transpiler_include_paths.extend(args.include_paths.clone());
    let mut transpiler = create_transpiler(&transpiler_include_paths);
    let sources = transpile_sources(&mut transpiler, &args.files)?;
    if args.flag_source {
        write_transpiled_sources(&sources, &args.output)?;
        return Ok(0);
    }

    let temp_mgr = TempFileManager::new();
    let std_temp_path = temp_mgr
        .base_dir()
        .to_str()
        .expect("std temp dir is valid UTF-8")
        .to_owned();
    let mut include_paths = vec![".".to_owned(), std_temp_path];
    include_paths.extend(args.include_paths.clone());

    setup_std_headers(&temp_mgr)?;

    let compiler = compiler_build(&temp_mgr, &include_paths)
        .try_get_compiler()
        .map_err(|error| format!("can't find C compiler: {error}"))?;
    let mut linker_inputs = compile_sources(
        &temp_mgr,
        &include_paths,
        &sources,
        &args.output,
        args.flag_compile,
    )?;
    if args.flag_compile {
        return Ok(0);
    }
    let std_archive_path = setup_std_archive(&temp_mgr, compiler.is_like_msvc())?;
    linker_inputs.push(std_archive_path);

    // Link and optionally run
    link_and_run(
        &linker_inputs,
        &args.output,
        &args.files,
        &compiler,
        ld.as_deref(),
        args.flag_run,
        &ldflags,
    )
}

fn main() {
    let exit_code = match run() {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    };
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}
