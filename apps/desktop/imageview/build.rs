/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("README.md")]

fn main() {
    // Compile Windows resources
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        let manifest = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3" manifestVersion="1.0">
    <assemblyIdentity type="win32" name="ImageView" version="{}.0" processorArchitecture="*"/>
    <dependency>
        <dependentAssembly>
            <assemblyIdentity type="win32" name="Microsoft.Windows.Common-Controls" version="6.0.0.0" processorArchitecture="*" publicKeyToken="6595b64144ccf1df" language="*"/>
        </dependentAssembly>
    </dependency>
    <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
        <application>
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
"#,
            env!("CARGO_PKG_VERSION")
        );

        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "ImageView")
            .set("FileDescription", "ImageView")
            .set("LegalCopyright", "Copyright © 2026 Bastiaan van der Plaat")
            .set_icon("meta/windows/icon.ico")
            .set_icon_with_id("meta/windows/file-types/apng.ico", "2")
            .set_icon_with_id("meta/windows/file-types/bmp.ico", "3")
            .set_icon_with_id("meta/windows/file-types/gif.ico", "4")
            .set_icon_with_id("meta/windows/file-types/icns.ico", "5")
            .set_icon_with_id("meta/windows/file-types/ico.ico", "6")
            .set_icon_with_id("meta/windows/file-types/jpeg2000.ico", "7")
            .set_icon_with_id("meta/windows/file-types/jpeg.ico", "8")
            .set_icon_with_id("meta/windows/file-types/netpbm.ico", "9")
            .set_icon_with_id("meta/windows/file-types/png.ico", "10")
            .set_icon_with_id("meta/windows/file-types/qoi.ico", "11")
            .set_icon_with_id("meta/windows/file-types/svg.ico", "12")
            .set_icon_with_id("meta/windows/file-types/tga.ico", "13")
            .set_icon_with_id("meta/windows/file-types/tiff.ico", "14")
            .set_icon_with_id("meta/windows/file-types/tinyvg.ico", "15")
            .set_icon_with_id("meta/windows/file-types/webp.ico", "16")
            .set_manifest(&manifest);
        res.compile().expect("Failed to compile Windows resources.");
    }

    // Embed Info.plist so unbundled runs (e.g. cargo run) still know the document types
    println!("cargo:rerun-if-env-changed=CARGO_BUNDLE");
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "macos"
        && std::env::var_os("CARGO_BUNDLE").is_none()
    {
        println!("cargo:rerun-if-changed=Info.plist");
        println!(
            "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}/Info.plist",
            env!("CARGO_MANIFEST_DIR")
        );
    }
}
