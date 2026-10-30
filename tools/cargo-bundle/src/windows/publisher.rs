/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fs;
use std::process::exit;

use serde::Deserialize;

use crate::manifest::{Authors, Manifest};

#[derive(Deserialize)]
struct WorkspaceManifest {
    workspace: Option<Workspace>,
}

#[derive(Deserialize)]
struct Workspace {
    package: Option<WorkspacePackage>,
}

#[derive(Deserialize)]
struct WorkspacePackage {
    authors: Option<Vec<String>>,
}

pub(crate) fn publisher_name(path: &str, manifest: &Manifest) -> String {
    let authors = match &manifest.package.authors {
        Some(Authors::List(authors)) => authors.clone(),
        Some(Authors::Workspace { workspace: true }) => workspace_authors(path),
        _ => Vec::new(),
    };
    let author = authors.first().unwrap_or_else(|| {
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

fn workspace_authors(path: &str) -> Vec<String> {
    let path = fs::canonicalize(path).expect("Failed to resolve crate path");
    for directory in path.ancestors().skip(1) {
        let manifest_path = directory.join("Cargo.toml");
        let Ok(contents) = fs::read_to_string(&manifest_path) else {
            continue;
        };
        let manifest: WorkspaceManifest = basic_toml::from_str(&contents)
            .unwrap_or_else(|error| panic!("Can't parse {}: {error}", manifest_path.display()));
        if let Some(workspace) = manifest.workspace {
            return workspace
                .package
                .and_then(|package| package.authors)
                .unwrap_or_default();
        }
    }
    Vec::new()
}
