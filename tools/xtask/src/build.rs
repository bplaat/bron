/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result, bail};
use directories::{BaseDirs, UserDirs};
use serde_json::Value;

use crate::metadata::{InstallableApp, has_target_kind, packages, supports_os};
use crate::process::{capture, npm_program, run};
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

fn cargo_install_path(path: &str) -> Result<()> {
    let mut offline = Command::new("cargo");
    offline.args([
        "install",
        "--locked",
        "--offline",
        "--force",
        "--path",
        path,
    ]);
    if run(&mut offline).is_ok() {
        return Ok(());
    }

    println!("Locked dependencies are not fully cached; retrying with network access...");
    let mut online = Command::new("cargo");
    online.args(["install", "--locked", "--force", "--path", path]);
    run(&mut online)
}

const ERROR_CANCELLED: i32 = 1223;

fn powershell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
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
        self.build_bundles(&apps, &[])
    }

    fn build_bundles(&self, apps: &[InstallableApp], args: &[&str]) -> Result<()> {
        // Run the workspace version, `cargo bundle` could pick a stale cargo-bundle from PATH
        for app in apps {
            run(Command::new("cargo")
                .args([
                    "run",
                    "--release",
                    "--quiet",
                    "-p",
                    "cargo-bundle",
                    "--",
                    "bundle",
                ])
                .args(args)
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
                cargo_install_path(path)?;
            }
        }

        match self.os {
            Os::Macos => {
                if !apps.is_empty() {
                    self.build_bundles(&apps, &[])?;
                }
                for app in apps {
                    let source = self.bundle_output(&app, "", "app")?;
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
                if !apps.is_empty() {
                    self.build_bundles(&apps, &[])?;
                    let packages = apps
                        .iter()
                        .map(|app| self.bundle_output(app, "", "msixbundle"))
                        .collect::<Result<Vec<_>>>()?;
                    self.install_msix_packages(&packages)?;
                }
                // Remove executables that older versions of this task copied to the desktop
                let desktop = UserDirs::new()
                    .context("home directory is not available")?
                    .desktop_dir()
                    .to_owned();
                for app in apps {
                    remove_path_if_exists(&desktop.join(format!("{}.exe", app.name)))?;
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
                let thumbnailers = data.join("thumbnailers");
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
                    let thumbnailer = app.directory.join("meta/freedesktop/.thumbnailer");
                    if thumbnailer.exists() {
                        fs::create_dir_all(&thumbnailers)?;
                        fs::write(
                            thumbnailers.join(format!("{}.thumbnailer", app.identifier)),
                            fs::read_to_string(thumbnailer)?
                                .replace("$BIN_DIR", &bin.to_string_lossy()),
                        )?;
                    }
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

    pub(crate) fn install_ios(&self, selected: Option<&str>) -> Result<()> {
        if self.os != Os::Macos {
            bail!("iOS apps can only be installed on macOS");
        }
        let apps: Vec<_> = self
            .installable_apps(true)?
            .into_iter()
            .filter(|app| app.ios && selected.is_none_or(|selected| app.package == selected))
            .collect();
        if let Some(selected) = selected
            && apps.is_empty()
        {
            bail!("no iOS application bundle: {selected}");
        }

        let output =
            capture(Command::new("xcrun").args(["simctl", "list", "devices", "booted", "--json"]))?;
        let devices: Value = serde_json::from_slice(&output.stdout)?;
        let simulators: Vec<_> = devices
            .get("devices")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|runtimes| runtimes.values())
            .filter_map(Value::as_array)
            .flatten()
            .filter_map(|device| device.get("udid").and_then(Value::as_str))
            .collect();
        if simulators.is_empty() {
            bail!("no booted iOS simulator, start one with Simulator.app");
        }

        self.build_bundles(&apps, &["--ios"])?;
        for app in apps {
            let source = self.bundle_output(&app, "ios", "app")?;
            for simulator in &simulators {
                run(Command::new("xcrun")
                    .args(["simctl", "install", simulator])
                    .arg(&source))?;
            }
        }
        Ok(())
    }

    pub(crate) fn install_android(&self, selected: Option<&str>) -> Result<()> {
        let apps: Vec<_> = self
            .installable_apps(true)?
            .into_iter()
            .filter(|app| app.android && selected.is_none_or(|selected| app.package == selected))
            .collect();
        if let Some(selected) = selected
            && apps.is_empty()
        {
            bail!("no Android application bundle: {selected}");
        }

        let adb = android_build::adb();
        let output = capture(Command::new(&adb).arg("devices"))?;
        let device = String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(1)
            .find_map(|line| line.strip_suffix("\tdevice").map(str::to_owned))
            .context("no Android device connected, start an emulator or connect a device")?;

        self.build_bundles(&apps, &["--android"])?;
        for app in apps {
            let apk = self.bundle_output(&app, "", "apk")?;
            run(Command::new(&adb)
                .args(["-s", &device, "install", "-r"])
                .arg(&apk))?;
        }
        Ok(())
    }

    fn bundle_output(
        &self,
        app: &InstallableApp,
        subdirectory: &str,
        extension: &str,
    ) -> Result<PathBuf> {
        let bundle_directory = self
            .root
            .join(format!("target/bundle/{}", app.package))
            .join(subdirectory);
        fs::read_dir(&bundle_directory)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .find(|path| path.extension().is_some_and(|ext| ext == extension))
            .with_context(|| format!("no .{extension} found in {}", bundle_directory.display()))
    }

    fn install_msix_packages(&self, packages: &[PathBuf]) -> Result<()> {
        // Unsigned packages with executables can only be installed by an administrator, so all
        // packages are installed from one elevated PowerShell to only show one UAC prompt
        let log = self.root.join("target/bundle/install-msix.log");
        let script = self.root.join("target/bundle/install-msix.ps1");
        remove_path_if_exists(&log)?;
        // Windows fails to reinstall a bundle with the installed version, while the version only
        // stays the same when the contents didn't change, so those bundles are skipped
        let mut contents = format!(
            r"$ErrorActionPreference = 'Stop'
trap {{ $_ | Out-File -LiteralPath {} -Encoding utf8; exit 1 }}
Add-Type -AssemblyName System.IO.Compression.FileSystem
function Install-Bundle($Path) {{
    $zip = [IO.Compression.ZipFile]::OpenRead($Path)
    try {{
        $reader = New-Object IO.StreamReader($zip.GetEntry('AppxMetadata/AppxBundleManifest.xml').Open())
        $identity = ([xml]$reader.ReadToEnd()).Bundle.Identity
    }} finally {{
        $zip.Dispose()
    }}
    if ((Get-AppxPackage -Name $identity.Name).Version -ne $identity.Version) {{
        Add-AppxPackage -Path $Path -AllowUnsigned -ForceApplicationShutdown -ForceUpdateFromAnyVersion
    }}
}}
",
            powershell_quote(&log)
        );
        for package in packages {
            contents.push_str(&format!("Install-Bundle {}\n", powershell_quote(package)));
        }
        fs::write(&script, contents.replace('\n', "\r\n"))?;

        println!("Installing MSIX packages, accept the UAC prompt to continue...");
        let elevate = format!(
            "try {{ $process = Start-Process powershell.exe -Verb RunAs -Wait -PassThru \
             -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','\"{}\"' }} \
             catch {{ exit {ERROR_CANCELLED} }}; exit $process.ExitCode",
            script.display().to_string().replace('\'', "''")
        );
        let status = Command::new("powershell.exe")
            .args(["-NoProfile", "-Command", &elevate])
            .status()
            .context("failed to start PowerShell")?;
        match status.code() {
            Some(0) => Ok(()),
            Some(ERROR_CANCELLED) => {
                bail!("failed to install MSIX packages: the UAC prompt was declined")
            }
            code => {
                let error = fs::read_to_string(&log)
                    .unwrap_or_else(|_| format!("elevated PowerShell exited with {code:?}"));
                bail!("failed to install MSIX packages: {}", error.trim())
            }
        }
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
            let (name, identifier) = identity.unwrap_or_else(|| {
                let identifier = package_name.to_owned();
                (default_name, identifier)
            });
            apps.push(InstallableApp {
                package: package_name.to_owned(),
                directory: package_directory.to_path_buf(),
                identifier,
                name,
                ios: bundle.is_some_and(|bundle| bundle.get("ios").is_some()),
                android: bundle.is_some_and(|bundle| bundle.get("android").is_some()),
            });
        }
        Ok(apps)
    }
}
