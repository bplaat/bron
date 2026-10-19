/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use directories::{BaseDirs, UserDirs};
use serde_json::Value;

use crate::metadata::{InstallableApp, has_target_kind, packages, supports_os};
use crate::process::{npm_program, run};
use crate::utils::{
    collect_paths, copy_directory, copy_directory_contents, relative_slash,
    remove_directory_except, remove_path, remove_path_if_exists,
};
use crate::{Os, Xtask};

fn is_generated_database(root: &Path, path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    name.contains(".db")
        || (name.ends_with(".mmdb")
            && path != root.join("lib/net/maxminddb/test-data/GeoLite2-City-Test.mmdb"))
}

fn cargo_install_path(path: &str, force: bool) -> Result<()> {
    let mut offline = Command::new("cargo");
    offline.args(["install", "--locked", "--offline"]);
    if force {
        offline.arg("--force");
    }
    offline.args(["--path", path]);
    if run(&mut offline).is_ok() {
        return Ok(());
    }

    println!("Locked dependencies are not fully cached; retrying with network access...");
    let mut online = Command::new("cargo");
    online.args(["install", "--locked"]);
    if force {
        online.arg("--force");
    }
    online.args(["--path", path]);
    run(&mut online)
}

fn freedesktop_identity(path: &Path) -> Result<Option<(String, String)>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)?;
    let executable = contents.lines().find_map(|line| {
        let value = line.strip_prefix("Exec=")?.trim().trim_start_matches('"');
        let value = value.strip_prefix("$BIN_DIR/")?;
        value.split(['"', ' ']).next().map(str::to_owned)
    });
    let identifier = contents
        .lines()
        .find_map(|line| line.strip_prefix("Icon=").map(str::to_owned));
    Ok(executable.zip(identifier))
}

impl Xtask {
    pub(crate) fn clean(&self) -> Result<()> {
        if self.os == Os::Windows {
            // Windows does not allow `cargo clean` to delete the xtask executable
            // while it is running. Remove everything except that executable instead.
            println!("Cleaning Cargo artifacts...");
            let target = self.root.join("target");
            if target.exists() {
                let executable = env::current_exe().context("failed to locate xtask executable")?;
                remove_directory_except(&target, &executable)?;
            }
        } else {
            run(Command::new("cargo").arg("clean"))?;
        }
        let mut generated = Vec::new();
        collect_paths(&self.root, &mut generated, &|path, is_dir| {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if is_dir {
                (name == "target" && path != self.root.join("target"))
                    || matches!(
                        name,
                        "node_modules"
                            | "dist"
                            | "src-gen"
                            | "playwright"
                            | "playwright-report"
                            | "test-results"
                    )
            } else {
                is_generated_database(&self.root, path)
            }
        })?;
        for path in generated {
            println!("Removing {}", relative_slash(&self.root, &path));
            remove_path(&path)?;
        }
        Ok(())
    }

    pub(crate) fn build_pages(&self) -> Result<()> {
        let pages = self.root.join("target/pages");
        fs::create_dir_all(&pages)?;
        fs::copy(self.root.join("index.html"), pages.join("index.html"))?;
        self.build_pages_baksteen(&pages)?;
        self.build_pages_plaatui(&pages)
    }

    fn build_pages_baksteen(&self, pages: &Path) -> Result<()> {
        let destination = pages.join("baksteen");
        fs::create_dir_all(&destination)?;
        copy_directory_contents(&self.root.join("apps/web/baksteen/public"), &destination)?;
        run(Command::new("cargo").args([
            "build",
            "--release",
            "-p",
            "baksteen",
            "--target",
            "wasm32-unknown-unknown",
        ]))?;
        run(Command::new("wasm-bindgen").args([
            "--target",
            "web",
            "--no-typescript",
            "--out-dir",
            destination
                .to_str()
                .context("pages path is not valid UTF-8")?,
            "--out-name",
            "baksteen",
            "target/wasm32-unknown-unknown/release/baksteen.wasm",
        ]))
    }

    fn build_pages_plaatui(&self, pages: &Path) -> Result<()> {
        self.ensure_npm_deps()?;
        run(Command::new(npm_program(self.os)).args([
            "run",
            "build-release",
            "--workspace",
            "plaatui-showcase",
        ]))?;
        let destination = pages.join("plaatui");
        fs::create_dir_all(&destination)?;
        copy_directory_contents(
            &self.root.join("lib/ui/plaatui/examples/showcase/dist"),
            &destination,
        )
    }

    pub(crate) fn build_bundle(&self) -> Result<()> {
        let apps = self.installable_apps(true)?;
        self.build_bundles(&apps)
    }

    fn build_bundles(&self, apps: &[InstallableApp]) -> Result<()> {
        cargo_install_path("tools/cargo-bundle", false)?;
        for app in apps {
            run(Command::new("cargo")
                .arg("bundle")
                .arg("--path")
                .arg(&app.directory))?;
        }
        Ok(())
    }

    pub(crate) fn install(&self, selected: Option<&str>) -> Result<()> {
        let apps: Vec<_> = self
            .installable_apps(false)?
            .into_iter()
            .filter(|app| selected.is_none_or(|selected| app.package == selected))
            .collect();
        if let Some(selected) = selected
            && apps.is_empty()
        {
            bail!(
                "application bundle is not installable on {}: {selected}",
                self.os.name()
            );
        }

        if selected.is_none() {
            for path in ["tools/bob", "tools/ccontinue", "apps/desktop/music-dl"] {
                cargo_install_path(path, true)?;
            }
        }

        match self.os {
            Os::Macos => {
                if !apps.is_empty() {
                    self.build_bundles(&apps)?;
                }
                for app in apps {
                    let bundle_directory = self.root.join(format!("target/bundle/{}", app.package));
                    let source = fs::read_dir(&bundle_directory)?
                        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                        .find(|path| path.extension().is_some_and(|extension| extension == "app"))
                        .with_context(|| {
                            format!("no .app found in {}", bundle_directory.display())
                        })?;
                    let destination = Path::new("/Applications").join(
                        source
                            .file_name()
                            .context("application bundle has no file name")?,
                    );
                    remove_path_if_exists(&destination)?;
                    copy_directory(&source, &destination)?;
                }
            }
            Os::Windows => {
                let desktop = UserDirs::new()
                    .context("home directory is not available")?
                    .desktop_dir();
                for app in apps {
                    run(Command::new("cargo").args(["build", "--release", "--bin", &app.package]))?;
                    fs::copy(
                        self.root
                            .join(format!("target/release/{}.exe", app.package)),
                        desktop.join(format!("{}.exe", app.name)),
                    )?;
                }
            }
            Os::Linux => {
                let base_dirs = BaseDirs::new().context("home directory is not available")?;
                let data = base_dirs.data_dir();
                let bin = base_dirs
                    .executable_dir()
                    .context("executable directory is not available")?;
                let applications = data.join("applications");
                let icons = data.join("icons/hicolor");
                let mime_packages = data.join("mime/packages");
                fs::create_dir_all(&bin)?;
                fs::create_dir_all(&applications)?;
                fs::create_dir_all(&mime_packages)?;
                for app in apps {
                    run(Command::new("cargo").args(["build", "--release", "--bin", &app.package]))?;
                    let executable = bin.join(&app.name);
                    fs::copy(
                        self.root.join(format!("target/release/{}", app.package)),
                        &executable,
                    )?;
                    let desktop_template =
                        fs::read_to_string(app.directory.join("meta/freedesktop/.desktop"))?;
                    let desktop_entry =
                        desktop_template.replace("$BIN_DIR", &bin.to_string_lossy());
                    fs::write(
                        applications.join(format!("{}.desktop", app.identifier)),
                        desktop_entry,
                    )?;
                    let mime_package = app.directory.join("meta/freedesktop/mime.xml");
                    if mime_package.exists() {
                        fs::copy(
                            mime_package,
                            mime_packages.join(format!("{}.xml", app.identifier)),
                        )?;
                    }
                    for size in [16, 24, 32, 48, 64, 128, 256, 512] {
                        let destination = icons.join(format!("{size}x{size}/apps"));
                        fs::create_dir_all(&destination)?;
                        fs::copy(
                            app.directory
                                .join(format!("meta/freedesktop/icons/{size}x{size}.png")),
                            destination.join(format!("{}.png", app.identifier)),
                        )?;
                    }
                }
                run(Command::new("update-mime-database").arg(data.join("mime")))?;
                run(Command::new("update-desktop-database").arg(&applications))?;
                run(Command::new("gtk-update-icon-cache")
                    .args(["--force", "--ignore-theme-index"])
                    .arg(&icons))?;
            }
        }
        Ok(())
    }

    fn installable_apps(&self, bundle_only: bool) -> Result<Vec<InstallableApp>> {
        let metadata = self.cargo_metadata()?;
        let mut apps = Vec::new();
        for package in packages(&metadata)? {
            if !supports_os(package, self.os) {
                continue;
            }
            let manifest = package
                .get("manifest_path")
                .and_then(Value::as_str)
                .context("Cargo package has no manifest path")?;
            let package_directory = Path::new(manifest)
                .parent()
                .context("Cargo manifest has no parent directory")?;
            let is_example = package_directory
                .components()
                .any(|component| component.as_os_str() == "examples");
            if !has_target_kind(package, &["bin"]) || is_example {
                continue;
            }
            let bundle = package.pointer("/metadata/bundle");
            let uses_windowing = package
                .get("dependencies")
                .and_then(Value::as_array)
                .is_some_and(|dependencies| {
                    dependencies.iter().any(|dependency| {
                        matches!(
                            dependency.get("name").and_then(Value::as_str),
                            Some("bwindow" | "bwebview")
                        )
                    })
                });
            if bundle_only && bundle.is_none() || bundle.is_none() && !uses_windowing {
                continue;
            }
            let package_name = package
                .get("name")
                .and_then(Value::as_str)
                .context("Cargo package has no name")?;
            let mut name_characters = package_name.chars();
            let default_name = name_characters
                .next()
                .map(|first| first.to_uppercase().chain(name_characters).collect())
                .unwrap_or_default();
            let identity =
                freedesktop_identity(&package_directory.join("meta/freedesktop/.desktop"))?;
            let (mut name, identifier) = identity.unwrap_or_else(|| {
                let identifier = package_name.to_owned();
                (default_name, identifier)
            });
            if let Some(app_name) = package
                .pointer("/metadata/app/name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
            {
                name = app_name.to_owned();
            }
            apps.push(InstallableApp {
                package: package_name.to_owned(),
                directory: package_directory.to_path_buf(),
                identifier,
                name,
            });
        }
        Ok(apps)
    }
}
