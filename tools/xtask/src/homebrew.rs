/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Bundles of the Wii and Wii U homebrew programs, for the Homebrew Channel and the Wii U Menu.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs, io};

use anyhow::{Context, Result, bail};

use crate::process::run;
use crate::utils::collect_source_files;
use crate::{Os, Xtask};

/// A console with homebrew programs that have a `meta/` folder.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Console {
    /// Homebrew Channel bundles: a folder with `boot.dol`, `meta.xml` and `icon.png`.
    Wii,
    /// Wii U Menu bundles for Aroma: a `.wuhb` file made from `meta.ini` and its images.
    WiiU,
}

impl Console {
    const fn name(self) -> &'static str {
        match self {
            Self::Wii => "Wii",
            Self::WiiU => "Wii U",
        }
    }

    /// The name of the Rust target, the toolchain and the bundle folder.
    const fn target(self) -> &'static str {
        match self {
            Self::Wii => "wii",
            Self::WiiU => "wiiu",
        }
    }

    /// The metadata file in `meta/` that marks a program.
    const fn meta_file(self) -> &'static str {
        match self {
            Self::Wii => "meta.xml",
            Self::WiiU => "meta.ini",
        }
    }

    /// The folder on the SD card the bundles go to.
    const fn apps(self) -> &'static str {
        match self {
            Self::Wii => "apps",
            Self::WiiU => "wiiu/apps",
        }
    }

    const fn sd_card_variable(self) -> &'static str {
        match self {
            Self::Wii => "WII_SD_CARD",
            Self::WiiU => "WIIU_SD_CARD",
        }
    }
}

/// A homebrew program with a `meta/` folder.
struct Program {
    directory: PathBuf,
    /// The name of the built program.
    name: String,
    /// The bundle name on the SD card.
    bundle: String,
    rust: bool,
}

fn discover_programs(root: &Path, console: Console) -> Result<Vec<Program>> {
    let mut files = Vec::new();
    collect_source_files(root, &mut files)?;
    let mut programs = Vec::new();
    for file in files {
        if !file.ends_with(Path::new("meta").join(console.meta_file())) {
            continue;
        }
        let directory = file
            .parent()
            .and_then(Path::parent)
            .context("invalid metadata path")?;
        let leaf = directory
            .file_name()
            .and_then(|name| name.to_str())
            .context("invalid program directory")?;
        let rust = directory.join("Cargo.toml").is_file();
        if !rust && !directory.join("Makefile").is_file() {
            continue;
        }
        let (name, bundle) = if matches!(leaf, "c" | "rust") {
            let name = directory
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .context("invalid example directory")?;
            (name.to_owned(), format!("{name}-{leaf}"))
        } else {
            let suffix = format!("-{}", console.target());
            (
                leaf.to_owned(),
                leaf.strip_suffix(suffix.as_str())
                    .unwrap_or(leaf)
                    .to_owned(),
            )
        };
        programs.push(Program {
            directory: directory.to_owned(),
            name,
            bundle,
            rust,
        });
    }
    programs.sort_by(|left, right| left.bundle.cmp(&right.bundle));
    Ok(programs)
}

impl Xtask {
    pub(crate) fn build_homebrew_bundle(
        &self,
        console: Console,
        selected: Option<&str>,
    ) -> Result<()> {
        for bundle in self.homebrew_bundles(console, selected)? {
            println!("Built {}", bundle.display());
        }
        Ok(())
    }

    pub(crate) fn install_homebrew(&self, console: Console, selected: Option<&str>) -> Result<()> {
        let apps = self.sd_card(console)?.join(console.apps());
        for bundle in self.homebrew_bundles(console, selected)? {
            let destination = apps.join(bundle.file_name().context("bundle has a name")?);
            let files = if bundle.is_dir() {
                fs::create_dir_all(&destination)?;
                fs::read_dir(&bundle)?
                    .map(|entry| {
                        let source = entry?.path();
                        let name = source.file_name().context("file has a name")?;
                        Ok((destination.join(name), source))
                    })
                    .collect::<Result<Vec<_>>>()?
            } else {
                fs::create_dir_all(&apps)?;
                vec![(destination.clone(), bundle)]
            };
            for (destination, source) in files {
                // Streaming the bytes skips the extended attributes, which macOS stores in `._`
                // files on FAT SD cards
                io::copy(
                    &mut fs::File::open(source)?,
                    &mut fs::File::create(destination)?,
                )?;
            }
            println!("Installed {}", destination.display());
        }
        Ok(())
    }

    /// Builds the selected program or all of them into `target/bundle/<target>/<apps>/`.
    fn homebrew_bundles(&self, console: Console, selected: Option<&str>) -> Result<Vec<PathBuf>> {
        let available = discover_programs(&self.root, console)?;
        let programs: Vec<_> = available
            .iter()
            .filter(|program| selected.is_none_or(|selected| program.bundle == selected))
            .collect();
        if let Some(selected) = selected
            && programs.is_empty()
        {
            let names: Vec<_> = available
                .iter()
                .map(|program| program.bundle.as_str())
                .collect();
            bail!(
                "no {} program {selected}, choose from: {}",
                console.name(),
                names.join(", ")
            );
        }
        let toolchain = self
            .root
            .join(format!("target/{}-toolchain", console.target()));
        let apps = self
            .root
            .join("target/bundle")
            .join(console.target())
            .join(console.apps().rsplit('/').next().unwrap_or_default());
        let mut bundles = Vec::new();
        for program in programs {
            let directory = self.root.join(&program.directory);
            let built = directory.join(if program.rust {
                format!("target/{}/release/{}", console.target(), program.name)
            } else {
                format!("target/{}", program.name)
            });
            if program.rust {
                // The programs select their nightly toolchain themselves
                run(Command::new("cargo")
                    .args(["build", "--release", "--locked"])
                    .current_dir(&directory)
                    .env_remove("RUSTUP_TOOLCHAIN"))?;
            } else {
                let mut make = Command::new("make");
                make.arg("-C").arg(&directory);
                if console == Console::WiiU {
                    make.arg("bundle");
                }
                run(&mut make)?;
            }
            let bundle = match console {
                Console::Wii => {
                    let bundle = apps.join(&program.bundle);
                    fs::create_dir_all(&bundle)?;
                    let dol = bundle.join("boot.dol");
                    if program.rust {
                        run(Command::new(toolchain.join("elf2dol"))
                            .arg(built.with_extension("elf"))
                            .arg(&dol))?;
                    } else {
                        fs::copy(built.with_extension("dol"), &dol)?;
                    }
                    for file in ["meta.xml", "icon.png"] {
                        fs::copy(directory.join("meta").join(file), bundle.join(file))?;
                    }
                    bundle
                }
                Console::WiiU => {
                    fs::create_dir_all(&apps)?;
                    let bundle = apps.join(format!("{}.wuhb", program.bundle));
                    if program.rust {
                        let rpx = built.with_extension("rpx");
                        run(Command::new(toolchain.join("elf2rpl"))
                            .arg(built.with_extension("elf"))
                            .arg(&rpx))?;
                        run(
                            Command::new(self.root.join("tools/wiiu-toolchain/bundle.sh"))
                                .arg(rpx)
                                .arg(directory.join("meta"))
                                .arg(&bundle),
                        )?;
                    } else {
                        fs::copy(built.with_extension("wuhb"), &bundle)?;
                    }
                    bundle
                }
            };
            bundles.push(bundle);
        }
        Ok(bundles)
    }

    /// The mounted volume with the console's homebrew folder, or the SD card variable when it's
    /// set.
    fn sd_card(&self, console: Console) -> Result<PathBuf> {
        let variable = console.sd_card_variable();
        if let Some(path) = env::var_os(variable) {
            return Ok(PathBuf::from(path));
        }
        let user = env::var("USER").unwrap_or_default();
        let mounts: Vec<PathBuf> = match self.os {
            Os::Macos => vec!["/Volumes".into()],
            Os::Linux => vec![
                Path::new("/media").join(&user),
                Path::new("/run/media").join(&user),
            ],
            Os::Windows => bail!("set {variable} to the SD card drive, like E:\\"),
        };
        let marker = console.apps().split('/').next().unwrap_or_default();
        let cards: Vec<PathBuf> = mounts
            .iter()
            .filter_map(|mounts| fs::read_dir(mounts).ok())
            .flatten()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|volume| volume.join(marker).is_dir())
            .collect();
        match cards.as_slice() {
            [card] => Ok(card.clone()),
            [] => bail!("no SD card with a {marker} folder found, set {variable} to its path"),
            _ => bail!(
                "several SD cards with a {marker} folder found, set {variable} to one of: {}",
                cards
                    .iter()
                    .map(|card| card.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovers_wii_examples_with_homebrew_metadata() -> Result<()> {
        let root = env::temp_dir().join(format!("xtask-wii-discovery-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        for directory in [
            "examples/wii/new-example/c/meta",
            "examples/wii/new-example/rust/meta",
            "examples/wii-extra/another/c/meta",
            "examples/wii/target/ignored/c/meta",
            "games/plaatcraft-wii/meta",
            "apps/homebrew/rust/meta",
        ] {
            let directory = root.join(directory);
            fs::create_dir_all(&directory)?;
            fs::write(directory.join("meta.xml"), "")?;
            let program = directory.parent().unwrap();
            let manifest = if program.ends_with("rust") || program.ends_with("plaatcraft-wii") {
                "Cargo.toml"
            } else {
                "Makefile"
            };
            fs::write(program.join(manifest), "")?;
        }
        fs::create_dir_all(root.join("examples/wii/no-metadata/c"))?;
        fs::create_dir_all(root.join("unrelated/meta"))?;
        fs::write(root.join("unrelated/meta/meta.xml"), "")?;
        let programs = discover_programs(&root, Console::Wii)?;
        fs::remove_dir_all(&root)?;
        assert_eq!(
            programs
                .iter()
                .map(|program| program.bundle.as_str())
                .collect::<Vec<_>>(),
            [
                "another-c",
                "homebrew-rust",
                "new-example-c",
                "new-example-rust",
                "plaatcraft"
            ]
        );
        assert_eq!(programs[2].name, "new-example");
        assert!(!programs[2].rust);
        assert!(programs[3].rust);
        assert_eq!(programs[4].name, "plaatcraft-wii");
        Ok(())
    }

    #[test]
    fn discovers_wii_u_programs_by_their_menu_metadata() -> Result<()> {
        let root = env::temp_dir().join(format!("xtask-wiiu-discovery-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        for (directory, meta, manifest) in [
            ("examples/wiiu/cube/c", "meta.ini", "Makefile"),
            ("examples/wiiu/cube/rust", "meta.ini", "Cargo.toml"),
            ("games/plaatcraft-wiiu", "meta.ini", "Cargo.toml"),
            ("examples/wii/cube/c", "meta.xml", "Makefile"),
        ] {
            let directory = root.join(directory);
            fs::create_dir_all(directory.join("meta"))?;
            fs::write(directory.join("meta").join(meta), "")?;
            fs::write(directory.join(manifest), "")?;
        }
        let programs = discover_programs(&root, Console::WiiU)?;
        fs::remove_dir_all(&root)?;
        assert_eq!(
            programs
                .iter()
                .map(|program| (program.bundle.as_str(), program.name.as_str(), program.rust))
                .collect::<Vec<_>>(),
            [
                ("cube-c", "cube", false),
                ("cube-rust", "cube", true),
                ("plaatcraft", "plaatcraft-wiiu", true)
            ]
        );
        Ok(())
    }
}
