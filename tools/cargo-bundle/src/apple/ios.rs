/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;

use copy_dir::copy_dir;

use crate::apple::support::{bundle_info, codesign, compile_icon, merge_custom_info};
use crate::manifest::Manifest;
use crate::support::{assert_file_name, cargo_build, remove_path_if_exists};

const TARGET: &str = "aarch64-apple-ios-sim";
const MINIMUM_OS_VERSION: &str = "15.0";

/// Build an ad-hoc signed iOS app bundle
pub(crate) fn bundle(path: &str, target_dir: &str, manifest: &Manifest) {
    let package = &manifest.package;
    let bundle = &package.metadata.bundle;
    assert_file_name(&bundle.name, "bundle name");
    let target_dir = format!("{target_dir}/ios");
    let app = format!("{target_dir}/{}.app", bundle.name);
    remove_path_if_exists(&app);
    fs::create_dir_all(&target_dir).expect("Can't create target directory");
    // iOS bundles are flat, so resources go in the bundle root
    if let Some(resources_dir) = &bundle.resources_dir {
        copy_dir(format!("{path}/{resources_dir}"), &app)
            .expect("Failed to copy resources directory");
    } else {
        fs::create_dir(&app).expect("Can't create bundle directory");
    }

    let status = cargo_build(path)
        .args(["--target", TARGET])
        .status()
        .expect("Failed to run cargo build");
    assert!(status.success(), "cargo build failed for {TARGET}");
    fs::copy(
        format!("target/{TARGET}/release/{}", package.name),
        format!("{app}/{}", bundle.name),
    )
    .expect("Failed to copy executable");

    let mut info = bundle_info(&bundle.name, &bundle.identifier, &package.version, "APPL");
    info.insert("MinimumOSVersion".into(), MINIMUM_OS_VERSION.into());
    info.insert(
        "CFBundleSupportedPlatforms".into(),
        plist::Value::Array(vec!["iPhoneSimulator".into()]),
    );
    // iPhone and iPad
    info.insert(
        "UIDeviceFamily".into(),
        plist::Value::Array(vec![1.into(), 2.into()]),
    );
    info.insert(
        "UILaunchScreen".into(),
        plist::Value::Dictionary(plist::Dictionary::new()),
    );
    let mut scene_manifest = plist::Dictionary::new();
    scene_manifest.insert("UIApplicationSupportsMultipleScenes".into(), false.into());
    info.insert(
        "UIApplicationSceneManifest".into(),
        plist::Value::Dictionary(scene_manifest),
    );
    info.insert(
        "UISupportedInterfaceOrientations".into(),
        interface_orientations(&[
            "UIInterfaceOrientationPortrait",
            "UIInterfaceOrientationLandscapeLeft",
            "UIInterfaceOrientationLandscapeRight",
        ]),
    );
    // iPad multitasking requires all orientations
    info.insert(
        "UISupportedInterfaceOrientations~ipad".into(),
        interface_orientations(&[
            "UIInterfaceOrientationPortrait",
            "UIInterfaceOrientationPortraitUpsideDown",
            "UIInterfaceOrientationLandscapeLeft",
            "UIInterfaceOrientationLandscapeRight",
        ]),
    );
    if let Some(copyright) = &bundle.copyright {
        info.insert("NSHumanReadableCopyright".into(), copyright.clone().into());
    }
    if let Some(icon) = &bundle.icon {
        info.extend(compile_icon(
            &format!("{path}/{icon}"),
            &app,
            "iphonesimulator",
            &["iphone", "ipad"],
            MINIMUM_OS_VERSION,
        ));
    }
    merge_custom_info(&mut info, path, bundle, bundle.ios.info_plist.as_deref());
    plist::Value::Dictionary(info)
        .to_file_binary(format!("{app}/Info.plist"))
        .expect("Failed to write binary Info.plist");

    let entitlements = bundle
        .ios
        .entitlements
        .as_ref()
        .map(|name| format!("{path}/{name}"));
    let status = codesign(&app, entitlements.as_deref(), false)
        .status()
        .expect("Failed to run codesign");
    assert!(status.success(), "codesign failed");
}

fn interface_orientations(orientations: &[&str]) -> plist::Value {
    plist::Value::Array(
        orientations
            .iter()
            .map(|&orientation| orientation.into())
            .collect(),
    )
}
