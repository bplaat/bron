/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::{env, io};

use winresource::WindowsResource;

/// Compiles Windows application resources.
///
/// Call this only when the Cargo target is Windows.
///
/// The manifest enables common controls v6, compatibility with the Windows versions the target
/// supports and per-monitor DPI awareness, and requests the caller's existing privileges.
/// Extra icons are `(path, resource_id)` pairs; the application icon uses ID 1.
pub fn compile_application(
    name: &str,
    description: &str,
    copyright: &str,
    icon: &str,
    extra_icons: &[(&str, &str)],
) -> io::Result<()> {
    let version = env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION not set");
    let mut resources = WindowsResource::new();
    resources
        .set("ProductName", name)
        .set("FileDescription", description)
        .set("LegalCopyright", copyright)
        .set_icon(icon)
        .set_manifest(&application_manifest(
            name,
            &version,
            env::var("CARGO_CFG_TARGET_VENDOR").is_ok_and(|vendor| vendor == "win7"),
        ));
    for &(path, id) in extra_icons {
        resources.set_icon_with_id(path, id);
    }
    resources.compile()
}

/// The manifest, the normal Windows targets need Windows 10, the `win7` targets also support
/// Windows Vista, 7, 8 and 8.1, and Windows XP with the XP toolchain, which ignores the
/// compatibility section
fn application_manifest(name: &str, version: &str, win7: bool) -> String {
    let older_supported_os = if win7 {
        r#"
            <!-- Windows Vista -->
            <supportedOS Id="{e2011457-1546-43c5-a5fe-008deee3d3f0}"/>
            <!-- Windows 7 -->
            <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
            <!-- Windows 8 -->
            <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
            <!-- Windows 8.1 -->
            <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>"#
    } else {
        ""
    };
    let name = name
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3" manifestVersion="1.0">
    <assemblyIdentity type="win32" name="{name}" version="{version}.0" processorArchitecture="*"/>
    <dependency>
        <dependentAssembly>
            <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
        </dependentAssembly>
    </dependency>
    <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
        <application>{older_supported_os}
            <!-- Windows 10/11 -->
            <supportedOS Id="{{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}}"/>
        </application>
    </compatibility>
    <asmv3:application>
        <asmv3:windowsSettings>
            <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
            <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
        </asmv3:windowsSettings>
    </asmv3:application>
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
            </requestedPrivileges>
        </security>
    </trustInfo>
</assembly>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::application_manifest;

    #[test]
    fn manifest_escapes_application_names() {
        let manifest = application_manifest("A&B <\"App\">'", "1.2.3", false);
        assert!(
            manifest.contains("name=\"A&amp;B &lt;&quot;App&quot;&gt;&apos;\" version=\"1.2.3.0\"")
        );
        assert!(manifest.contains("PerMonitorV2, PerMonitor"));
        assert!(manifest.contains("level=\"asInvoker\""));
        assert!(
            manifest.contains("name=\"Microsoft.Windows.Common-Controls\" version=\"6.0.0.0\"")
        );
    }

    #[test]
    fn manifest_supports_the_windows_versions_of_the_target() {
        let windows_10 = "{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}";
        let windows_vista = "{e2011457-1546-43c5-a5fe-008deee3d3f0}";
        let manifest = application_manifest("App", "1.0.0", false);
        assert!(manifest.contains(windows_10) && !manifest.contains(windows_vista));
        let manifest = application_manifest("App", "1.0.0", true);
        assert!(manifest.contains(windows_10) && manifest.contains(windows_vista));
    }
}
