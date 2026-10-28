/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::Path;
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use crate::args::Args;
use crate::manifest::Manifest;
use crate::support::cargo_build;

mod deb;
mod elf;
mod freedesktop;
mod rpm;

/// The Debian and RPM names of an architecture.
pub(crate) struct Arch {
    pub deb: &'static str,
    pub rpm: &'static str,
}

/// A file installed by a package at an absolute path.
pub(crate) struct File {
    pub path: String,
    pub mode: u32,
    pub data: Vec<u8>,
}

/// The distribution independent contents of a package.
pub(crate) struct Package {
    pub name: String,
    pub version: String,
    pub arch: Arch,
    pub summary: String,
    pub maintainer: String,
    pub license: String,
    pub url: Option<String>,
    pub mtime: u32,
    /// Sorted by path
    pub files: Vec<File>,
    /// The directory with the bundle assets, owned by the package
    pub data_dir: Option<String>,
}

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    exit(1);
}

/// Build a .deb and .rpm package, or only one with --deb or --rpm.
pub(crate) fn bundle(args: &Args, target_dir: &str, manifest: &Manifest) {
    if args.zip || args.dmg || args.run {
        fail("--zip, --dmg and --run are not supported for Linux packages");
    }
    let status = cargo_build(&args.path)
        .status()
        .expect("Failed to run cargo build");
    assert!(status.success(), "cargo build failed");

    let binary = format!("target/release/{}", manifest.package.name);
    let executable =
        fs::read(&binary).unwrap_or_else(|error| panic!("Failed to read {binary}: {error}"));
    let elf = elf::parse(&executable).unwrap_or_else(|| fail("The executable is not an ELF file"));
    let package = package(args, manifest, &elf, Path::new(&binary), executable);

    fs::create_dir_all(target_dir).expect("Failed to create target directory");
    let (deb, rpm) = (args.deb || !args.rpm, args.rpm || !args.deb);
    // Remove the packages of older versions, so only the new ones are found
    for entry in fs::read_dir(target_dir).expect("Failed to read target directory") {
        let path = entry.expect("Failed to read target directory entry").path();
        if path
            .extension()
            .is_some_and(|extension| extension == "deb" || extension == "rpm")
        {
            fs::remove_file(&path)
                .unwrap_or_else(|error| panic!("Failed to remove {}: {error}", path.display()));
        }
    }
    if deb {
        let path = format!(
            "{target_dir}/{}_{}_{}.deb",
            package.name, package.version, package.arch.deb
        );
        fs::write(&path, deb::package(&package, &deb::dependencies(&elf)))
            .expect("Failed to write .deb package");
        println!("Created {path}");
    }
    if rpm {
        let path = format!("{target_dir}/{}", rpm::file_name(&package));
        fs::write(&path, rpm::package(&package, &rpm::dependencies(&elf)))
            .expect("Failed to write .rpm package");
        println!("Created {path}");
    }
}

fn package(
    args: &Args,
    manifest: &Manifest,
    elf: &elf::Elf,
    binary: &Path,
    executable: Vec<u8>,
) -> Package {
    let package = &manifest.package;
    let bundle = &package.metadata.bundle;
    let linux = &bundle.linux;
    let name = package.name.replace('_', "-").to_lowercase();
    let executable_path = format!("/usr/bin/{}", package.name);
    let summary = package
        .description
        .as_deref()
        .unwrap_or(&bundle.name)
        .lines()
        .collect::<Vec<_>>()
        .join(" ");
    let read = |path: &str| {
        let path = format!("{}/{path}", args.path);
        fs::read(&path).unwrap_or_else(|error| panic!("Failed to read {path}: {error}"))
    };

    let mut files = Vec::new();
    let mut add = |path: String, mode, data| files.push(File { path, mode, data });
    let data_dir = format!("/usr/share/{}", package.name);
    let assets = crate::assets::collect_shared(&[binary.to_path_buf()])
        .expect("Failed to collect bundle assets");
    let has_assets = !assets.is_empty();
    for (asset, data) in assets {
        add(format!("{data_dir}/{asset}"), 0o644, data);
    }
    add(executable_path.clone(), 0o755, executable);

    let identifier = &bundle.identifier;
    let mut has_icon = false;
    if let Some(icons) = &linux.icons {
        let directory = format!("{}/{icons}", args.path);
        let mut names: Vec<_> = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("Failed to read {directory}: {error}"))
            .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
            .collect();
        names.sort();
        for icon in names {
            let size = if icon == "scalable.svg" {
                "scalable"
            } else if let Some(size) = icon.strip_suffix(".png")
                && size
                    .split_once('x')
                    .is_some_and(|(width, height)| width == height && width.parse::<u32>().is_ok())
            {
                size
            } else {
                continue;
            };
            let extension = if size == "scalable" { "svg" } else { "png" };
            add(
                format!("/usr/share/icons/hicolor/{size}/apps/{identifier}.{extension}"),
                0o644,
                read(&format!("{icons}/{icon}")),
            );
            has_icon = true;
        }
        if !has_icon {
            fail(&format!(
                "{directory} has no <size>x<size>.png or scalable.svg icons"
            ));
        }
    }

    let custom = linux.desktop_entry.as_deref().map(|path| {
        String::from_utf8(read(path)).unwrap_or_else(|_| fail(&format!("{path} is not UTF-8")))
    });
    add(
        format!("/usr/share/applications/{identifier}.desktop"),
        0o644,
        freedesktop::desktop_entry(package, &executable_path, has_icon, custom.as_deref())
            .into_bytes(),
    );
    add(
        format!("/usr/share/metainfo/{identifier}.metainfo.xml"),
        0o644,
        freedesktop::metainfo(package, &summary).into_bytes(),
    );
    if let Some(mime_info) = &linux.mime_info {
        add(
            format!("/usr/share/mime/packages/{identifier}.xml"),
            0o644,
            read(mime_info),
        );
    }
    if let Some(arguments) = &linux.thumbnailer {
        add(
            format!("/usr/share/thumbnailers/{identifier}.thumbnailer"),
            0o644,
            freedesktop::thumbnailer(package, &executable_path, arguments).into_bytes(),
        );
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Package {
        name,
        version: package.version.replace('-', "~"),
        arch: arch(elf.machine, elf.is_64bit),
        summary,
        maintainer: package
            .authors
            .first()
            .cloned()
            .unwrap_or_else(|| fail("Package needs authors for the package maintainer")),
        license: package
            .license
            .clone()
            .unwrap_or_else(|| "Unknown".to_owned()),
        url: package
            .homepage
            .clone()
            .or_else(|| package.repository.clone()),
        mtime: mtime(),
        files,
        data_dir: has_assets.then_some(data_dir),
    }
}

fn arch(machine: u16, is_64bit: bool) -> Arch {
    let (deb, rpm) = match (machine, is_64bit) {
        (elf::EM_X86_64, true) => ("amd64", "x86_64"),
        (elf::EM_AARCH64, true) => ("arm64", "aarch64"),
        (elf::EM_RISCV, true) => ("riscv64", "riscv64"),
        (elf::EM_386, false) => ("i386", "i686"),
        (elf::EM_ARM, false) => ("armhf", "armv7hl"),
        _ => fail(&format!("Unsupported ELF machine: {machine}")),
    };
    Arch { deb, rpm }
}

/// The file and build time, `SOURCE_DATE_EPOCH` makes packages reproducible.
fn mtime() -> u32 {
    env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|epoch| epoch.parse().ok())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("System clock predates Unix epoch")
                .as_secs() as u32
        })
}

// MARK: Tests
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn package() -> Package {
        Package {
            name: "example".to_owned(),
            version: "1.0.0~beta".to_owned(),
            arch: arch(elf::EM_X86_64, true),
            summary: "An example".to_owned(),
            maintainer: "Example Author <author@example.com>".to_owned(),
            license: "MIT".to_owned(),
            url: Some("https://example.com".to_owned()),
            mtime: 1_700_000_000,
            files: vec![
                File {
                    path: "/usr/bin/example".to_owned(),
                    mode: 0o755,
                    data: b"\x7fELF".to_vec(),
                },
                File {
                    path: format!(
                        "/usr/share/example/{}/{}/file",
                        "a".repeat(60),
                        "b".repeat(60)
                    ),
                    mode: 0o644,
                    data: b"asset".to_vec(),
                },
            ],
            data_dir: Some("/usr/share/example".to_owned()),
        }
    }

    #[test]
    fn maps_elf_machines_to_architectures() {
        let arch = arch(elf::EM_AARCH64, true);
        assert_eq!((arch.deb, arch.rpm), ("arm64", "aarch64"));
    }
}
