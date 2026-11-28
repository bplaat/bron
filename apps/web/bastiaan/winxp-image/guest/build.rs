/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// v86disp is a display driver: a DLL of the native subsystem that GDI starts at its entry point,
// DrvEnableDriver, without exports. The other drivers are .sys images with DriverEntry.
fn main() {
    if std::env::var("TARGET").is_ok_and(|target| target.ends_with("i686-winxp-windows-driver")) {
        println!("cargo:rustc-link-arg-bin=v86disp=--dll");
        println!("cargo:rustc-link-arg-bin=v86disp=--entry=_DrvEnableDriver@12");
    }
}
