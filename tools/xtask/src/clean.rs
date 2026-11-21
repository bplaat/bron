/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result};

use crate::process::{capture, run};
use crate::utils::{collect_paths, relative_slash, remove_directory_except, remove_path};
use crate::{Os, Xtask};

fn is_generated_path(root: &Path, path: &Path, is_dir: bool) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let extension = path.extension().and_then(|extension| extension.to_str());
    let xp = Path::new("apps/web/bastiaan/winxp-image");
    let macos = Path::new("examples/macos");

    if is_dir {
        return (name == "target" && relative != Path::new("target"))
            || matches!(
                name,
                "node_modules"
                    | "dist"
                    | "src-gen"
                    | "playwright"
                    | "playwright-report"
                    | "test-results"
            )
            || relative == xp.join("out")
            || relative == Path::new("apps/web/bastiaan/web/public/winxp")
            || (relative.parent().and_then(Path::parent) == Some(xp.join("software").as_path())
                && matches!(name, "root" | "extract"))
            || relative == xp.join("software/mitec/files")
            || (relative.starts_with(macos) && extension == Some("app"));
    }

    // Installers, CD contents, music and archives under winxp-image are build inputs.
    name.contains(".db")
        || (extension == Some("mmdb")
            && relative != Path::new("lib/net/maxminddb/test-data/GeoLite2-City-Test.mmdb"))
        || [
            "software/bastiaan/bastiaan.exe",
            "software/reversi/reversi.exe",
        ]
        .iter()
        .any(|file| relative == xp.join(file))
        || (relative.starts_with("examples/windows") && extension == Some("exe"))
        || (relative.starts_with(macos.join("portable-executable")) && extension == Some("com"))
        || (relative.starts_with(macos)
            && extension.is_none()
            && path.with_extension("s").is_file())
}

fn path_size(path: &Path) -> Result<u64> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to inspect {}", path.display()));
        }
    };
    if metadata.is_dir() {
        let entries =
            fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))?;
        entries.map(|entry| path_size(&entry?.path())).sum()
    } else if metadata.is_file() {
        Ok(metadata.len())
    } else {
        Ok(0)
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} {}", if bytes == 1 { "byte" } else { "bytes" });
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = "KiB";
    for next in ["MiB", "GiB", "TiB", "PiB", "EiB"] {
        if size < 1024.0 {
            break;
        }
        size /= 1024.0;
        unit = next;
    }
    format!("{size:.2} {unit}")
}

impl Xtask {
    pub(crate) fn clean(&self) -> Result<()> {
        let bytes = self.clean_cargo_artifacts()? + self.clean_generated_files()?;
        println!("Freed {}", format_bytes(bytes));
        Ok(())
    }

    fn clean_cargo_artifacts(&self) -> Result<u64> {
        if self.os == Os::Windows {
            // Windows cannot delete the running xtask executable.
            println!("Cleaning Cargo artifacts...");
            let target = self.root.join("target");
            if target.exists() {
                let executable = env::current_exe().context("failed to locate xtask executable")?;
                return remove_directory_except(&target, &executable);
            }
            return Ok(0);
        }

        let output = capture(Command::new("cargo").current_dir(&self.root).args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
        ]))?;
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)?;
        let target = Path::new(
            metadata["target_directory"]
                .as_str()
                .context("Cargo metadata has no target directory")?,
        );
        let build = Path::new(
            metadata["build_directory"]
                .as_str()
                .context("Cargo metadata has no build directory")?,
        );
        let artifact_size = || {
            if build.starts_with(target) {
                path_size(target)
            } else if target.starts_with(build) {
                path_size(build)
            } else {
                Ok(path_size(target)? + path_size(build)?)
            }
        };
        let before = artifact_size()?;
        run(Command::new("cargo")
            .current_dir(&self.root)
            .args(["clean", "--quiet"]))?;
        Ok(before.saturating_sub(artifact_size()?))
    }

    fn clean_generated_files(&self) -> Result<u64> {
        let mut generated = Vec::new();
        collect_paths(&self.root, &mut generated, &|path, is_dir| {
            is_generated_path(&self.root, path, is_dir)
        })?;
        let mut bytes = 0;
        for path in generated {
            println!("Removing {}", relative_slash(&self.root, &path));
            let size = path_size(&path)?;
            remove_path(&path)?;
            bytes += size;
        }
        Ok(bytes)
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_summary_formats_mebibytes_and_gibibytes() {
        assert_eq!(format_bytes(1_572_864), "1.50 MiB");
        assert_eq!(format_bytes(2_684_354_560), "2.50 GiB");
    }

    #[test]
    fn clean_removes_build_outputs_and_preserves_inputs() -> Result<()> {
        let root = env::temp_dir().join(format!("xtask-clean-outputs-{}", std::process::id()));
        let generated = [
            "apps/web/bastiaan/winxp-image/guest/target/i686-winxp-windows-gnu/release/v86agent.exe",
            "apps/web/bastiaan/winxp-image/guest/target/i686-winxp-windows-driver/release/v86vid.sys",
            "apps/web/bastiaan/winxp-image/out/disk.img",
            "apps/web/bastiaan/winxp-image/software/7zip/root/Program Files/7-Zip/7z.exe",
            "apps/web/bastiaan/winxp-image/software/civilization-3/extract/app/data",
            "apps/web/bastiaan/winxp-image/software/mitec/files/EXE.exe",
            "apps/web/bastiaan/winxp-image/software/bastiaan/bastiaan.exe",
            "apps/web/bastiaan/winxp-image/software/reversi/reversi.exe",
            "apps/web/bastiaan/web/public/winxp/disk.json",
            "examples/windows/hello-x86.exe",
            "examples/windows/redsquare/redsquare-x64.exe",
            "examples/macos/hello-arm64",
            "examples/macos/alert/asm/alert",
            "examples/macos/portable-executable/hello.com",
            "examples/macos/window/c/Window.app/Contents/MacOS/Window",
            "examples/wii/cube/c/target/cube.dol",
            "apps/web/example/dist/index.html",
            "node_modules/example/index.js",
            "tmp/data.db-wal",
            "tmp/download.mmdb",
        ];
        let preserved = [
            "apps/web/bastiaan/winxp-image/winxp.iso",
            "apps/web/bastiaan/winxp-image/music/song.mp3",
            "apps/web/bastiaan/winxp-image/software/7zip/7z2605.exe",
            "apps/web/bastiaan/winxp-image/software/civilization-3/setup_civilization3_complete_2.0.0.7.exe",
            "apps/web/bastiaan/winxp-image/software/age-of-empires/cd/AOE1/AOESETUP.EXE",
            "apps/web/bastiaan/winxp-image/software/internet-explorer-8/IE8-WindowsXP-x86-ENU.exe",
            "apps/web/bastiaan/winxp-image/software/notepad-plus-plus/npp.7.9.2.Installer.exe",
            "apps/web/bastiaan/winxp-image/software/mitec/EXE.zip",
            "apps/web/bastiaan/winxp-image/software/age-of-empires/cd/root/data",
            "apps/web/bastiaan/winxp-image/guest/src/protocol.rs",
            "apps/web/bastiaan/winxp-image/guest/user.bmp",
            "apps/web/bastiaan/winxp-image/guest/v86vid.inf",
            "examples/windows/hello.asm",
            "examples/macos/hello-arm64.s",
            "examples/macos/alert/asm/alert.s",
            "examples/macos/portable-executable/hello.s",
            "lib/net/maxminddb/test-data/GeoLite2-City-Test.mmdb",
            "target/debug/xtask.exe",
            ".git/objects/example",
        ];
        for file in generated.iter().chain(&preserved) {
            let path = root.join(file);
            fs::create_dir_all(path.parent().context("fixture has no parent")?)?;
            fs::write(path, "fixture")?;
        }
        let xtask = Xtask {
            root: root.clone(),
            os: Os::detect()?,
        };
        assert_eq!(xtask.clean_generated_files()?, 7 * generated.len() as u64);
        for file in generated {
            assert!(!root.join(file).exists(), "generated file remains: {file}");
        }
        for file in preserved {
            assert!(root.join(file).exists(), "input was deleted: {file}");
        }
        // Cleaning an already clean tree must also succeed.
        assert_eq!(xtask.clean_generated_files()?, 0);
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
