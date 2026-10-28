/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Writes Debian binary packages: an ar archive with gzip compressed control and data tarballs.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::process::Command;

use crate::linux::{Package, elf};

/// Find the Debian packages that contain the needed shared libraries with ldconfig and dpkg.
pub(crate) fn dependencies(elf: &elf::Elf) -> Vec<String> {
    if elf.needed.is_empty() {
        return Vec::new();
    }
    let Ok(output) = Command::new("/sbin/ldconfig").arg("-p").output() else {
        eprintln!("Warning: ldconfig is missing, the .deb has no dependencies");
        return Vec::new();
    };
    let mut libraries = BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((name, path)) = line.split_once(" => ") else {
            continue;
        };
        let name = name.split_whitespace().next().unwrap_or_default();
        if elf.needed.iter().any(|needed| needed == name)
            && !libraries.contains_key(name)
            && same_machine(elf, Path::new(path))
            && let Ok(path) = fs::canonicalize(path)
        {
            libraries.insert(name.to_owned(), path);
        }
    }
    for needed in &elf.needed {
        if !libraries.contains_key(needed) {
            eprintln!("Warning: {needed} is not installed, the .deb doesn't depend on it");
        }
    }
    if libraries.is_empty() {
        return Vec::new();
    }

    let Ok(output) = Command::new("dpkg-query")
        .arg("-S")
        .args(libraries.values())
        .output()
    else {
        eprintln!("Warning: dpkg-query is missing, the .deb has no dependencies");
        return Vec::new();
    };
    // Lines look like `libgtk-3-0t64:amd64: /usr/lib/x86_64-linux-gnu/libgtk-3.so.0.2400.41`
    let mut packages = BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some((owners, _)) = line.split_once(": /") {
            for owner in owners.split(", ") {
                packages.insert(owner.split(':').next().unwrap_or(owner).to_owned());
            }
        }
    }
    packages.into_iter().collect()
}

fn same_machine(elf: &elf::Elf, path: &Path) -> bool {
    let mut header = [0; 20];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .is_ok_and(|()| elf::identify(&header) == Some((elf.machine, elf.is_64bit)))
}

pub(crate) fn package(package: &Package, depends: &[String]) -> Vec<u8> {
    let installed_size = package
        .files
        .iter()
        .map(|file| file.data.len() as u64)
        .sum::<u64>()
        .div_ceil(1024);
    let mut control = format!(
        "Package: {}\nVersion: {}\nArchitecture: {}\nMaintainer: {}\nInstalled-Size: {installed_size}\n",
        package.name, package.version, package.arch.deb, package.maintainer
    );
    if !depends.is_empty() {
        _ = writeln!(control, "Depends: {}", depends.join(", "));
    }
    _ = writeln!(control, "Priority: optional");
    if let Some(url) = &package.url {
        _ = writeln!(control, "Homepage: {url}");
    }
    _ = writeln!(control, "Description: {}", package.summary);

    let mut control_tar = Tar::new(package.mtime);
    control_tar.directory("./");
    control_tar.file("./control", 0o644, control.as_bytes());

    let mut data_tar = Tar::new(package.mtime);
    let mut directories = BTreeSet::from(["./".to_owned()]);
    for file in &package.files {
        let mut path = file.path.as_str();
        while let Some((parent, _)) = path.rsplit_once('/')
            && !parent.is_empty()
        {
            directories.insert(format!(".{parent}/"));
            path = parent;
        }
    }
    for directory in &directories {
        data_tar.directory(directory);
    }
    for file in &package.files {
        data_tar.file(&format!(".{}", file.path), file.mode, &file.data);
    }

    let mut deb = b"!<arch>\n".to_vec();
    for (name, data) in [
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar.gz", gzip::compress(&control_tar.finish())),
        ("data.tar.gz", gzip::compress(&data_tar.finish())),
    ] {
        deb.extend_from_slice(
            format!(
                "{name:<16}{:<12}0     0     100644  {:<10}`\n",
                package.mtime,
                data.len()
            )
            .as_bytes(),
        );
        deb.extend_from_slice(&data);
        if data.len() % 2 == 1 {
            deb.push(b'\n');
        }
    }
    deb
}

/// A ustar archive writer for files owned by root.
struct Tar {
    data: Vec<u8>,
    mtime: u32,
}

impl Tar {
    const fn new(mtime: u32) -> Self {
        Self {
            data: Vec::new(),
            mtime,
        }
    }

    fn directory(&mut self, path: &str) {
        self.header(path, 0o755, 0, b'5');
    }

    fn file(&mut self, path: &str, mode: u32, data: &[u8]) {
        self.header(path, mode, data.len(), b'0');
        self.data.extend_from_slice(data);
        self.data.resize(self.data.len().next_multiple_of(512), 0);
    }

    fn header(&mut self, path: &str, mode: u32, size: usize, kind: u8) {
        // Long paths are split over the prefix and name fields at a slash before the name
        let (prefix, name) = if path.len() <= 100 {
            ("", path)
        } else {
            let split = path.as_bytes()[..(path.len() - 1).min(156)]
                .iter()
                .rposition(|&byte| byte == b'/')
                .filter(|split| path.len() - split - 1 <= 100)
                .unwrap_or_else(|| panic!("Path is too long for a tar archive: {path}"));
            (&path[..split], &path[split + 1..])
        };
        let mut header = [0u8; 512];
        let mut field = |offset: usize, value: &[u8]| {
            header[offset..offset + value.len()].copy_from_slice(value);
        };
        field(0, name.as_bytes());
        field(100, format!("{mode:07o}").as_bytes());
        field(108, b"0000000");
        field(116, b"0000000");
        field(124, format!("{size:011o}").as_bytes());
        field(136, format!("{:011o}", self.mtime).as_bytes());
        field(148, b"        ");
        field(156, &[kind]);
        field(257, b"ustar\x0000");
        field(265, b"root");
        field(297, b"root");
        field(345, prefix.as_bytes());
        let checksum = header.iter().map(|&byte| u32::from(byte)).sum::<u32>();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        self.data.extend_from_slice(&header);
    }

    fn finish(mut self) -> Vec<u8> {
        self.data.resize(self.data.len() + 1024, 0);
        self.data
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::tests::package as fixture;

    fn members(deb: &[u8]) -> Vec<(String, Vec<u8>)> {
        assert_eq!(&deb[..8], b"!<arch>\n");
        let mut members = Vec::new();
        let mut offset = 8;
        while offset < deb.len() {
            let header = &deb[offset..offset + 60];
            assert_eq!(&header[58..], b"`\n");
            let name = String::from_utf8_lossy(&header[..16]).trim().to_owned();
            let size: usize = String::from_utf8_lossy(&header[48..58])
                .trim()
                .parse()
                .expect("member size");
            members.push((name, deb[offset + 60..offset + 60 + size].to_vec()));
            offset += 60 + size.next_multiple_of(2);
        }
        members
    }

    fn entries(tar: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut entries = Vec::new();
        let mut offset = 0;
        while tar[offset] != 0 {
            let header = &tar[offset..offset + 512];
            let checksum = header
                .iter()
                .enumerate()
                .map(|(index, &byte)| {
                    u32::from(if (148..156).contains(&index) {
                        b' '
                    } else {
                        byte
                    })
                })
                .sum::<u32>();
            assert_eq!(format!("{checksum:06o}\0 ").as_bytes(), &header[148..156]);
            let text = |range: std::ops::Range<usize>| {
                String::from_utf8_lossy(&header[range])
                    .trim_end_matches('\0')
                    .to_owned()
            };
            let prefix = text(345..500);
            let name = text(0..100);
            let size = usize::from_str_radix(&text(124..135), 8).expect("size");
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            entries.push((path, tar[offset + 512..offset + 512 + size].to_vec()));
            offset += 512 + size.next_multiple_of(512);
        }
        entries
    }

    #[test]
    fn writes_control_and_data_archives() {
        let deb = package(&fixture(), &["libc6".to_owned(), "libgtk-3-0".to_owned()]);
        let members = members(&deb);
        let names: Vec<_> = members.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["debian-binary", "control.tar.gz", "data.tar.gz"]);
        assert_eq!(members[0].1, b"2.0\n");

        let control = entries(&gzip::decompress(&members[1].1).expect("gzip"));
        assert_eq!(control[1].0, "./control");
        assert_eq!(
            String::from_utf8_lossy(&control[1].1),
            "Package: example\nVersion: 1.0.0~beta\nArchitecture: amd64\n\
             Maintainer: Example Author <author@example.com>\nInstalled-Size: 1\n\
             Depends: libc6, libgtk-3-0\nPriority: optional\nHomepage: https://example.com\n\
             Description: An example\n"
        );

        let data = entries(&gzip::decompress(&members[2].1).expect("gzip"));
        let paths: Vec<_> = data.iter().map(|(path, _)| path.as_str()).collect();
        let nested = format!("./usr/share/example/{}/", "a".repeat(60));
        let long = format!("{nested}{}/", "b".repeat(60));
        assert_eq!(
            paths,
            [
                "./",
                "./usr/",
                "./usr/bin/",
                "./usr/share/",
                "./usr/share/example/",
                &nested,
                &long,
                "./usr/bin/example",
                &format!("{long}file"),
            ]
        );
        assert_eq!(data[7].1, b"\x7fELF");
        assert_eq!(data[8].1, b"asset");
    }
}
