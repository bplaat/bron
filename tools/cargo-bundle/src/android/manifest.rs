/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::Path;

use super::xml::Element;
use super::{ACTIVITY, fail};

// The usual window size changes plus every other change, because recreating the activity
// would end the app: the process runs one Rust main and exits with the activity.
const CONFIG_CHANGES: &str = "orientation|screenSize|screenLayout|smallestScreenSize|keyboard|\
    keyboardHidden|uiMode|density|locale|layoutDirection|fontScale|fontWeightAdjustment|\
    colorMode|grammaticalGender|mcc|mnc|navigation|touchscreen";

pub(super) struct ManifestInfo<'a> {
    pub identifier: &'a str,
    pub version: &'a str,
    pub label: &'a str,
    pub lib_name: &'a str,
    pub min_sdk: u32,
    pub target_sdk: u32,
    pub has_icon: bool,
}

/// Generate the AndroidManifest.xml from bundle metadata, then merge the optional extension file.
pub(super) fn write(info: &ManifestInfo, extension: &Path, output: &Path) {
    let mut manifest = generate(info);
    if extension.exists() {
        let source = fs::read_to_string(extension)
            .unwrap_or_else(|error| fail(&format!("Can't read {}: {error}", extension.display())));
        let extension = Element::parse(&source).unwrap_or_else(|error| {
            fail(&format!(
                "Invalid manifest {}: {error}",
                extension.display()
            ))
        });
        if extension.name != "manifest" {
            fail("Invalid Android manifest extension: root element must be <manifest>");
        }
        merge(&mut manifest, extension);
    }
    fs::write(output, manifest.to_document()).expect("Failed to write AndroidManifest.xml");
}

fn generate(info: &ManifestInfo) -> Element {
    let mut application = Element::new("application").attr("android:label", info.label);
    if info.has_icon {
        application.set("android:icon", "@mipmap/ic_launcher");
    }
    let application = application
        .attr("android:hasCode", "false")
        .attr("android:extractNativeLibs", "false")
        // Deliver the back gesture as a back key, also when targeting Android 16
        .attr("android:enableOnBackInvokedCallback", "false")
        .child(
            Element::new("activity")
                .attr("android:name", ACTIVITY)
                .attr("android:launchMode", "singleTask")
                .attr("android:configChanges", CONFIG_CHANGES)
                .attr("android:exported", "true")
                .child(
                    Element::new("meta-data")
                        .attr("android:name", "android.app.lib_name")
                        .attr("android:value", info.lib_name),
                )
                .child(
                    Element::new("intent-filter")
                        .child(
                            Element::new("action")
                                .attr("android:name", "android.intent.action.MAIN"),
                        )
                        .child(
                            Element::new("category")
                                .attr("android:name", "android.intent.category.LAUNCHER"),
                        ),
                ),
        );
    Element::new("manifest")
        .attr(
            "xmlns:android",
            "http://schemas.android.com/apk/res/android",
        )
        .attr("package", info.identifier)
        .attr(
            "android:versionCode",
            version_code(info.version).to_string(),
        )
        .attr("android:versionName", info.version)
        .child(
            Element::new("uses-sdk")
                .attr("android:minSdkVersion", info.min_sdk.to_string())
                .attr("android:targetSdkVersion", info.target_sdk.to_string()),
        )
        .child(application)
}

// Singleton elements and elements with the same android:name merge their attributes,
// the custom value wins.
fn is_same_element(base: &Element, extension: &Element) -> bool {
    base.name == extension.name
        && match extension.name.as_str() {
            "application" | "uses-sdk" => true,
            _ => extension
                .get("android:name")
                .is_some_and(|name| base.get("android:name") == Some(name)),
        }
}

fn merge(base: &mut Element, extension: Element) {
    for (name, value) in extension.attributes {
        base.set(&name, value);
    }
    for child in extension.children {
        match base
            .children
            .iter_mut()
            .find(|existing| is_same_element(existing, &child))
        {
            Some(existing) => merge(existing, child),
            None => base.children.push(child),
        }
    }
}

/// Derive a monotonic version code from a `major.minor.patch` version.
pub(super) fn version_code(version: &str) -> u32 {
    let mut parts = version.split(['.', '-', '+']).map(|part| {
        part.parse::<u32>()
            .unwrap_or_else(|_| fail(&format!("Version {version} must be major.minor.patch")))
    });
    let mut part = || parts.next().unwrap_or(0).min(999);
    part() * 1_000_000 + part() * 1_000 + part()
}

/// Convert bundle identifier dashes to underscores for Android application ids.
pub(super) fn normalize_identifier(identifier: &str) -> String {
    identifier.replace('-', "_")
}

/// Check that an identifier is a valid Android application id.
pub(super) fn is_valid_identifier(identifier: &str) -> bool {
    let segments = identifier.split('.').collect::<Vec<_>>();
    segments.len() >= 2
        && segments.iter().all(|segment| {
            segment.starts_with(|c: char| c.is_ascii_alphabetic())
                && segment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ManifestInfo<'static> {
        ManifestInfo {
            identifier: "nl.bplaat.game",
            version: "1.2.3",
            label: "Game",
            lib_name: "game",
            min_sdk: 21,
            target_sdk: 37,
            has_icon: false,
        }
    }

    #[test]
    fn generated_manifest_launches_the_bwindow_activity() {
        let manifest = generate(&info());
        assert_eq!(manifest.get("package"), Some("nl.bplaat.game"));
        assert_eq!(manifest.get("android:versionCode"), Some("1002003"));
        let application = &manifest.children[1];
        assert_eq!(application.get("android:icon"), None);
        let activity = &application.children[0];
        assert_eq!(activity.get("android:name"), Some(ACTIVITY));
        assert_eq!(activity.children[0].get("android:value"), Some("game"));
        let icon = generate(&ManifestInfo {
            has_icon: true,
            ..info()
        });
        assert_eq!(
            icon.children[1].get("android:icon"),
            Some("@mipmap/ic_launcher")
        );
    }

    #[test]
    fn extension_overrides_attributes_and_appends_other_elements() {
        let mut manifest = generate(&info());
        let extension = Element::parse(&format!(
            r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" android:installLocation="auto">
    <uses-sdk android:minSdkVersion="26" />
    <uses-permission android:name="android.permission.VIBRATE" />
    <application android:label="Custom" android:theme="@style/Theme.App">
        <activity android:name="{ACTIVITY}" android:screenOrientation="portrait">
            <meta-data android:name="custom" android:value="1" />
            <meta-data android:name="android.app.lib_name" android:value="other" />
        </activity>
        <activity android:name="nl.bplaat.Other" />
    </application>
</manifest>"#
        ))
        .unwrap();
        merge(&mut manifest, extension);
        assert_eq!(manifest.get("android:installLocation"), Some("auto"));
        assert_eq!(
            manifest.children[0].get("android:minSdkVersion"),
            Some("26")
        );
        assert_eq!(
            manifest.children[0].get("android:targetSdkVersion"),
            Some("37")
        );
        assert_eq!(manifest.children.len(), 3);
        assert_eq!(manifest.children[2].name, "uses-permission");
        let application = &manifest.children[1];
        assert_eq!(application.get("android:label"), Some("Custom"));
        assert_eq!(application.get("android:theme"), Some("@style/Theme.App"));
        assert_eq!(application.children.len(), 2);
        let activity = &application.children[0];
        assert_eq!(activity.get("android:screenOrientation"), Some("portrait"));
        assert_eq!(activity.get("android:exported"), Some("true"));
        assert_eq!(activity.children.len(), 3);
        assert_eq!(activity.children[0].get("android:value"), Some("other"));
        assert_eq!(activity.children[2].get("android:name"), Some("custom"));
        assert_eq!(
            application.children[1].get("android:name"),
            Some("nl.bplaat.Other")
        );
    }

    #[test]
    fn normalizes_bundle_identifiers() {
        let manifest = crate::manifest::Manifest::parse(
            "[package]\nname = \"example-bcanvas-showcase\"\nversion = \"0.1.0\"\n",
            None,
        )
        .expect("valid manifest");
        let identifier = normalize_identifier(&manifest.package.metadata.bundle.identifier);
        assert_eq!(identifier, "nl.bplaat.example_bcanvas_showcase");
        assert!(is_valid_identifier(&identifier));
        assert_eq!(normalize_identifier("com.example.App"), "com.example.App");
        assert!(!is_valid_identifier(&normalize_identifier(
            "nl.bplaat.2048"
        )));
    }

    #[test]
    fn version_codes_and_identifiers() {
        assert_eq!(version_code("0.1.0"), 1_000);
        assert_eq!(version_code("2.10"), 2_010_000);
        assert_eq!(version_code("1.0.0-beta"), 1_000_000);
        assert!(is_valid_identifier("nl.bplaat.game2048"));
        assert!(is_valid_identifier("nl.bplaat.Reversi"));
        assert!(!is_valid_identifier("nl.bplaat.2048"));
        assert!(!is_valid_identifier("game"));
        assert!(!is_valid_identifier("nl..game"));
    }
}
