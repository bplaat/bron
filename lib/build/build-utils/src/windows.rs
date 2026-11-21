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
        .set_manifest(&minify_xml(&application_manifest(
            name,
            &version,
            &env::var("CARGO_CFG_TARGET_VENDOR").expect("CARGO_CFG_TARGET_VENDOR not set"),
        )));
    for &(path, id) in extra_icons {
        resources.set_icon_with_id(path, id);
    }
    resources.compile()
}

/// Declares support for XP+, Windows 7+ or Windows 10+ according to the target vendor.
/// Windows XP ignores the compatibility section; later versions use their supportedOS entry.
fn application_manifest(name: &str, version: &str, vendor: &str) -> String {
    let windows_vista = if vendor == "winxp" {
        r#"
            <!-- Windows Vista -->
            <supportedOS Id="{e2011457-1546-43c5-a5fe-008deee3d3f0}"/>"#
    } else {
        ""
    };
    let windows_7_8 = if matches!(vendor, "winxp" | "win7") {
        r#"
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
        <application>{windows_vista}{windows_7_8}
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

/// Drops comments and the whitespace between and inside tags.
fn minify_xml(xml: &str) -> String {
    let mut without_comments = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some((before, after)) = rest.split_once("<!--") {
        without_comments.push_str(before);
        rest = after.split_once("-->").map_or("", |(_, after)| after);
    }
    without_comments.push_str(rest);

    let mut minified = String::with_capacity(without_comments.len());
    let (mut in_tag, mut quote) = (false, None);
    let mut chars = without_comments.chars().peekable();
    while let Some(char) = chars.next() {
        if char.is_whitespace() && quote.is_none() {
            while chars.next_if(|char| char.is_whitespace()).is_some() {}
            let next = chars.peek().copied();
            let keep = if in_tag {
                !matches!(next, Some('/' | '?' | '>'))
            } else {
                !(minified.is_empty()
                    || minified.ends_with('>')
                    || matches!(next, None | Some('<')))
            };
            if keep {
                minified.push(' ');
            }
            continue;
        }
        match char {
            '<' if quote.is_none() => in_tag = true,
            '>' if quote.is_none() => in_tag = false,
            '"' | '\'' if in_tag => {
                quote = match quote {
                    None => Some(char),
                    Some(open) if open == char => None,
                    open => open,
                }
            }
            _ => {}
        }
        minified.push(char);
    }
    minified
}

#[cfg(test)]
mod tests {
    use super::{application_manifest, minify_xml};

    #[test]
    fn minify_xml_drops_comments_and_whitespace_but_keeps_values() {
        let xml = r#"<?xml version="1.0" ?>
<assembly a="x  y">
    <!-- Comment -->
    <empty
        b="1"
    />
    <text>PerMonitorV2, PerMonitor</text>
</assembly>
"#;
        assert_eq!(
            minify_xml(xml),
            r#"<?xml version="1.0"?><assembly a="x  y"><empty b="1"/><text>PerMonitorV2, PerMonitor</text></assembly>"#
        );
    }

    #[test]
    fn minified_manifest_keeps_all_settings() {
        let manifest = application_manifest("App", "1.0.0", "winxp");
        let minified = minify_xml(&manifest);
        assert!(!minified.contains('\n') && !minified.contains("<!--"));
        assert!(minified.contains("<dpiAwareness xmlns=\"http://schemas.microsoft.com/SMI/2016/WindowsSettings\">PerMonitorV2, PerMonitor</dpiAwareness>"));
        assert_eq!(minified.matches("<supportedOS ").count(), 5);
    }

    #[test]
    fn manifest_escapes_application_names() {
        let manifest = application_manifest("A&B <\"App\">'", "1.2.3", "pc");
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
        let supported_versions = [
            ("{e2011457-1546-43c5-a5fe-008deee3d3f0}", 6),
            ("{35138b9a-5d96-4fbd-8e2d-a2440225f93a}", 7),
            ("{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}", 8),
            ("{1f676c76-80e1-4239-95bb-83d0f6d0da78}", 8),
            ("{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}", 10),
        ];
        for (vendor, minimum) in [("winxp", 5), ("win7", 7), ("pc", 10)] {
            let manifest = application_manifest("App", "1.0.0", vendor);
            for (id, version) in supported_versions {
                assert_eq!(manifest.contains(id), version >= minimum, "{vendor}: {id}");
            }
        }
    }
}
