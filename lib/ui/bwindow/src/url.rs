/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

/// Open a URL with the system's default handler, like a web link in the default browser
pub fn open_url(url: &str) {
    crate::platforms::open_url(url);
}
