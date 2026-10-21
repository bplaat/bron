/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::path::PathBuf;

use android_build::{Jdk, Sdk};

use super::fail;

/// The Android SDK, NDK, and JDK tools used to build an APK.
pub(super) struct Toolchain {
    pub sdk: Sdk,
    pub jdk: Jdk,
    pub ndk_bin: PathBuf,
    pub target_sdk: u32,
}

impl Toolchain {
    pub(super) fn find(target_sdk: Option<u32>) -> Self {
        let sdk = Sdk::find().unwrap_or_else(|error| fail(&error));
        let jdk = Jdk::find().unwrap_or_else(|error| fail(&error));
        let ndk_bin = sdk.ndk_bin().unwrap_or_else(|error| fail(&error));
        Self {
            target_sdk: target_sdk.unwrap_or(sdk.platform),
            sdk,
            jdk,
            ndk_bin,
        }
    }

    /// The NDK clang driver for a target triple that links against `min_sdk`.
    pub(super) fn clang(&self, target: &str, min_sdk: u32) -> PathBuf {
        let clang = self.ndk_bin.join(format!("{target}{min_sdk}-clang"));
        if !clang.exists() {
            fail(&format!(
                "The Android NDK does not support API level {min_sdk}, {} is missing",
                clang.display()
            ));
        }
        clang
    }
}
