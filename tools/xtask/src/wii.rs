/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Homebrew Channel bundles of the Wii programs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use anyhow::{Context, Result, bail};

use crate::process::run;
use crate::{Os, Xtask};

/// A Wii program with a `meta/` folder for the Homebrew Channel.
struct Program {
    directory: &'static str,
    /// The name of the built program.
    name: &'static str,
    /// The folder in `apps/` on the SD card.
    bundle: &'static str,
    rust: bool,
}

const PROGRAMS: [Program; 7] = [
    Program {
        directory: "examples/wii/triangles/c",
        name: "triangles",
        bundle: "triangles-c",
        rust: false,
    },
    Program {
        directory: "examples/wii/triangles/rust",
        name: "triangles",
        bundle: "triangles-rust",
        rust: true,
    },
    Program {
        directory: "examples/wii/canvas/c",
        name: "canvas",
        bundle: "canvas-c",
        rust: false,
    },
    Program {
        directory: "examples/wii/canvas/rust",
        name: "canvas",
        bundle: "canvas-rust",
        rust: true,
    },
    Program {
        directory: "examples/wii/blocks/c",
        name: "blocks",
        bundle: "blocks-c",
        rust: false,
    },
    Program {
        directory: "examples/wii/blocks/rust",
        name: "blocks",
        bundle: "blocks-rust",
        rust: true,
    },
    Program {
        directory: "games/plaatcraft-wii",
        name: "plaatcraft-wii",
        bundle: "plaatcraft",
        rust: true,
    },
];

impl Xtask {
    pub(crate) fn build_wii_bundle(&self, selected: Option<&str>) -> Result<()> {
        for bundle in self.wii_bundles(selected)? {
            println!("Built {}", bundle.display());
        }
        Ok(())
    }

    pub(crate) fn install_wii(&self, selected: Option<&str>) -> Result<()> {
        let apps = self.wii_sd_card()?.join("apps");
        for bundle in self.wii_bundles(selected)? {
            let destination = apps.join(bundle.file_name().context("bundle has a name")?);
            fs::create_dir_all(&destination)?;
            for entry in fs::read_dir(&bundle)? {
                let source = entry?.path();
                // Writing the bytes skips the extended attributes, which macOS stores in
                // `._` files on FAT SD cards
                fs::write(
                    destination.join(source.file_name().context("file has a name")?),
                    fs::read(&source)?,
                )?;
            }
            println!("Installed {}", destination.display());
        }
        Ok(())
    }

    /// Builds the selected program or all of them into `target/bundle/wii/apps/<bundle>/`.
    fn wii_bundles(&self, selected: Option<&str>) -> Result<Vec<PathBuf>> {
        let programs: Vec<_> = PROGRAMS
            .iter()
            .filter(|program| selected.is_none_or(|selected| program.bundle == selected))
            .collect();
        if let Some(selected) = selected
            && programs.is_empty()
        {
            let names: Vec<_> = PROGRAMS.iter().map(|program| program.bundle).collect();
            bail!(
                "no Wii program {selected}, choose from: {}",
                names.join(", ")
            );
        }
        let elf2dol = self.root.join("target/wii-toolchain/elf2dol");
        let mut bundles = Vec::new();
        for program in programs {
            let directory = self.root.join(program.directory);
            let bundle = self
                .root
                .join("target/bundle/wii/apps")
                .join(program.bundle);
            fs::create_dir_all(&bundle)?;
            let dol = bundle.join("boot.dol");
            if program.rust {
                // The Wii programs select their nightly toolchain themselves
                run(Command::new("cargo")
                    .args(["build", "--release", "--locked"])
                    .current_dir(&directory)
                    .env_remove("RUSTUP_TOOLCHAIN"))?;
                let elf = directory.join(format!("target/wii/release/{}.elf", program.name));
                run(Command::new(&elf2dol).arg(elf).arg(&dol))?;
            } else {
                run(Command::new("make").arg("-C").arg(&directory))?;
                fs::copy(directory.join(format!("target/{}.dol", program.name)), &dol)?;
            }
            for file in ["meta.xml", "icon.png"] {
                fs::copy(directory.join("meta").join(file), bundle.join(file))?;
            }
            bundles.push(bundle);
        }
        Ok(bundles)
    }

    /// The mounted volume with an `apps/` folder, or `WII_SD_CARD` when it's set.
    fn wii_sd_card(&self) -> Result<PathBuf> {
        if let Some(path) = env::var_os("WII_SD_CARD") {
            return Ok(PathBuf::from(path));
        }
        let user = env::var("USER").unwrap_or_default();
        let mounts: Vec<PathBuf> = match self.os {
            Os::Macos => vec!["/Volumes".into()],
            Os::Linux => vec![
                Path::new("/media").join(&user),
                Path::new("/run/media").join(&user),
            ],
            Os::Windows => bail!("set WII_SD_CARD to the SD card drive, like E:\\"),
        };
        let cards: Vec<PathBuf> = mounts
            .iter()
            .filter_map(|mounts| fs::read_dir(mounts).ok())
            .flatten()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|volume| volume.join("apps").is_dir())
            .collect();
        match cards.as_slice() {
            [card] => Ok(card.clone()),
            [] => bail!("no SD card with an apps folder found, set WII_SD_CARD to its path"),
            _ => bail!(
                "several SD cards with an apps folder found, set WII_SD_CARD to one of: {}",
                cards
                    .iter()
                    .map(|card| card.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}
