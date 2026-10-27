// Copyright (c) 2026 Bastiaan van der Plaat
// SPDX-License-Identifier: MIT

//! Compiles the native game shader.

fn main() -> bwebgpu_shader_build::Result<()> {
    println!("cargo:rerun-if-changed=meta/windows");
    compile_windows_resources();
    bwebgpu_shader_build::Builder::new()
        .shader("scene.wgsl", "src/scene.wgsl")
        .compile()
}

fn compile_windows_resources() {
    // Compile Windows resources
    if std::env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS not set") == "windows" {
        let manifest = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3" manifestVersion="1.0">
    <assemblyIdentity type="win32" name="Frogger" version="{}.0" processorArchitecture="*"/>
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
        res.set("ProductName", "Frogger")
            .set("FileDescription", "Frogger")
            .set(
                "LegalCopyright",
                "Copyright (c) 2026 Bastiaan van der Plaat",
            )
            .set_icon("meta/windows/icon.ico")
            .set_manifest(&manifest);
        res.compile().expect("Failed to compile Windows resources.");
    }
}
