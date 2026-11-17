/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The Direct2D 1.1 and DirectWrite backend.

pub(crate) use self::context::{Context, ImageCache};
pub(crate) use self::target::{OffscreenTarget, WindowTarget};

mod context;
mod headers;
mod target;

/// Whether Direct2D 1.1 and DirectWrite load and create their factories.
#[cfg(any(target_vendor = "winxp", target_vendor = "win7", test))]
pub(crate) fn available() -> bool {
    unsafe {
        headers::ID2D1Factory::new().is_ok_and(|factory| {
            factory
                .query::<headers::ID2D1Factory>(&headers::IID_ID2D1_FACTORY1)
                .is_ok()
        }) && headers::IDWriteFactory::new().is_ok()
    }
}
