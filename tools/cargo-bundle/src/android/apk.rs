/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::fail;
use super::native::ABIS;
use super::toolchain::Toolchain;
use crate::support::remove_path_if_exists;

/// Whether the resources contain a launcher icon for the manifest.
pub(super) fn has_launcher_icon(res_dir: &Path) -> bool {
    fs::read_dir(res_dir).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry.file_name().to_string_lossy().starts_with("mipmap")
                && fs::read_dir(entry.path()).is_ok_and(|files| {
                    files.filter_map(Result::ok).any(|file| {
                        Path::new(&file.file_name()).file_stem() == Some("ic_launcher".as_ref())
                    })
                })
        })
    })
}

/// Link the resources and manifest, add the native libraries, then align and sign.
pub(super) fn package(
    target_dir: &str,
    res_dir: &Path,
    toolchain: &Toolchain,
    min_sdk: u32,
    lib_name: &str,
    output: &Path,
) {
    let build_tools = &toolchain.sdk.build_tools;
    let aapt2 = build_tools.join("aapt2");
    let unaligned = format!("{target_dir}/unaligned.apk");
    let mut link = Command::new(&aapt2);
    link.arg("link")
        .arg("-I")
        .arg(&toolchain.sdk.platform_jar)
        .args(["--manifest", &format!("{target_dir}/AndroidManifest.xml")])
        .args(["-o", &unaligned]);
    if res_dir.is_dir() {
        let compiled = format!("{target_dir}/resources.zip");
        remove_path_if_exists(&compiled);
        let status = Command::new(&aapt2)
            .arg("compile")
            .arg("--dir")
            .arg(res_dir)
            .args(["-o", &compiled])
            .status()
            .expect("Failed to run aapt2 compile");
        assert!(status.success(), "aapt2 compile failed");
        link.arg(compiled);
    }
    let status = link.status().expect("Failed to run aapt2 link");
    assert!(status.success(), "aapt2 link failed");

    let mut apk = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&unaligned)
        .expect("Failed to open unaligned.apk");
    for (abi, _, _) in ABIS {
        let name = format!("lib/{abi}/lib{lib_name}.so");
        let library =
            fs::read(format!("{target_dir}/{name}")).expect("Failed to read native library");
        // Stored so the libraries can be mapped directly from the APK.
        zip::append_stored_file(&mut apk, &name, &library).expect("Failed to add native library");
    }
    drop(apk);

    let aligned = format!("{target_dir}/aligned.apk");
    let status = Command::new(build_tools.join("zipalign"))
        .args(["-P", "16", "-f", "4", &unaligned, &aligned])
        .status()
        .expect("Failed to run zipalign");
    assert!(status.success(), "zipalign failed");

    let keystore = debug_keystore(toolchain);
    // Lets Conscrypt in apksigner load its native library without JDK 24+ warnings
    let status = toolchain
        .jdk
        .command(build_tools.join("apksigner"))
        .arg("-J-enable-native-access=ALL-UNNAMED")
        .args(["sign", "--min-sdk-version", &min_sdk.to_string()])
        .args(["--v4-signing-enabled", "false", "--ks"])
        .arg(&keystore)
        .args([
            "--ks-key-alias",
            "androiddebugkey",
            "--ks-pass",
            "pass:android",
            "--key-pass",
            "pass:android",
            "--out",
        ])
        .arg(output)
        .arg(&aligned)
        .status()
        .expect("Failed to run apksigner");
    assert!(status.success(), "apksigner failed");
}

// The shared debug keystore of the Android tools, created like Android Studio does when missing.
fn debug_keystore(toolchain: &Toolchain) -> PathBuf {
    let home = std::env::home_dir().unwrap_or_else(|| fail("Can't find the home directory"));
    let keystore = home.join(".android/debug.keystore");
    if keystore.exists() {
        return keystore;
    }
    fs::create_dir_all(home.join(".android")).expect("Failed to create ~/.android");
    let status = Command::new(toolchain.jdk.tool("keytool"))
        .args(["-genkeypair", "-keystore"])
        .arg(&keystore)
        .args([
            "-storepass",
            "android",
            "-alias",
            "androiddebugkey",
            "-keypass",
            "android",
            "-dname",
            "CN=Android Debug,O=Android,C=US",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "10000",
        ])
        .status()
        .unwrap_or_else(|error| {
            fail(&format!(
                "Failed to run keytool, is a JDK installed? {error}"
            ))
        });
    assert!(status.success(), "keytool failed");
    keystore
}
