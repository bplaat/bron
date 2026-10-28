/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use directories::BaseDirs;

use crate::metadata::InstallableApp;
use crate::process::run;
use crate::utils::remove_path;

/// The package format of the running Linux distribution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageFormat {
    Deb,
    Rpm,
}

impl PackageFormat {
    pub(crate) fn detect() -> Result<Self> {
        let os_release = fs::read_to_string("/etc/os-release")
            .or_else(|_| fs::read_to_string("/usr/lib/os-release"))
            .context("failed to read os-release to detect the Linux distribution")?;
        Self::from_os_release(&os_release)
    }

    fn from_os_release(os_release: &str) -> Result<Self> {
        let value = |key: &str| {
            os_release
                .lines()
                .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
                .map(|value| value.trim().trim_matches(['"', '\'']))
                .unwrap_or_default()
        };
        let id = value("ID");
        for distribution in std::iter::once(id).chain(value("ID_LIKE").split_whitespace()) {
            match distribution {
                "debian" | "ubuntu" => return Ok(Self::Deb),
                "fedora" | "rhel" | "centos" => return Ok(Self::Rpm),
                _ => {}
            }
        }
        bail!(
            "unsupported Linux distribution: {id}, applications can only be installed on Debian \
             and Fedora based distributions"
        )
    }

    pub(crate) const fn extension(self) -> &'static str {
        match self {
            Self::Deb => "deb",
            Self::Rpm => "rpm",
        }
    }

    /// Install the packages with their dependencies using the package manager, all from one
    /// elevated shell to only show one sudo prompt.
    pub(crate) fn install(self, packages: &[PathBuf]) -> Result<()> {
        let script = match self {
            // Reinstalls replace a package of the same version with the new build
            Self::Deb => package_command("apt-get install --reinstall -y", packages),
            // dnf can only reinstall the installed version and ignores installs of it
            Self::Rpm => {
                let (installed, other): (Vec<_>, Vec<_>) =
                    packages.iter().cloned().partition(|package| {
                        package.file_stem().is_some_and(|nevra| {
                            Command::new("rpm")
                                .args(["-q", "--quiet"])
                                .arg(nevra)
                                .status()
                                .is_ok_and(|status| status.success())
                        })
                    });
                [("dnf install -y", other), ("dnf reinstall -y", installed)]
                    .into_iter()
                    .filter(|(_, packages)| !packages.is_empty())
                    .map(|(command, packages)| package_command(command, &packages))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        println!(
            "Installing {} packages, accept the sudo prompt to continue...",
            self.extension()
        );
        run(Command::new("sudo").args(["sh", "-ec", &script]))
    }
}

fn package_command(command: &str, packages: &[PathBuf]) -> String {
    packages
        .iter()
        .fold(command.to_owned(), |command, package| {
            let package = package.display().to_string().replace('\'', r"'\''");
            format!("{command} '{package}'")
        })
}

/// Remove the files older versions of this task installed in the home directory, because
/// their desktop entries hide the entries of the packages.
pub(crate) fn remove_home_install(apps: &[InstallableApp]) -> Result<()> {
    let base_dirs = BaseDirs::new().context("home directory is not available")?;
    let data = base_dirs.data_dir();
    let mut removed = false;
    for app in apps {
        let identifier = &app.identifier;
        let mut paths = vec![
            data.join(format!("applications/{identifier}.desktop")),
            data.join(format!("thumbnailers/{identifier}.thumbnailer")),
            data.join(format!("mime/packages/{identifier}.xml")),
        ];
        if let Some(bin) = base_dirs.executable_dir() {
            paths.push(bin.join(app.name.replace(' ', "")));
        }
        for size in [16, 24, 32, 48, 64, 128, 256, 512] {
            paths.push(data.join(format!("icons/hicolor/{size}x{size}/apps/{identifier}.png")));
        }
        for path in paths.into_iter().filter(|path| path.exists()) {
            println!("Removing {}", path.display());
            remove_path(&path)?;
            removed = true;
        }
    }
    if !removed {
        return Ok(());
    }
    for (program, arguments, directory) in [
        ("update-mime-database", &[][..], "mime"),
        ("update-desktop-database", &[], "applications"),
        (
            "gtk-update-icon-cache",
            &["--force", "--ignore-theme-index"],
            "icons/hicolor",
        ),
    ] {
        // The caches only speed up lookups, so a missing tool is not an error
        let directory = data.join(directory);
        if directory.is_dir()
            && let Err(error) = run(Command::new(program).args(arguments).arg(directory))
        {
            eprintln!("Warning: {error:#}");
        }
    }
    Ok(())
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_debian_and_fedora_based_distributions() -> Result<()> {
        for (os_release, format) in [
            ("ID=debian\nVERSION_ID=\"13\"\n", PackageFormat::Deb),
            ("ID=ubuntu\nID_LIKE=debian\n", PackageFormat::Deb),
            (
                "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n",
                PackageFormat::Deb,
            ),
            ("ID=fedora\nVERSION_ID=43\n", PackageFormat::Rpm),
            (
                "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n",
                PackageFormat::Rpm,
            ),
        ] {
            assert_eq!(PackageFormat::from_os_release(os_release)?, format);
        }
        Ok(())
    }

    #[test]
    fn package_commands_quote_paths() {
        assert_eq!(
            package_command(
                "dnf install -y",
                &[PathBuf::from("/a b/x.rpm"), PathBuf::from("/it's/y.rpm")]
            ),
            r"dnf install -y '/a b/x.rpm' '/it'\''s/y.rpm'"
        );
    }

    #[test]
    fn rejects_other_distributions() {
        for os_release in [
            "ID=arch\n",
            "ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n",
            "",
        ] {
            assert!(PackageFormat::from_os_release(os_release).is_err());
        }
    }
}
