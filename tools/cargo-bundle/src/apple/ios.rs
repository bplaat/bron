/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use copy_dir::copy_dir;
use serde::Deserialize;

use crate::apple::support::{bundle_info, codesign, compile_icon, merge_custom_info};
use crate::args::Args;
use crate::manifest::Manifest;
use crate::support::{assert_file_name, remove_path_if_exists};

const TARGET: &str = "aarch64-apple-ios-sim";
const MINIMUM_OS_VERSION: &str = "15.0";

/// Build an ad-hoc signed iOS app bundle
pub(crate) fn bundle(args: &Args, target_dir: &str, manifest: &Manifest) {
    let path = &args.path;
    let simulator = args.run.then(|| prepare_simulator(args.device.as_deref()));
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

    let executable = build_executable(args);
    fs::copy(executable, format!("{app}/{}", bundle.name)).expect("Failed to copy executable");

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
    let identifier = info
        .get("CFBundleIdentifier")
        .and_then(|value| match value {
            plist::Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .expect("Info.plist must contain CFBundleIdentifier")
        .to_string();
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
    if let Some(simulator) = simulator {
        run(Command::new("xcrun").args(["simctl", "install", &simulator, &app]));
        run(Command::new("xcrun").args([
            "simctl",
            "launch",
            "--terminate-running-process",
            "--console",
            &simulator,
            &identifier,
        ]));
    }
}

fn interface_orientations(orientations: &[&str]) -> plist::Value {
    plist::Value::Array(
        orientations
            .iter()
            .map(|&orientation| orientation.into())
            .collect(),
    )
}

fn run(command: &mut Command) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("Failed to run {command:?}: {error}"));
    assert!(status.success(), "{command:?} failed: {status}");
}

#[derive(Deserialize)]
struct Device {
    name: String,
    udid: String,
    state: String,
}

fn select_simulator(devices: &serde_json::Value, selected: Option<&str>) -> Device {
    let mut candidates: Vec<Device> = devices["devices"]
        .as_object()
        .expect("Invalid simulator list")
        .iter()
        .filter(|(runtime, _)| runtime.contains(".iOS-"))
        .flat_map(|(_, devices)| devices.as_array().into_iter().flatten())
        .map(|device| serde_json::from_value(device.clone()).expect("Invalid simulator device"))
        .filter(|device: &Device| {
            selected.is_none_or(|selected| device.udid == selected || device.name == selected)
        })
        .collect();
    candidates.sort_by(|a, b| {
        (a.state != "Booted", &a.name, &a.udid).cmp(&(b.state != "Booted", &b.name, &b.udid))
    });
    if selected.is_some_and(|selected| candidates.iter().all(|device| device.udid != selected)) {
        assert!(
            candidates.len() <= 1,
            "Simulator name is ambiguous; use --device with a UDID"
        );
    }
    candidates.into_iter().next().expect("No matching iOS simulator; install a runtime and create a device in Xcode, or select one with --device")
}

fn prepare_simulator(selected: Option<&str>) -> String {
    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "available", "--json"])
        .output()
        .expect("Cannot run simctl; install Xcode and select it with xcode-select");
    assert!(
        output.status.success(),
        "Cannot list simulators: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let devices = serde_json::from_slice(&output.stdout).expect("Invalid simctl JSON");
    let device = select_simulator(&devices, selected);
    println!("Using simulator {} ({})", device.name, device.udid);
    run(Command::new("xcrun").args(["simctl", "bootstatus", &device.udid, "-b"]));
    let opened = Command::new("open")
        .args([
            "-a",
            "Simulator",
            "--args",
            "-CurrentDeviceUDID",
            &device.udid,
        ])
        .status();
    if !opened.is_ok_and(|status| status.success()) {
        eprintln!("Could not open Simulator.app; continuing on the booted simulator");
    }
    device.udid
}

fn build_executable(args: &Args) -> String {
    let mut command = Command::new("cargo");
    command
        .args([
            "build",
            "--manifest-path",
            &format!("{}/Cargo.toml", args.path),
            "--target",
            TARGET,
            "--message-format=json-render-diagnostics",
        ])
        .env("CARGO_BUNDLE", "1")
        .stdout(Stdio::piped());
    if !args.run || args.release {
        command.arg("--release");
    }
    if let Some(binary) = &args.bin {
        command.args(["--bin", binary]);
    } else {
        command.arg("--bins");
    }
    let mut child = command.spawn().expect("Failed to run cargo build");
    let mut executables = Vec::new();
    for line in BufReader::new(child.stdout.take().expect("Missing Cargo output")).lines() {
        let line = line.expect("Failed to read Cargo output");
        let message: serde_json::Value = serde_json::from_str(&line).expect("Invalid Cargo JSON");
        if message["reason"] == "compiler-message" {
            if let Some(rendered) = message["message"]["rendered"].as_str() {
                eprint!("{rendered}");
            }
        } else if message["reason"] == "compiler-artifact"
            && message["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
            && let Some(executable) = message["executable"].as_str()
        {
            executables.push(executable.to_string());
        }
    }
    assert!(
        child.wait().expect("Failed to wait for Cargo").success(),
        "iOS build failed; ensure rustup target add {TARGET} and the Xcode iOS SDK are installed"
    );
    assert_eq!(
        executables.len(),
        1,
        "Expected one application binary; select one with --bin <name>"
    );
    executables.pop().expect("Missing application executable")
}

#[cfg(test)]
mod tests {
    use super::select_simulator;

    #[test]
    fn selects_ios_and_prefers_booted_device() {
        let devices = serde_json::json!({"devices": {
            "com.apple.CoreSimulator.SimRuntime.tvOS-26-0": [{"name":"TV", "udid":"tv", "state":"Booted"}],
            "com.apple.CoreSimulator.SimRuntime.iOS-26-0": [
                {"name":"A", "udid":"a", "state":"Shutdown"},
                {"name":"B", "udid":"b", "state":"Booted"}
            ]
        }});
        assert_eq!(select_simulator(&devices, None).udid, "b");
        assert_eq!(select_simulator(&devices, Some("A")).udid, "a");
        assert_eq!(select_simulator(&devices, Some("b")).udid, "b");
    }
}
