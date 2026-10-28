/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::process::exit;

use crate::manifest::Manifest;

pub(crate) fn publisher_name(manifest: &Manifest) -> String {
    let author = manifest.package.authors.first().unwrap_or_else(|| {
        eprintln!("Package needs authors to create a MSIX publisher");
        exit(1);
    });
    let name = author.split('<').next().unwrap_or(author).trim().to_owned();
    assert!(
        !name.is_empty() && !name.contains([',', '=', '+', '<', '>', '#', ';', '"', '\\']),
        "Author name can't be used as MSIX publisher: {name}"
    );
    name
}
