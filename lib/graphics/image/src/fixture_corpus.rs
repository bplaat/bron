/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Test fixtures under the including crate's tests/images and tests/reference, shared with the
// vector crate through a #[path] module.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) struct FixtureCorpus {
    pub(crate) images: PathBuf,
    pub(crate) references: PathBuf,
}

impl FixtureCorpus {
    pub(crate) fn new(format: &str) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        Self {
            images: root.join("images").join(format),
            references: root.join("reference").join(format),
        }
    }

    pub(crate) fn image_files(&self, extension: &str) -> Vec<PathBuf> {
        files_with_extension(&self.images, extension)
    }

    pub(crate) fn reference_files(&self, extension: &str) -> Vec<PathBuf> {
        files_with_extension(&self.references, extension)
    }

    pub(crate) fn reference_for(&self, image: &Path, suffix: &str) -> PathBuf {
        let mut name = self.relative_name(image).as_os_str().to_owned();
        name.push(suffix);
        self.references.join(name)
    }

    pub(crate) fn relative_name<'a>(&self, image: &'a Path) -> &'a Path {
        image
            .strip_prefix(&self.images)
            .expect("relative fixture path")
    }
}

fn files_with_extension(directory: &Path, extension: &str) -> Vec<PathBuf> {
    fn visit(directory: &Path, extension: &str, output: &mut Vec<PathBuf>) {
        let mut entries = fs::read_dir(directory)
            .expect("read fixture directory")
            .map(|entry| entry.expect("read fixture entry").path())
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                visit(&path, extension, output);
            } else if path.extension().is_some_and(|value| value == extension) {
                output.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(directory, extension, &mut files);
    files
}
