/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Browsing and launching for platforms that leave recent files to the file manager.

use std::path::{Path, PathBuf};

use crate::AppEvent;

/// Returns the path of the image `offset` places from `path` inside its folder.
pub(crate) fn neighbour_path(path: &Path, offset: isize) -> Option<PathBuf> {
    imageview_core::neighbour_path(
        path,
        offset,
        imageview_core::is_supported,
        imageview_core::compare_names,
    )
}

/// Recent files are left to the file manager.
pub(crate) const fn note_recent_file(_path: &Path) {}

/// There is nothing to set up.
pub(crate) fn init(_proxy: bwindow::EventLoopProxy<AppEvent>) {}
