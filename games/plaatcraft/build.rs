/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Compiles the game shaders and platform resources.

fn compile_windows_resources() {
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") != "windows" {
        return;
    }

    let manifest = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3" manifestVersion="1.0">
    <assemblyIdentity type="win32" name="PlaatCraft" version="{}.0" processorArchitecture="*"/>
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

    let mut resources = winresource::WindowsResource::new();
    resources
        .set("ProductName", "PlaatCraft")
        .set("FileDescription", "PlaatCraft voxel sandbox")
        .set(
            "LegalCopyright",
            "Copyright (c) 2026 Bastiaan van der Plaat",
        )
        .set_icon("meta/windows/icon.ico")
        .set_manifest(&manifest);
    resources
        .compile()
        .expect("failed to compile Windows resources");
}

fn main() -> bwebgpu_shader_build::Result<()> {
    compile_windows_resources();

    bwebgpu_shader_build::Builder::new()
        .shader("terrain.wgsl", "src/shaders/terrain.wgsl")
        .shader("liquid.wgsl", "src/shaders/liquid.wgsl")
        .shader("sky.wgsl", "src/shaders/sky.wgsl")
        .shader("crosshair.wgsl", "src/shaders/crosshair.wgsl")
        .shader("selection.wgsl", "src/shaders/selection.wgsl")
        .shader("preview.wgsl", "src/shaders/preview.wgsl")
        .compile()
}
