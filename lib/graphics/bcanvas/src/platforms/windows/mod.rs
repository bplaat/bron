/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Windows drawing with Direct2D 1.1. The normal Windows targets need Windows 10, which has it.
//! The `winxp` and `win7` targets draw with GDI+ where it is not available. The
//! tests have GDI+ on every Windows and draw with it when `BCANVAS_TEST_GDI` is set.

pub(crate) use self::context::PlatformCanvasContext;
pub(crate) use self::hwnd::PlatformHwndCanvas;
pub(crate) use self::image::PlatformImage;
pub(crate) use self::offscreen::PlatformOffscreenCanvas;
#[cfg(feature = "window")]
pub(crate) use self::window::PlatformCanvas;

mod com;
mod context;
mod dib;
mod direct2d;
#[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
mod gdi;
mod headers;
mod hwnd;
mod image;
mod offscreen;
#[cfg(feature = "window")]
mod window;

/// A native drawing API.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Backend {
    Direct2D,
    #[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
    Gdi,
}

/// The backend of every canvas of the process.
#[cfg(not(any(target_vendor = "winxp", target_vendor = "win7", test)))]
pub(crate) const fn backend() -> Backend {
    Backend::Direct2D
}

/// The backend of every canvas of the process, Direct2D when it is available, chosen once. The
/// tests draw with GDI+ when `BCANVAS_TEST_GDI` is set.
#[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
pub(crate) fn backend() -> Backend {
    static BACKEND: std::sync::OnceLock<Backend> = std::sync::OnceLock::new();
    *BACKEND.get_or_init(|| {
        if !(cfg!(test) && std::env::var_os("BCANVAS_TEST_GDI").is_some()) && direct2d::available()
        {
            Backend::Direct2D
        } else {
            Backend::Gdi
        }
    })
}
