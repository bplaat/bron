/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [winresource](https://crates.io/crates/winresource) crate

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io};

use crate::version::MicrosoftVersion;

mod version;

/// Windows resource compiler supporting MSVC, LLVM, MinGW and Zig, with optional `RC` override.
pub struct WindowsResource {
    icon_path: Option<PathBuf>,
    manifest: Option<String>,
    version_fields: BTreeMap<String, String>,
}

impl WindowsResource {
    /// Create a new Windows resource compiler
    #[allow(clippy::new_without_default)] // Upstream has no Default impl
    pub fn new() -> Self {
        let mut version_fields = BTreeMap::new();
        version_fields.insert(
            "FileVersion".to_string(),
            env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set"),
        );
        version_fields.insert(
            "ProductVersion".to_string(),
            env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set"),
        );
        version_fields.insert(
            "FileDescription".to_string(),
            env::var("CARGO_PKG_NAME").expect("CARGO_PKG_NAME not set"),
        );
        version_fields.insert(
            "ProductName".to_string(),
            env::var("CARGO_PKG_NAME").expect("CARGO_PKG_NAME not set"),
        );

        Self {
            icon_path: None,
            manifest: None,
            version_fields,
        }
    }

    /// Set a version field
    pub fn set(&mut self, name: &str, value: &str) -> &mut Self {
        self.version_fields
            .insert(name.to_string(), value.to_string());
        self
    }

    /// Set the icon file
    pub fn set_icon(&mut self, path: &str) -> &mut Self {
        self.icon_path = Some(PathBuf::from(path));
        self
    }

    /// Set the manifest content
    pub fn set_manifest(&mut self, manifest: &str) -> &mut Self {
        self.manifest = Some(manifest.to_string());
        self
    }

    /// Compile the resources
    pub fn compile(&self) -> io::Result<()> {
        println!("cargo:rerun-if-env-changed=RC");
        println!("cargo:rerun-if-env-changed=RUSTC_LINKER");
        println!("cargo:rerun-if-env-changed=ProgramFiles(x86)");
        let out_dir = env::var("OUT_DIR").expect("OUT_DIR environment variable not set");

        // Write manifest file
        if let Some(manifest) = &self.manifest {
            fs::write(Path::new(&out_dir).join("manifest.xml"), manifest)?;
        }

        // Write resource.rc file
        let mut rc_content = "#pragma code_page(65001)\r\n\r\n".to_string();

        if let Some(icon_path) = &self.icon_path {
            rc_content.push_str(&format!(
                "1 ICON \"{}\"\r\n\r\n",
                escape_string(&icon_path.display().to_string())
            ));
        }

        if self.manifest.is_some() {
            rc_content.push_str(&format!(
                "1 24 \"{}\"\r\n\r\n",
                escape_string(
                    &Path::new(&out_dir)
                        .join("manifest.xml")
                        .display()
                        .to_string()
                )
            ));
        }

        let version = |part: &str| {
            let name = format!("CARGO_PKG_VERSION_{part}");
            env::var(&name).unwrap_or_else(|_| panic!("{name} not set"))
        };
        rc_content.push_str(&format!(
            "1 VERSIONINFO\r\n\
        FILEVERSION {maj},{min},{pat},0\r\n\
        PRODUCTVERSION {maj},{min},{pat},0\r\n\
        FILEOS 0x00040004\r\n\
        FILETYPE 1\r\n\
        FILESUBTYPE 0\r\n\
        FILEFLAGSMASK 0x3F\r\n\
        FILEFLAGS 0\r\n\
        BEGIN\r\n\
          BLOCK \"StringFileInfo\"\r\n\
          BEGIN\r\n\
            BLOCK \"040904B0\"\r\n\
            BEGIN\r\n",
            maj = version("MAJOR"),
            min = version("MINOR"),
            pat = version("PATCH")
        ));
        for (k, v) in &self.version_fields {
            rc_content.push_str(&format!(
                "VALUE \"{k}\", \"{val}\"\n",
                k = escape_string(k),
                val = escape_string(v)
            ));
        }
        rc_content.push_str(
            "END\r\n\
          END\r\n\
          BLOCK \"VarFileInfo\"\r\n\
          BEGIN\r\n\
            VALUE \"Translation\", 0x0409, 0x04B0\r\n\
          END\r\n\
        END\n",
        );

        let rc_path = Path::new(&out_dir).join("resource.rc");
        fs::write(&rc_path, rc_content)?;

        // Compile resource.rc
        let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
        let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
        let compilers = resource_compilers(
            &target_env,
            &target_arch,
            env::var_os("RC"),
            find_rc_exe(),
            &env::var("RUSTC_LINKER").unwrap_or_default(),
        )
        .map_err(io::Error::other)?;
        let (resource_path, needs_link_search) =
            run_resource_compiler(&compilers, Path::new(&out_dir), &rc_path)
                .map_err(io::Error::other)?;
        if needs_link_search {
            println!("cargo:rustc-link-search=native={out_dir}");
            println!("cargo:rustc-link-lib=static=resource");
        } else {
            println!("cargo:rustc-link-arg={}", resource_path.display());
        }
        Ok(())
    }
}

fn find_rc_exe() -> Option<PathBuf> {
    let kit_root = env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"))
        .join(r"Windows Kits\10\bin");
    if !kit_root.exists() {
        return None;
    }

    let arch = if cfg!(target_arch = "x86_64") {
        "x64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86"
    };

    let mut best_version: Option<MicrosoftVersion> = None;
    let mut best_path: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&kit_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            if let Some(version_str) = path.file_name().and_then(|s| s.to_str())
                && let Ok(version) = MicrosoftVersion::parse(version_str)
            {
                let rc_path = path.join(arch).join("rc.exe");
                if rc_path.exists()
                    && (best_version.is_none()
                        || &version > best_version.as_ref().expect("Should be some"))
                {
                    best_version = Some(version.clone());
                    best_path = Some(rc_path);
                }
            }
        }
    }
    best_path
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ResourceCompiler {
    Rc(PathBuf),
    Windres {
        program: PathBuf,
        target: Option<String>,
    },
    Zig {
        program: PathBuf,
        target: Option<String>,
    },
}

impl ResourceCompiler {
    fn program(&self) -> &Path {
        match self {
            Self::Rc(program) | Self::Windres { program, .. } | Self::Zig { program, .. } => {
                program
            }
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Rc(program) => program.display().to_string(),
            Self::Zig { program, .. } => format!("{} rc", program.display()),
            Self::Windres {
                program, target, ..
            } => target.as_ref().map_or_else(
                || program.display().to_string(),
                |target| format!("{} --target {target}", program.display()),
            ),
        }
    }

    fn output_path(&self, out_dir: &Path) -> PathBuf {
        match self {
            Self::Windres { .. }
            | Self::Zig {
                target: Some(_), ..
            } => out_dir.join("resource.o"),
            Self::Rc(_) | Self::Zig { target: None, .. } => out_dir.join("resource.lib"),
        }
    }

    fn command(&self, out_dir: &Path, rc_path: &Path) -> Command {
        let output_path = self.output_path(out_dir);
        let mut command = Command::new(self.program());
        match self {
            Self::Rc(_) => {
                command.arg("/fo").arg(output_path).arg(rc_path);
            }
            Self::Windres { target, .. } => {
                if let Some(target) = target {
                    command.arg("--target").arg(target);
                }
                command
                    .arg(rc_path)
                    .arg("-O")
                    .arg("coff")
                    .arg("-o")
                    .arg(output_path);
            }
            Self::Zig { target, .. } => {
                command.arg("rc");
                if let Some(target) = target {
                    command
                        .arg("/:output-format")
                        .arg("coff")
                        .arg("/:target")
                        .arg(target);
                }
                command.arg("/fo").arg(output_path).arg(rc_path);
            }
        }
        command
    }
}

fn zig_resource_compiler(target_env: &str, target_arch: &str) -> ResourceCompiler {
    ResourceCompiler::Zig {
        program: PathBuf::from("zig"),
        target: (target_env == "gnu").then(|| target_arch.to_string()),
    }
}

fn compiler_from_override(
    program: OsString,
    target_env: &str,
    target_arch: &str,
) -> Result<ResourceCompiler, String> {
    let path = PathBuf::from(program);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.contains("zig") {
        Ok(ResourceCompiler::Zig {
            program: path,
            target: (target_env == "gnu").then(|| target_arch.to_string()),
        })
    } else if target_env == "gnu" && name.contains("llvm-rc") {
        Err("RC=llvm-rc is incompatible with GNU targets; use llvm-windres".to_string())
    } else if name.contains("windres") || target_env == "gnu" {
        Ok(ResourceCompiler::Windres {
            program: path,
            target: None,
        })
    } else {
        Ok(ResourceCompiler::Rc(path))
    }
}

fn push_unique(compilers: &mut Vec<ResourceCompiler>, compiler: ResourceCompiler) {
    if !compilers.contains(&compiler) {
        compilers.push(compiler);
    }
}

fn resource_compilers(
    target_env: &str,
    target_arch: &str,
    rc_override: Option<OsString>,
    rc_exe: Option<PathBuf>,
    rustc_linker: &str,
) -> Result<Vec<ResourceCompiler>, String> {
    let mut compilers = Vec::new();
    if let Some(program) = rc_override {
        push_unique(
            &mut compilers,
            compiler_from_override(program, target_env, target_arch)?,
        );
    }
    if rustc_linker.to_ascii_lowercase().contains("zig") {
        push_unique(
            &mut compilers,
            zig_resource_compiler(target_env, target_arch),
        );
    }

    match target_env {
        "msvc" => {
            if let Some(program) = rc_exe {
                push_unique(&mut compilers, ResourceCompiler::Rc(program));
            }
            push_unique(
                &mut compilers,
                ResourceCompiler::Rc(PathBuf::from("rc.exe")),
            );
            push_unique(
                &mut compilers,
                ResourceCompiler::Rc(PathBuf::from("llvm-rc")),
            );
        }
        "gnu" => {
            let (prefix, bfd_target) = match target_arch {
                "aarch64" => (Some("aarch64"), Some("pe-aarch64-little")),
                "x86" => (Some("i686"), Some("pe-i386")),
                "x86_64" => (Some("x86_64"), Some("pe-x86-64")),
                _ => (None, None),
            };
            if let Some(prefix) = prefix {
                push_unique(
                    &mut compilers,
                    ResourceCompiler::Windres {
                        program: PathBuf::from(format!("{prefix}-w64-mingw32-windres")),
                        target: None,
                    },
                );
                push_unique(
                    &mut compilers,
                    ResourceCompiler::Windres {
                        program: PathBuf::from("llvm-windres"),
                        target: Some(format!("{prefix}-w64-mingw32")),
                    },
                );
            }
            push_unique(
                &mut compilers,
                ResourceCompiler::Windres {
                    program: PathBuf::from("windres"),
                    target: bfd_target.map(str::to_string),
                },
            );
        }
        other => return Err(format!("unsupported target environment: {other}")),
    }

    push_unique(
        &mut compilers,
        zig_resource_compiler(target_env, target_arch),
    );
    Ok(compilers)
}

enum ResourceCompilerStatus {
    Success,
    Failed(Option<i32>),
}

fn try_resource_compilers(
    compilers: &[ResourceCompiler],
    mut run: impl FnMut(&ResourceCompiler) -> io::Result<ResourceCompilerStatus>,
) -> Result<usize, String> {
    for (index, compiler) in compilers.iter().enumerate() {
        match run(compiler) {
            Ok(ResourceCompilerStatus::Success) => return Ok(index),
            Ok(ResourceCompilerStatus::Failed(code)) => {
                return Err(format!(
                    "{} failed with exit code: {}",
                    compiler.description(),
                    code.unwrap_or(-1)
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "failed to execute {}: {error}",
                    compiler.description()
                ));
            }
        }
    }

    Err(format!(
        "failed to find a Windows resource compiler; tried: {}",
        compilers
            .iter()
            .map(ResourceCompiler::description)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn run_resource_compiler(
    compilers: &[ResourceCompiler],
    out_dir: &Path,
    rc_path: &Path,
) -> Result<(PathBuf, bool), String> {
    let index = try_resource_compilers(compilers, |compiler| {
        compiler.command(out_dir, rc_path).status().map(|status| {
            if status.success() {
                ResourceCompilerStatus::Success
            } else {
                ResourceCompilerStatus::Failed(status.code())
            }
        })
    })?;
    let compiler = &compilers[index];
    Ok((
        compiler.output_path(out_dir),
        matches!(
            compiler,
            ResourceCompiler::Rc(_) | ResourceCompiler::Zig { target: None, .. }
        ),
    ))
}

fn escape_string(string: &str) -> String {
    let mut escaped = String::new();
    for chr in string.chars() {
        match chr {
            '"' => escaped.push_str("\"\""),
            '\'' => escaped.push_str("\\'"),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\t' => escaped.push_str("\\t"),
            '\r' => escaped.push_str("\\r"),
            _ => escaped.push(chr),
        };
    }
    escaped
}

// MARK: Tests
#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn string_escaping() {
        assert_eq!(&escape_string(""), "");
        assert_eq!(&escape_string("foo"), "foo");
        assert_eq!(&escape_string(r#""Hello""#), r#"""Hello"""#);
        assert_eq!(
            &escape_string(r"C:\Program Files\Foobar"),
            r"C:\\Program Files\\Foobar"
        );
    }

    #[test]
    fn msvc_resource_compiler_order() {
        let rc_exe = PathBuf::from(r"C:\Windows Kits\rc.exe");
        assert_eq!(
            resource_compilers("msvc", "aarch64", None, Some(rc_exe.clone()), ""),
            Ok(vec![
                ResourceCompiler::Rc(rc_exe),
                ResourceCompiler::Rc(PathBuf::from("rc.exe")),
                ResourceCompiler::Rc(PathBuf::from("llvm-rc")),
                ResourceCompiler::Zig {
                    program: PathBuf::from("zig"),
                    target: None,
                },
            ])
        );
    }

    #[test]
    fn gnu_resource_compiler_order_includes_aarch64_and_llvm() {
        assert_eq!(
            resource_compilers("gnu", "aarch64", None, None, ""),
            Ok(vec![
                ResourceCompiler::Windres {
                    program: PathBuf::from("aarch64-w64-mingw32-windres"),
                    target: None,
                },
                ResourceCompiler::Windres {
                    program: PathBuf::from("llvm-windres"),
                    target: Some("aarch64-w64-mingw32".to_string()),
                },
                ResourceCompiler::Windres {
                    program: PathBuf::from("windres"),
                    target: Some("pe-aarch64-little".to_string()),
                },
                ResourceCompiler::Zig {
                    program: PathBuf::from("zig"),
                    target: Some("aarch64".to_string()),
                },
            ])
        );
    }

    #[test]
    fn resource_compiler_override_and_zig_linker_are_preferred() {
        assert_eq!(
            resource_compilers(
                "gnu",
                "x86_64",
                Some(OsString::from("custom-windres")),
                None,
                ""
            ),
            Ok(vec![
                ResourceCompiler::Windres {
                    program: PathBuf::from("custom-windres"),
                    target: None,
                },
                ResourceCompiler::Windres {
                    program: PathBuf::from("x86_64-w64-mingw32-windres"),
                    target: None,
                },
                ResourceCompiler::Windres {
                    program: PathBuf::from("llvm-windres"),
                    target: Some("x86_64-w64-mingw32".to_string()),
                },
                ResourceCompiler::Windres {
                    program: PathBuf::from("windres"),
                    target: Some("pe-x86-64".to_string()),
                },
                ResourceCompiler::Zig {
                    program: PathBuf::from("zig"),
                    target: Some("x86_64".to_string()),
                },
            ])
        );
        assert_eq!(
            resource_compilers("msvc", "x86_64", None, None, "zig-linker-wrapper"),
            Ok(vec![
                ResourceCompiler::Zig {
                    program: PathBuf::from("zig"),
                    target: None,
                },
                ResourceCompiler::Rc(PathBuf::from("rc.exe")),
                ResourceCompiler::Rc(PathBuf::from("llvm-rc")),
            ])
        );
    }

    #[test]
    fn resource_compiler_commands_use_the_correct_interface() {
        let out_dir = Path::new("out");
        let rc_path = Path::new("resource.rc");
        let commands = [
            ResourceCompiler::Rc(PathBuf::from("llvm-rc")).command(out_dir, rc_path),
            ResourceCompiler::Windres {
                program: PathBuf::from("llvm-windres"),
                target: Some("aarch64-w64-mingw32".to_string()),
            }
            .command(out_dir, rc_path),
            ResourceCompiler::Zig {
                program: PathBuf::from("zig"),
                target: None,
            }
            .command(out_dir, rc_path),
            ResourceCompiler::Zig {
                program: PathBuf::from("zig"),
                target: Some("aarch64".to_string()),
            }
            .command(out_dir, rc_path),
        ];
        let args = commands
            .iter()
            .map(|command| command.get_args().map(OsString::from).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(
            args[0],
            [
                OsString::from("/fo"),
                out_dir.join("resource.lib").into_os_string(),
                rc_path.as_os_str().to_os_string(),
            ]
        );
        assert_eq!(
            args[1],
            [
                OsString::from("--target"),
                OsString::from("aarch64-w64-mingw32"),
                rc_path.as_os_str().to_os_string(),
                OsString::from("-O"),
                OsString::from("coff"),
                OsString::from("-o"),
                out_dir.join("resource.o").into_os_string(),
            ]
        );
        assert_eq!(
            args[2],
            [
                OsString::from("rc"),
                OsString::from("/fo"),
                out_dir.join("resource.lib").into_os_string(),
                rc_path.as_os_str().to_os_string(),
            ]
        );
        assert_eq!(
            args[3],
            [
                OsString::from("rc"),
                OsString::from("/:output-format"),
                OsString::from("coff"),
                OsString::from("/:target"),
                OsString::from("aarch64"),
                OsString::from("/fo"),
                out_dir.join("resource.o").into_os_string(),
                rc_path.as_os_str().to_os_string(),
            ]
        );
    }

    #[test]
    fn llvm_rc_override_is_rejected_for_gnu_targets() {
        assert_eq!(
            resource_compilers("gnu", "aarch64", Some(OsString::from("llvm-rc")), None, ""),
            Err("RC=llvm-rc is incompatible with GNU targets; use llvm-windres".to_string())
        );
    }

    #[test]
    fn missing_resource_compiler_falls_back_to_next() {
        let compilers = vec![
            ResourceCompiler::Rc(PathBuf::from("rc.exe")),
            ResourceCompiler::Rc(PathBuf::from("llvm-rc")),
        ];
        let mut attempts = Vec::new();
        let result = try_resource_compilers(&compilers, |compiler| {
            attempts.push(compiler.clone());
            if compiler.program() == Path::new("llvm-rc") {
                Ok(ResourceCompilerStatus::Success)
            } else {
                Err(io::Error::from(io::ErrorKind::NotFound))
            }
        });
        assert_eq!(result, Ok(1));
        assert_eq!(attempts, compilers);
    }

    #[test]
    fn failed_resource_compiler_does_not_fall_back() {
        let compilers = vec![
            ResourceCompiler::Rc(PathBuf::from("rc.exe")),
            ResourceCompiler::Rc(PathBuf::from("llvm-rc")),
        ];
        let mut attempts = Vec::new();
        let result = try_resource_compilers(&compilers, |compiler| {
            attempts.push(compiler.clone());
            Ok(ResourceCompilerStatus::Failed(Some(1)))
        });
        assert_eq!(result, Err("rc.exe failed with exit code: 1".to_string()));
        assert_eq!(attempts, [ResourceCompiler::Rc(PathBuf::from("rc.exe"))]);
    }

    #[test]
    fn missing_resource_compilers_are_reported() {
        let compilers = vec![
            ResourceCompiler::Rc(PathBuf::from("rc.exe")),
            ResourceCompiler::Rc(PathBuf::from("llvm-rc")),
            ResourceCompiler::Zig {
                program: PathBuf::from("zig"),
                target: None,
            },
        ];
        let result = try_resource_compilers(&compilers, |_| {
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert_eq!(
            result,
            Err(
                "failed to find a Windows resource compiler; tried: rc.exe, llvm-rc, zig rc"
                    .to_string()
            )
        );
    }
}
