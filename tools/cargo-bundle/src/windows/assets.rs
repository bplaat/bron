/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{fs, io};

// Preserve the runtime paths when Windows folds this package into the selected code package.
pub(super) fn stage(binaries: &[PathBuf], layout: &Path) -> io::Result<Vec<String>> {
    let shared = crate::assets::collect_shared(binaries)?;
    let mut files = Vec::with_capacity(shared.len());
    for (name, bytes) in shared {
        let name = format!("Assets/{name}");
        let destination = layout.join(&name);
        fs::create_dir_all(destination.parent().expect("Asset has a parent"))?;
        fs::write(destination, bytes)?;
        files.push(name);
    }
    Ok(files)
}

pub(super) fn mapping(layout: &Path, files: &[String]) -> io::Result<String> {
    // Omitting ResourceDimensions makes this an always-installed asset package.
    let mut text = String::from("[ResourceMetadata]\n\"ResourceId\" \"assets\"\n\n[Files]\n");
    for name in files {
        let source = std::path::absolute(layout.join(name))?;
        let source = source
            .to_str()
            .ok_or_else(|| io::Error::other("MSIX asset paths must be UTF-8"))?;
        if source.contains(['"', '\r', '\n']) || name.contains(['"', '\r', '\n']) {
            return Err(io::Error::other(
                "MSIX asset paths cannot contain quotes or newlines",
            ));
        }
        writeln!(text, "\"{source}\" \"{}\"", name.replace('/', "\\"))
            .expect("Writing to a String cannot fail");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Clock")
                .as_nanos();
            let directory =
                std::env::temp_dir().join(format!("msix-assets-{}-{unique}", std::process::id()));
            fs::create_dir_all(&directory).expect("Scratch directory");
            Self(directory)
        }

        fn binary(&self, name: &str, source: &Path, files: &[&str]) -> PathBuf {
            let json = serde_json::to_vec(&serde_json::json!({
                "collection": "0123456789abcdef", "root": source, "files": files,
            }))
            .expect("Metadata");
            let mut record = b"\0cargo-bundle-assets:v1\0".to_vec();
            record.extend_from_slice(&(json.len() as u32).to_le_bytes());
            record.extend(json);
            let binary = self.0.join(name);
            fs::write(&binary, record).expect("Executable metadata");
            binary
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn stages_one_shared_copy_and_maps_folded_runtime_paths() {
        let scratch = Scratch::new();
        let source = scratch.0.join("source with spaces");
        fs::create_dir_all(source.join("nested")).expect("Source directory");
        fs::write(source.join("nested/texture.bin"), b"shared texture").expect("Shared texture");
        fs::write(source.join("extra.bin"), b"additional asset").expect("Additional asset");
        let first = scratch.binary("x64.exe", &source, &["nested/texture.bin"]);
        let second = scratch.binary("arm64.exe", &source, &["nested/texture.bin", "extra.bin"]);
        let layout = scratch.0.join("shared");
        let files = stage(&[first, second], &layout).expect("Stage architectures");
        assert_eq!(files.len(), 2);
        assert_eq!(
            fs::read(layout.join("Assets/0123456789abcdef/nested/texture.bin")).expect("Texture"),
            b"shared texture"
        );
        let mapping = mapping(&layout, &files).expect("Asset mapping");
        assert!(mapping.starts_with("[ResourceMetadata]\n\"ResourceId\" \"assets\"\n\n[Files]\n"));
        assert!(!mapping.contains("ResourceDimensions"));
        assert!(!mapping.contains("resources.pri"));
        let entries: Vec<_> = mapping
            .split_once("[Files]\n")
            .expect("File section")
            .1
            .lines()
            .collect();
        assert_eq!(entries.len(), 2);
        for entry in entries {
            let (source, destination) = entry
                .strip_prefix('"')
                .expect("Quoted source")
                .split_once("\" \"")
                .expect("Path pair");
            assert!(Path::new(source).is_file());
            let destination = destination.strip_suffix('"').expect("Quoted destination");
            assert!(destination.starts_with("Assets\\0123456789abcdef\\"));
            assert_eq!(
                fs::read(source).expect("Mapped source"),
                fs::read(layout.join(destination.replace('\\', "/"))).expect("Mapped destination")
            );
        }
    }

    #[test]
    fn omits_empty_packages_and_rejects_cross_architecture_conflicts() {
        let scratch = Scratch::new();
        let empty = scratch.0.join("empty.exe");
        fs::write(&empty, b"no asset declarations").expect("Empty executable");
        let layout = scratch.0.join("shared");
        assert!(stage(&[empty], &layout).expect("Empty assets").is_empty());
        assert!(!layout.exists());

        let first = scratch.0.join("first");
        let second = scratch.0.join("second");
        fs::create_dir_all(&first).expect("First root");
        fs::create_dir_all(&second).expect("Second root");
        fs::write(first.join("texture.bin"), b"one").expect("First payload");
        fs::write(second.join("texture.bin"), b"two").expect("Second payload");
        let first = scratch.binary("x64.exe", &first, &["texture.bin"]);
        let second = scratch.binary("arm64.exe", &second, &["texture.bin"]);
        assert!(stage(&[first, second], &layout).is_err());
        assert!(!layout.exists());
    }
}
