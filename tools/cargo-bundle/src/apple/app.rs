/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::Path;
use std::process::{Command, exit};

use copy_dir::copy_dir;

use crate::apple::support::{
    bundle_info, codesign, compile_icon, compile_universal, merge_custom_info,
    remove_empty_directory,
};
use crate::manifest::{BundleMetadata, Manifest, Package};
use crate::support::{assert_file_name, cargo_build, remove_path_if_exists};

pub(crate) fn build(path: &str, target_dir: &str, manifest: &Manifest, universal: bool) {
    assert_file_name(&manifest.package.metadata.bundle.name, "bundle name");
    remove_path_if_exists(&format!(
        "{target_dir}/{}.app",
        manifest.package.metadata.bundle.name
    ));
    generate_resources(path, target_dir, manifest);
    let binary = compile_binary(path, target_dir, &manifest.package, universal);
    create(path, target_dir, &manifest.package.metadata.bundle, &binary);
}

fn generate_resources(path: &str, target_dir: &str, manifest: &Manifest) {
    let bundle = &manifest.package.metadata.bundle;
    fs::create_dir_all(target_dir).expect("Failed to create target directory");

    if let Some(iconset) = &bundle.macos.iconset {
        let status = Command::new("iconutil")
            .args([
                "-c",
                "icns",
                &format!("{path}/{iconset}"),
                "-o",
                &format!("{target_dir}/icon.icns"),
            ])
            .status()
            .expect("Failed to create icon.icns");
        assert!(status.success(), "iconutil failed");
    }

    if let Some(icon) = &bundle.icon {
        compile_icon(
            &format!("{path}/{icon}"),
            target_dir,
            "macosx",
            &["mac"],
            bundle.macos.minimal_os_version.as_deref().unwrap_or("11.0"),
        );
    }

    if let Some(icns) = &bundle.macos.icns {
        fs::copy(format!("{path}/{icns}"), format!("{target_dir}/icon.icns"))
            .expect("Failed to copy icon.icns");
    }

    write_info_plist(path, target_dir, manifest);
}

fn write_info_plist(path: &str, target_dir: &str, manifest: &Manifest) {
    let bundle = &manifest.package.metadata.bundle;
    let mut info = bundle_info(
        &bundle.name,
        &bundle.identifier,
        &manifest.package.version,
        "APPL",
    );
    info.insert(
        "LSMinimumSystemVersion".into(),
        bundle
            .macos
            .minimal_os_version
            .as_deref()
            .unwrap_or("11.0")
            .into(),
    );
    if let Some(copyright) = &bundle.copyright {
        info.insert("NSHumanReadableCopyright".into(), copyright.clone().into());
    }
    if has_icon(bundle) {
        info.insert("CFBundleIconFile".into(), "icon.icns".into());
        if bundle.icon.is_some() {
            info.insert("CFBundleIconName".into(), "icon".into());
        }
    }
    info.insert("NSHighResolutionCapable".into(), true.into());

    merge_custom_info(&mut info, path, bundle, bundle.macos.info_plist.as_deref());

    plist::Value::Dictionary(info)
        .to_file_binary(format!("{target_dir}/Info.plist"))
        .expect("Failed to write binary Info.plist");
}

fn compile_binary(path: &str, target_dir: &str, package: &Package, universal: bool) -> String {
    if universal {
        let output = format!("{target_dir}/{}", package.metadata.bundle.name);
        compile_universal(path, &package.name, &output);
        output
    } else {
        let status = cargo_build(path)
            .status()
            .expect("Failed to run cargo build");
        assert!(status.success(), "cargo build failed");
        format!("target/release/{}", package.name)
    }
}

fn create(path: &str, target_dir: &str, bundle: &BundleMetadata, binary: &str) {
    let contents = contents_dir(target_dir, bundle);
    fs::create_dir_all(format!("{contents}/MacOS")).expect("Can't create bundle directory");
    let resources = format!("{contents}/Resources");
    let has_resources = bundle.resources_dir.is_some() || has_icon(bundle);
    if has_resources {
        fs::create_dir_all(&resources).expect("Can't create resources directory");
    } else {
        remove_empty_directory(&resources);
    }

    if let Some(resources_dir) = &bundle.resources_dir {
        copy_dir(format!("{path}/{resources_dir}"), &resources)
            .expect("Failed to copy resources directory");
    }
    if has_icon(bundle) {
        fs::copy(
            format!("{target_dir}/icon.icns"),
            format!("{resources}/icon.icns"),
        )
        .expect("Failed to copy icon.icns");
    }
    if bundle.icon.is_some() {
        fs::copy(
            format!("{target_dir}/Assets.car"),
            format!("{resources}/Assets.car"),
        )
        .expect("Failed to copy Assets.car");
    }
    crate::assets::copy(binary, &resources);
    fs::copy(binary, format!("{contents}/MacOS/{}", bundle.name))
        .expect("Failed to copy executable");
    fs::copy(
        format!("{target_dir}/Info.plist"),
        format!("{contents}/Info.plist"),
    )
    .expect("Failed to copy Info.plist");
}

pub(crate) fn sign(path: &str, target_dir: &str, bundle: &BundleMetadata) {
    let app = format!("{target_dir}/{}.app", bundle.name);
    let entitlements = bundle
        .macos
        .entitlements
        .as_ref()
        .map(|name| format!("{path}/{name}"));
    let hardened_runtime = bundle
        .macos
        .hardened_runtime
        .unwrap_or(entitlements.is_some());

    let status = codesign(&app, entitlements.as_deref(), hardened_runtime)
        .status()
        .expect("Failed to run codesign");
    assert!(status.success(), "codesign failed");
}

pub(crate) fn create_zip(target_dir: &str, bundle: &BundleMetadata) {
    let zip = format!("{}/{}.zip", target_dir, bundle.name);
    if Path::new(&zip).exists() {
        fs::remove_file(&zip).expect("Failed to remove existing zip");
    }
    let status = Command::new("zip")
        .args([
            "-r",
            &format!("{}.zip", bundle.name),
            &format!("{}.app", bundle.name),
        ])
        .current_dir(target_dir)
        .status()
        .expect("Failed to run zip");
    assert!(status.success(), "zip command failed");
}

pub(crate) fn create_dmg(target_dir: &str, bundle: &BundleMetadata) {
    let disk_dir = format!("{target_dir}/disk");
    let app_name = format!("{}.app", bundle.name);
    fs::create_dir_all(&disk_dir).expect("Failed to create disk directory");
    copy_dir(
        format!("{target_dir}/{app_name}"),
        format!("{disk_dir}/{app_name}"),
    )
    .expect("Failed to copy app bundle to disk");

    let applications = format!("{disk_dir}/Applications");
    if Path::new(&applications).exists() {
        fs::remove_file(&applications).expect("Failed to remove existing Applications symlink");
    }
    std::os::unix::fs::symlink("/Applications", &applications)
        .expect("Failed to create Applications symlink");

    let dmg = format!("{}/{}.dmg", target_dir, bundle.name);
    if Path::new(&dmg).exists() {
        fs::remove_file(&dmg).expect("Failed to remove existing DMG");
    }
    let status = Command::new("hdiutil")
        .args([
            "create",
            "-srcfolder",
            &disk_dir,
            "-volname",
            &bundle.name,
            "-fs",
            "HFS+",
            "-format",
            "UDZO",
            &dmg,
        ])
        .status()
        .expect("Failed to run hdiutil");
    assert!(status.success(), "hdiutil failed to create DMG");
}

/// Run the app's executable in the foreground, attaching its console output.
pub(crate) fn run(target_dir: &str, bundle: &BundleMetadata) -> ! {
    let executable = format!("{}/MacOS/{}", contents_dir(target_dir, bundle), bundle.name);
    let status = Command::new(&executable)
        .status()
        .unwrap_or_else(|error| panic!("Failed to run {executable}: {error}"));
    exit(status.code().unwrap_or(1));
}

const fn has_icon(bundle: &BundleMetadata) -> bool {
    bundle.macos.iconset.is_some() || bundle.macos.icns.is_some() || bundle.icon.is_some()
}

fn contents_dir(target_dir: &str, bundle: &BundleMetadata) -> String {
    format!("{target_dir}/{}.app/Contents", bundle.name)
}
