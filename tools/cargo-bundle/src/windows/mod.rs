/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::process::exit;

use crate::args::Args;
use crate::manifest::Manifest;
use crate::support::{assert_file_name, cargo_build, remove_path_if_exists};
use crate::windows::appx_manifest::{appx_manifest, package_version};
use crate::windows::publisher::publisher_name;

mod appx_manifest;
mod makeappx;
mod publisher;

const LOGOS: [&str; 3] = [
    "Square44x44Logo.png",
    "Square150x150Logo.png",
    "StoreLogo.png",
];

// Rust targets and their MSIX architectures, the bundle lets Windows install the matching one
const ARCHITECTURES: [(&str, &str); 2] = [
    ("x86_64-pc-windows-msvc", "x64"),
    ("aarch64-pc-windows-msvc", "arm64"),
];

#[cfg(target_arch = "x86_64")]
const HOST_ARCH: &str = "x64";
#[cfg(target_arch = "x86")]
const HOST_ARCH: &str = "x86";
#[cfg(target_arch = "aarch64")]
const HOST_ARCH: &str = "arm64";

pub(crate) fn bundle(args: &Args, target_dir: &str, manifest: &Manifest) {
    if args.zip || args.dmg {
        eprintln!("--zip and --dmg are only supported on macOS");
        exit(1);
    }

    let bundle = &manifest.package.metadata.bundle;
    assert_file_name(&bundle.name, "bundle name");
    let windows = bundle.windows.clone().unwrap_or_default();
    let executable = format!("{}.exe", manifest.package.name);
    for (target, _) in ARCHITECTURES {
        let status = cargo_build(&args.path)
            .args(["--target", target])
            .status()
            .expect("Failed to run cargo build");
        assert!(status.success(), "cargo build failed for {target}");
    }

    // Windows ignores reinstalls of the same version, so the revision changes with the contents
    let publisher = publisher_name(&args.path, manifest);
    let assets = windows.assets.as_deref().unwrap_or("meta/windows");
    let mut hash = Fnv1a::default();
    for (target, _) in ARCHITECTURES {
        hash.write(
            &fs::read(format!("target/{target}/release/{executable}"))
                .expect("Failed to read executable"),
        );
    }
    for logo in LOGOS {
        hash.write(
            &fs::read(format!("{}/{assets}/{logo}", args.path))
                .unwrap_or_else(|error| panic!("Failed to read {assets}/{logo}: {error}")),
        );
    }
    hash.write(appx_manifest(manifest, &windows, &publisher, &executable, HOST_ARCH, 0).as_bytes());
    let revision = hash.revision();

    let msix_dir = format!("{target_dir}/msix");
    let packages = format!("{msix_dir}/packages");
    remove_path_if_exists(&msix_dir);
    fs::create_dir_all(&packages).expect("Failed to create MSIX packages directory");
    for (target, arch) in ARCHITECTURES {
        let layout = format!("{msix_dir}/{arch}");
        fs::create_dir_all(format!("{layout}/Assets")).expect("Failed to create MSIX layout");
        fs::copy(
            format!("target/{target}/release/{executable}"),
            format!("{layout}/{executable}"),
        )
        .expect("Failed to copy executable");
        for logo in LOGOS {
            fs::copy(
                format!("{}/{assets}/{logo}", args.path),
                format!("{layout}/Assets/{logo}"),
            )
            .expect("Failed to copy logo");
        }
        fs::write(
            format!("{layout}/AppxManifest.xml"),
            appx_manifest(manifest, &windows, &publisher, &executable, arch, revision),
        )
        .expect("Failed to write AppxManifest.xml");

        makeappx::pack(&layout, &format!("{packages}/{}_{arch}.msix", bundle.name));
    }

    let msix_bundle = format!("{target_dir}/{}.msixbundle", bundle.name);
    remove_path_if_exists(&msix_bundle);
    // Remove the single architecture package of older versions
    remove_path_if_exists(&format!("{target_dir}/{}.msix", bundle.name));
    makeappx::bundle(
        &packages,
        &msix_bundle,
        &package_version(&manifest.package.version, revision),
    );
    println!("Created {msix_bundle}");
}

#[derive(Clone, Copy)]
struct Fnv1a(u64);

impl Default for Fnv1a {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv1a {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    const fn revision(self) -> u16 {
        (self.0 ^ (self.0 >> 16) ^ (self.0 >> 32) ^ (self.0 >> 48)) as u16
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_changes_with_contents() {
        let mut first = Fnv1a::default();
        first.write(b"first");
        let mut second = Fnv1a::default();
        second.write(b"second");
        assert_ne!(first.revision(), second.revision());
    }
}
