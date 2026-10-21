/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::thread::sleep;
use std::time::Duration;

use self::manifest::{ManifestInfo, is_valid_identifier};
use self::toolchain::Toolchain;
use crate::args::Args;
use crate::manifest::Manifest;
use crate::support::assert_file_name;

mod apk;
mod manifest;
mod native;
mod toolchain;
mod xml;

/// The framework activity that loads the app library, bwindow implements its callbacks.
const ACTIVITY: &str = "android.app.NativeActivity";

fn fail(message: &str) -> ! {
    eprintln!("{message}");
    exit(1);
}

/// Build a signed APK for every ABI and optionally install and run it.
pub(crate) fn bundle(args: &Args, target_dir: &str, manifest: &Manifest) {
    if cfg!(windows) {
        fail("Android bundles can only be built on macOS and Linux");
    }
    if args.ios || args.zip || args.dmg {
        fail("--ios, --zip, and --dmg are not supported for Android bundles");
    }
    let path = args.path.as_str();
    let package = &manifest.package;
    let bundle = &package.metadata.bundle;
    assert_file_name(&bundle.name, "bundle name");
    let android = bundle.android.clone().unwrap_or_default();
    let identifier = bundle.identifier.as_str();
    if !is_valid_identifier(identifier) {
        fail(&format!(
            "Invalid bundle identifier {identifier} for Android: every segment must start with a \
             letter and only contain letters, digits, and underscores"
        ));
    }
    let min_sdk = android.min_sdk_version.unwrap_or(android_build::MIN_SDK);
    if min_sdk < android_build::MIN_SDK {
        fail(&format!(
            "min_sdk_version {min_sdk} is too low, bwindow requires at least Android API level {}",
            android_build::MIN_SDK
        ));
    }
    let toolchain = Toolchain::find(android.target_sdk_version);

    let android_dir = format!("{target_dir}/android");
    fs::create_dir_all(&android_dir).expect("Failed to create Android build directory");
    native::build(path, &android_dir, &package.name, &toolchain, min_sdk);

    let res_dir = Path::new(path).join(android.res_dir.as_deref().unwrap_or("meta/android/res"));
    let extension = Path::new(path).join(
        android
            .manifest
            .as_deref()
            .unwrap_or("meta/android/AndroidManifest.xml"),
    );
    manifest::write(
        &ManifestInfo {
            identifier,
            version: &package.version,
            label: &bundle.name,
            lib_name: &package.name,
            min_sdk,
            target_sdk: toolchain.target_sdk,
            has_icon: apk::has_launcher_icon(&res_dir),
        },
        &extension,
        &Path::new(&android_dir).join("AndroidManifest.xml"),
    );

    let apk = PathBuf::from(format!("{target_dir}/{}.apk", bundle.name));
    apk::package(
        &android_dir,
        &res_dir,
        &toolchain,
        min_sdk,
        &package.name,
        &apk,
    );
    println!("Created {}", apk.display());
    if args.run {
        install_and_run(&apk, identifier);
    }
}

// MARK: Run
fn install_and_run(apk: &Path, identifier: &str) -> ! {
    let adb_path = android_build::adb();
    let adb = |args: &[&str]| {
        Command::new(&adb_path)
            .args(args)
            .output()
            .unwrap_or_else(|error| fail(&format!("Failed to run adb: {error}")))
    };

    let apk = apk.to_string_lossy();
    let mut output = adb(&["install", "-r", &apk]);
    // A different signing key requires a reinstall.
    if String::from_utf8_lossy(&output.stderr).contains("INSTALL_FAILED_UPDATE_INCOMPATIBLE") {
        adb(&["uninstall", identifier]);
        output = adb(&["install", "-r", &apk]);
    }
    if !output.status.success() {
        fail(&format!(
            "adb install failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = adb(&[
        "shell",
        "am",
        "start",
        "-S",
        "-W",
        "-n",
        &format!("{identifier}/{ACTIVITY}"),
    ]);
    if !output.status.success() {
        fail("Failed to start the app");
    }
    let pid = (0..50)
        .find_map(|_| {
            let output = adb(&["shell", "pidof", identifier]);
            let pid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if pid.is_empty() {
                sleep(Duration::from_millis(100));
                None
            } else {
                Some(pid)
            }
        })
        .unwrap_or_else(|| fail("The app exited right after starting"));
    let status = Command::new(&adb_path)
        .args(["logcat", &format!("--pid={pid}")])
        .status()
        .unwrap_or_else(|error| fail(&format!("Failed to run adb logcat: {error}")));
    exit(status.code().unwrap_or(1));
}
