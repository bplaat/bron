/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::path::Path;

use crate::Result;

pub(crate) fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    if fs::read(path).is_ok_and(|previous| previous == contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("{}: create output directory: {e}", parent.display()))?;
    }
    fs::write(path, contents)
        .map_err(|e| format!("{}: write shader output: {e}", path.display()))?;
    Ok(())
}
