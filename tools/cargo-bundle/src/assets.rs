/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, io};

const MAGIC: &[u8] = b"\0cargo-bundle-assets:v1\0";

/// Select external assets and refresh procedural macro folder enumeration once per invocation.
pub(crate) fn configure(command: &mut Command) {
    static REVISION: OnceLock<String> = OnceLock::new();
    let revision = REVISION.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System clock predates Unix epoch")
            .as_nanos()
            .to_string()
    });
    command
        .env("CARGO_BUNDLE", "1")
        .env("CARGO_BUNDLE_ASSETS_REVISION", revision);
}

/// Collect inline metadata records, including all slices of universal executables.
fn collect(bytes: &[u8]) -> io::Result<BTreeMap<String, PathBuf>> {
    let mut files = BTreeMap::new();
    let mut offset = 0;
    while let Some(start) = bytes[offset..]
        .windows(MAGIC.len())
        .position(|window| window == MAGIC)
    {
        let start = offset + start + MAGIC.len();
        let length = bytes
            .get(start..start + 4)
            .ok_or_else(|| io::Error::other("Truncated asset metadata length"))?;
        let length = u32::from_le_bytes(length.try_into().expect("four bytes")) as usize;
        let start = start + 4;
        let end = start
            .checked_add(length)
            .ok_or_else(|| io::Error::other("Asset metadata too large"))?;
        let record = bytes
            .get(start..end)
            .ok_or_else(|| io::Error::other("Truncated asset metadata"))?;
        // v1 payload: postcard tuple (collection ID, UTF-8 root, relative file names).
        let ((collection, root, names), rest): ((String, PathBuf, Vec<String>), _) =
            postcard::take_from_bytes(record).map_err(io::Error::other)?;
        if !rest.is_empty() {
            return Err(io::Error::other("Trailing asset metadata bytes"));
        }
        if collection.len() != 16 || !collection.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::other("Invalid asset collection ID"));
        }
        let root = root.canonicalize()?;
        for name in names {
            if name.is_empty()
                || name.contains('\\')
                || !Path::new(&name)
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
            {
                return Err(io::Error::other("Invalid relative asset name"));
            }
            let source = root.join(&name).canonicalize()?;
            if !source.starts_with(&root) || !source.is_file() {
                return Err(io::Error::other("Asset is outside its collection"));
            }
            let destination = format!("{collection}/{name}");
            if let Some(previous) = files.insert(destination, source.clone())
                && previous != source
                && fs::read(previous)? != fs::read(source)?
            {
                return Err(io::Error::other(
                    "Conflicting asset collections across architectures",
                ));
            }
        }
        offset = end;
    }
    Ok(files)
}

/// Copy declared resources beside the platform's executable or into its Resources directory.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn copy(binary: impl AsRef<Path>, resources: impl AsRef<Path>) {
    let bytes = fs::read(binary).expect("Failed to read executable asset metadata");
    for (name, source) in collect(&bytes).expect("Failed to collect bundle assets") {
        let destination = resources.as_ref().join(name);
        fs::create_dir_all(destination.parent().expect("Asset has a parent"))
            .expect("Failed to create asset directory");
        fs::copy(source, destination).expect("Failed to copy bundle asset");
    }
}

/// Merge declared asset payloads across executable architectures.
pub(crate) fn collect_shared(binaries: &[PathBuf]) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let mut shared = BTreeMap::new();
    for binary in binaries {
        for (name, source) in collect(&fs::read(binary)?)? {
            let data = fs::read(source)?;
            if let Some(previous) = shared.insert(name, data.clone())
                && previous != data
            {
                return Err(io::Error::other("Conflicting assets across architectures"));
            }
        }
    }
    Ok(shared)
}

/// Add shared APK entries once, even when multiple native ABIs use the same collection.
pub(crate) fn append_apk(apk: &mut fs::File, binaries: &[PathBuf]) -> io::Result<()> {
    let shared = collect_shared(binaries)?;
    for (name, data) in shared {
        zip::append_stored_file(&mut *apk, &format!("assets/{name}"), &data)
            .map_err(io::Error::other)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(root: &Path, name: &str) -> Vec<u8> {
        let data =
            postcard::to_allocvec(&("0123456789abcdef", root, &[name][..])).expect("Valid fixture");
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend(data);
        bytes
    }

    #[test]
    fn collects_universal_records_without_duplicate_resources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets");
        let mut bytes = b"executable header".to_vec();
        bytes.extend(record(&root, "nested/example.txt"));
        bytes.extend(b"second architecture");
        bytes.extend(record(&root, "nested/example.txt"));
        let files = collect(&bytes).expect("Valid fixture");
        assert_eq!(files.len(), 1);
        assert_eq!(
            fs::read(files.values().next().expect("Valid fixture")).expect("Valid fixture"),
            b"bundle fixture\n"
        );
    }

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("System clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "cargo-bundle-assets-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("Create scratch directory");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn packages_shared_apk_entries_once_and_copies_resources() {
        use std::io::{Read, Seek};

        let scratch = Scratch::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets");
        let binary = scratch.0.join("executable");
        fs::write(&binary, record(&root, "nested/example.txt")).expect("Write metadata");
        let resources = scratch.0.join("Resources");
        copy(&binary, &resources);
        assert_eq!(
            fs::read(resources.join("0123456789abcdef/nested/example.txt"))
                .expect("Copied resource"),
            b"bundle fixture\n"
        );

        let apk_path = scratch.0.join("example.apk");
        let mut empty_zip = [0u8; 22];
        empty_zip[..4].copy_from_slice(b"PK\x05\x06");
        fs::write(&apk_path, empty_zip).expect("Create APK");
        let mut apk = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&apk_path)
            .expect("Open APK");
        append_apk(&mut apk, &[binary.clone(), binary.clone(), binary])
            .expect("Append shared assets");
        apk.rewind().expect("Rewind APK");
        let mut archive = zip::ZipArchive::new(apk).expect("Valid APK");
        assert_eq!(archive.len(), 1);
        let mut entry = archive.by_index(0).expect("Shared asset");
        assert_eq!(entry.name(), "assets/0123456789abcdef/nested/example.txt");
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("Read asset");
        assert_eq!(bytes, b"bundle fixture\n");
        let bytes = fs::read(apk_path).expect("Read APK");
        assert_eq!(&bytes[8..10], &[0, 0], "Stored ZIP entry");
    }

    #[test]
    fn rejects_different_payloads_under_the_same_collection_name() {
        let scratch = Scratch::new();
        let first = scratch.0.join("first");
        let second = scratch.0.join("second");
        fs::create_dir_all(&first).expect("First root");
        fs::create_dir_all(&second).expect("Second root");
        fs::write(first.join("file"), b"first").expect("First asset");
        fs::write(second.join("file"), b"second").expect("Second asset");
        let mut bytes = record(&first, "file");
        bytes.extend(record(&second, "file"));
        assert!(collect(&bytes).is_err());
    }

    #[test]
    fn rejects_malformed_postcard_and_trailing_payload_bytes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets");
        let mut bytes = record(&root, "nested/example.txt");
        let length = (bytes.len() - MAGIC.len() - 4 + 1) as u32;
        bytes[MAGIC.len()..MAGIC.len() + 4].copy_from_slice(&length.to_le_bytes());
        bytes.push(0);
        assert!(collect(&bytes).is_err());

        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0xff);
        assert!(collect(&bytes).is_err());
    }

    #[test]
    fn rejects_invalid_names_and_truncated_records() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/assets");
        for name in ["../Cargo.toml", "/etc/passwd", "nested\\example.txt", ""] {
            assert!(collect(&record(&root, name)).is_err());
        }
        let bytes = record(&root, "nested/example.txt");
        for end in MAGIC.len()..bytes.len() {
            assert!(collect(&bytes[..end]).is_err());
        }
        assert!(
            collect(b"ordinary executable")
                .expect("Valid fixture")
                .is_empty()
        );
    }
}
