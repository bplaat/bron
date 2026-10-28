/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::Path;
use std::process::exit;

use crate::args::Args;
use crate::manifest::Manifest;
use crate::support::{assert_file_name, cargo_build, remove_path_if_exists};
use crate::windows::appx_manifest::{appx_manifest, package_version, shell_extension_library};
use crate::windows::publisher::publisher_name;

mod appx_manifest;
mod assets;
mod publisher;
mod sdk;

// Logos referenced by the manifest, qualified variants like `.targetsize-88_altform-unplated.png`
// next to them are indexed by `resources.pri`
const LOGOS: [&str; 3] = ["Square44x44Logo", "Square150x150Logo", "StoreLogo"];

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
    let shell_package = windows
        .shell_extension
        .as_ref()
        .map(|shell| format!("{}/{}", args.path, shell.package));
    for (target, _) in ARCHITECTURES {
        for path in std::iter::once(&args.path).chain(&shell_package) {
            let status = cargo_build(path)
                .args(["--target", target])
                .status()
                .expect("Failed to run cargo build");
            assert!(status.success(), "cargo build failed for {path} ({target})");
        }
    }

    let publisher = publisher_name(manifest);
    let assets = windows.assets.as_deref().unwrap_or("meta/windows");
    let logos = logo_files(&format!("{}/{assets}", args.path));
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
        if let Some(shell) = &windows.shell_extension {
            let library = shell_extension_library(shell);
            fs::copy(
                format!("target/{target}/release/{library}"),
                format!("{layout}/{library}"),
            )
            .unwrap_or_else(|error| panic!("Failed to copy {library}: {error}"));
        }
        for logo in &logos {
            fs::copy(
                format!("{}/{assets}/{logo}", args.path),
                format!("{layout}/Assets/{logo}"),
            )
            .unwrap_or_else(|error| panic!("Failed to copy {assets}/{logo}: {error}"));
        }
        fs::write(
            format!("{layout}/AppxManifest.xml"),
            appx_manifest(manifest, &windows, &publisher, &executable, arch, 0),
        )
        .expect("Failed to write AppxManifest.xml");
    }

    let shared_layout = Path::new(&msix_dir).join("shared");
    let binaries: Vec<_> = ARCHITECTURES
        .iter()
        .map(|(_, arch)| Path::new(&msix_dir).join(arch).join(&executable))
        .collect();
    let shared_files =
        assets::stage(&binaries, &shared_layout).expect("Failed to stage shared MSIX assets");

    if !shared_files.is_empty() {
        fs::write(
            shared_layout.join("AppxManifest.xml"),
            appx_manifest::asset_manifest_template(manifest, &windows, &publisher, &executable, 0),
        )
        .expect("Failed to write shared asset manifest template");
    }

    // Windows ignores reinstalls of the same version, so the revision hashes all packaged files,
    // the resource index is created afterwards from these same files
    let mut hash = Fnv1a::default();
    for (_, arch) in ARCHITECTURES {
        hash_directory(&mut hash, Path::new(&format!("{msix_dir}/{arch}")));
    }
    if !shared_files.is_empty() {
        hash_directory(&mut hash, &shared_layout);
    }
    let revision = hash.revision();
    for (_, arch) in ARCHITECTURES {
        let layout = format!("{msix_dir}/{arch}");
        fs::write(
            format!("{layout}/AppxManifest.xml"),
            appx_manifest(manifest, &windows, &publisher, &executable, arch, revision),
        )
        .expect("Failed to write AppxManifest.xml");
        sdk::make_pri(&format!("{msix_dir}/priconfig.xml"), &layout);
        sdk::pack(&layout, &format!("{packages}/{}_{arch}.msix", bundle.name));
    }

    if !shared_files.is_empty() {
        let template = shared_layout.join("AppxManifest.xml");
        let mapping = shared_layout.join("mapping.txt");
        fs::write(
            &template,
            appx_manifest::asset_manifest_template(
                manifest,
                &windows,
                &publisher,
                &executable,
                revision,
            ),
        )
        .expect("Failed to write shared asset manifest template");
        fs::write(
            &mapping,
            assets::mapping(&shared_layout, &shared_files)
                .expect("Failed to map shared MSIX assets"),
        )
        .expect("Failed to write shared asset mapping");
        sdk::pack_assets(
            &template,
            &mapping,
            &format!("{packages}/{}_assets.msix", bundle.name),
        );
    }

    let msix_bundle = format!("{target_dir}/{}.msixbundle", bundle.name);
    remove_path_if_exists(&msix_bundle);
    // Remove the single architecture package of older versions
    remove_path_if_exists(&format!("{target_dir}/{}.msix", bundle.name));
    sdk::bundle(
        &packages,
        &msix_bundle,
        &package_version(&manifest.package.version, revision),
    );
    println!("Created {msix_bundle}");
}

fn logo_files(assets: &str) -> Vec<String> {
    let mut logos: Vec<_> = fs::read_dir(assets)
        .unwrap_or_else(|error| panic!("Failed to read {assets}: {error}"))
        .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
        .filter(|name| is_logo_file(name))
        .collect();
    logos.sort();
    for logo in LOGOS {
        assert!(
            logos.contains(&format!("{logo}.png")),
            "Missing {assets}/{logo}.png"
        );
    }
    logos
}

fn is_logo_file(name: &str) -> bool {
    LOGOS.iter().any(|logo| {
        name.strip_prefix(logo)
            .is_some_and(|rest| rest.starts_with('.') && rest.ends_with(".png"))
    })
}

fn hash_directory(hash: &mut Fnv1a, directory: &Path) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .expect("Failed to read MSIX layout")
        .map(|entry| entry.expect("Failed to read MSIX layout entry").path())
        .collect();
    entries.sort();
    for path in entries {
        hash.write(path.file_name().unwrap_or_default().as_encoded_bytes());
        if path.is_dir() {
            hash_directory(hash, &path);
        } else {
            hash.write(&fs::read(&path).expect("Failed to read MSIX layout file"));
        }
    }
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
    fn logo_files_include_qualified_variants() {
        assert!(is_logo_file("Square44x44Logo.png"));
        assert!(is_logo_file(
            "Square44x44Logo.targetsize-88_altform-unplated.png"
        ));
        assert!(is_logo_file("StoreLogo.scale-200.png"));
        assert!(!is_logo_file("icon.ico"));
        assert!(!is_logo_file("Square44x44LogoOld.png"));
        assert!(!is_logo_file("Square44x44Logo.svg"));
    }

    #[test]
    fn revision_changes_with_contents() {
        let mut first = Fnv1a::default();
        first.write(b"first");
        let mut second = Fnv1a::default();
        second.write(b"second");
        assert_ne!(first.revision(), second.revision());
    }
}
