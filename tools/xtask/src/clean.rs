/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::env;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};

use crate::process::run;
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
        || (relative.starts_with(xp.join("guest"))
            && matches!(extension, Some("exe" | "sys" | "bmp")))
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

impl Xtask {
    pub(crate) fn clean(&self) -> Result<()> {
        if self.os == Os::Windows {
            // Windows cannot delete the running xtask executable.
            println!("Cleaning Cargo artifacts...");
            let target = self.root.join("target");
            if target.exists() {
                let executable = env::current_exe().context("failed to locate xtask executable")?;
                remove_directory_except(&target, &executable)?;
            }
        } else {
            run(Command::new("cargo").arg("clean"))?;
        }
        self.clean_generated_files()
    }

    fn clean_generated_files(&self) -> Result<()> {
        let mut generated = Vec::new();
        collect_paths(&self.root, &mut generated, &|path, is_dir| {
            is_generated_path(&self.root, path, is_dir)
        })?;
        for path in generated {
            println!("Removing {}", relative_slash(&self.root, &path));
            remove_path(&path)?;
        }
        Ok(())
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn clean_removes_build_outputs_and_preserves_inputs() -> Result<()> {
        let root = env::temp_dir().join(format!("xtask-clean-outputs-{}", std::process::id()));
        let generated = [
            "apps/web/bastiaan/winxp-image/guest/v86agent.exe",
            "apps/web/bastiaan/winxp-image/guest/v86setup.exe",
            "apps/web/bastiaan/winxp-image/guest/v86mou.sys",
            "apps/web/bastiaan/winxp-image/guest/v86tools.sys",
            "apps/web/bastiaan/winxp-image/guest/v86vid.sys",
            "apps/web/bastiaan/winxp-image/guest/activate/confid.exe",
            "apps/web/bastiaan/winxp-image/guest/user.bmp",
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
            "apps/web/bastiaan/winxp-image/guest/v86agent.c",
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
        xtask.clean_generated_files()?;
        for file in generated {
            assert!(!root.join(file).exists(), "generated file remains: {file}");
        }
        for file in preserved {
            assert!(root.join(file).exists(), "input was deleted: {file}");
        }
        // Cleaning an already clean tree must also succeed.
        xtask.clean_generated_files()?;
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
