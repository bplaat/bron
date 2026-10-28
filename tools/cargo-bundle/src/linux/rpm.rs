/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Writes RPM v4 packages: a lead, a signature header with digests, the package header and a
//! gzip compressed cpio payload.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use sha2::Sha256;

use crate::linux::{Package, elf};

const RELEASE: &str = "1";

// Header data types
const INT16: u32 = 3;
const INT32: u32 = 4;
const STRING: u32 = 6;
const BIN: u32 = 7;
const STRING_ARRAY: u32 = 8;
const I18NSTRING: u32 = 9;

// Signature header tags
const SIGTAG_HEADERSIGNATURES: u32 = 62;
const SIGTAG_SHA256: u32 = 273;
const SIGTAG_SIZE: u32 = 1000;
const SIGTAG_PAYLOADSIZE: u32 = 1007;

// Package header tags
const TAG_HEADERIMMUTABLE: u32 = 63;
const TAG_HEADERI18NTABLE: u32 = 100;
const TAG_NAME: u32 = 1000;
const TAG_VERSION: u32 = 1001;
const TAG_RELEASE: u32 = 1002;
const TAG_SUMMARY: u32 = 1004;
const TAG_DESCRIPTION: u32 = 1005;
const TAG_BUILDTIME: u32 = 1006;
const TAG_SIZE: u32 = 1009;
const TAG_LICENSE: u32 = 1014;
const TAG_PACKAGER: u32 = 1015;
const TAG_GROUP: u32 = 1016;
const TAG_URL: u32 = 1020;
const TAG_OS: u32 = 1021;
const TAG_ARCH: u32 = 1022;
const TAG_FILESIZES: u32 = 1028;
const TAG_FILEMODES: u32 = 1030;
const TAG_FILERDEVS: u32 = 1033;
const TAG_FILEMTIMES: u32 = 1034;
const TAG_FILEDIGESTS: u32 = 1035;
const TAG_FILELINKTOS: u32 = 1036;
const TAG_FILEFLAGS: u32 = 1037;
const TAG_FILEUSERNAME: u32 = 1039;
const TAG_FILEGROUPNAME: u32 = 1040;
const TAG_SOURCERPM: u32 = 1044;
const TAG_PROVIDENAME: u32 = 1047;
const TAG_REQUIREFLAGS: u32 = 1048;
const TAG_REQUIRENAME: u32 = 1049;
const TAG_REQUIREVERSION: u32 = 1050;
const TAG_FILEDEVICES: u32 = 1095;
const TAG_FILEINODES: u32 = 1096;
const TAG_FILELANGS: u32 = 1097;
const TAG_PROVIDEFLAGS: u32 = 1112;
const TAG_PROVIDEVERSION: u32 = 1113;
const TAG_DIRINDEXES: u32 = 1116;
const TAG_BASENAMES: u32 = 1117;
const TAG_DIRNAMES: u32 = 1118;
const TAG_PAYLOADFORMAT: u32 = 1124;
const TAG_PAYLOADCOMPRESSOR: u32 = 1125;
const TAG_PAYLOADFLAGS: u32 = 1126;
const TAG_FILEDIGESTALGO: u32 = 5011;
const TAG_ENCODING: u32 = 5062;
const TAG_PAYLOADSHA256: u32 = 5092;
const TAG_PAYLOADSHA256ALGO: u32 = 5093;

const DIGEST_SHA256: u32 = 8;
const SENSE_LESS: u32 = 1 << 1;
const SENSE_EQUAL: u32 = 1 << 3;
const SENSE_FIND_REQUIRES: u32 = 1 << 14;
const SENSE_RPMLIB: u32 = 1 << 24;

/// Features of rpm itself that the package format needs.
const RPMLIB_FEATURES: [(&str, &str); 3] = [
    ("rpmlib(CompressedFileNames)", "3.0.4-1"),
    ("rpmlib(FileDigests)", "4.6.0-1"),
    ("rpmlib(PayloadFilesHavePrefix)", "4.0-1"),
];

/// The file name of the package.
pub(crate) fn file_name(package: &Package) -> String {
    format!(
        "{}-{}-{RELEASE}.{}.rpm",
        package.name, package.version, package.arch.rpm
    )
}

/// The automatic dependencies on shared libraries, like `libgtk-3.so.0()(64bit)`.
pub(crate) fn dependencies(elf: &elf::Elf) -> Vec<String> {
    let suffix = if elf.is_64bit { "()(64bit)" } else { "()" };
    elf.needed
        .iter()
        .map(|needed| format!("{needed}{suffix}"))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut output, byte| {
        _ = write!(output, "{byte:02x}");
        output
    })
}

enum Value {
    Int16(Vec<u16>),
    Int32(Vec<u32>),
    String(String),
    StringArray(Vec<String>),
    I18nString(String),
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

#[derive(Default)]
struct Header(Vec<(u32, Value)>);

impl Header {
    fn add(&mut self, tag: u32, value: impl Into<Value>) {
        self.0.push((tag, value.into()));
    }

    /// Serialize the header with the region tag that marks all entries as immutable.
    fn into_bytes(mut self, region: u32) -> Vec<u8> {
        self.0.sort_by_key(|(tag, _)| *tag);
        let count = self.0.len() as u32 + 1;
        let mut index = Vec::new();
        let mut store = Vec::new();
        for (tag, value) in self.0 {
            let (kind, length, align) = match &value {
                Value::Int16(values) => (INT16, values.len(), 2),
                Value::Int32(values) => (INT32, values.len(), 4),
                Value::String(_) => (STRING, 1, 1),
                Value::I18nString(_) => (I18NSTRING, 1, 1),
                Value::StringArray(values) => (STRING_ARRAY, values.len(), 1),
            };
            store.resize(store.len().next_multiple_of(align), 0);
            index_entry(&mut index, tag, kind, store.len(), length);
            match value {
                Value::Int16(values) => {
                    store.extend(values.iter().flat_map(|value| value.to_be_bytes()));
                }
                Value::Int32(values) => {
                    store.extend(values.iter().flat_map(|value| value.to_be_bytes()));
                }
                Value::String(value) | Value::I18nString(value) => {
                    store.extend_from_slice(value.as_bytes());
                    store.push(0);
                }
                Value::StringArray(values) => {
                    for value in values {
                        store.extend_from_slice(value.as_bytes());
                        store.push(0);
                    }
                }
            }
        }

        // The region trailer points back to the start of the index
        let trailer = store.len();
        store.extend_from_slice(&region.to_be_bytes());
        store.extend_from_slice(&BIN.to_be_bytes());
        store.extend_from_slice(&(-(count as i32 * 16)).to_be_bytes());
        store.extend_from_slice(&16u32.to_be_bytes());

        let mut output = vec![0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0];
        output.extend_from_slice(&count.to_be_bytes());
        output.extend_from_slice(&(store.len() as u32).to_be_bytes());
        index_entry(&mut output, region, BIN, trailer, 16);
        output.extend_from_slice(&index);
        output.extend_from_slice(&store);
        output
    }
}

fn index_entry(index: &mut Vec<u8>, tag: u32, kind: u32, offset: usize, count: usize) {
    for value in [tag, kind, offset as u32, count as u32] {
        index.extend_from_slice(&value.to_be_bytes());
    }
}

/// A packaged file or directory.
struct Entry<'a> {
    path: String,
    mode: u32,
    data: &'a [u8],
}

/// Append a cpio newc entry, its fields are inode, mode, uid, gid, links, mtime, size, the
/// device and rdev numbers, the name size and a zero checksum.
fn cpio_entry(cpio: &mut Vec<u8>, inode: usize, mode: u32, mtime: u32, name: &str, data: &[u8]) {
    let links = if mode & 0o040000 != 0 { 2 } else { 1 };
    cpio.extend_from_slice(b"070701");
    for field in [
        inode,
        mode as usize,
        0,
        0,
        links,
        mtime as usize,
        data.len(),
        0,
        0,
        0,
        0,
        name.len() + 1,
        0,
    ] {
        cpio.extend_from_slice(format!("{field:08x}").as_bytes());
    }
    cpio.extend_from_slice(name.as_bytes());
    cpio.push(0);
    cpio.resize(cpio.len().next_multiple_of(4), 0);
    cpio.extend_from_slice(data);
    cpio.resize(cpio.len().next_multiple_of(4), 0);
}

pub(crate) fn package(package: &Package, requires: &[String]) -> Vec<u8> {
    // The package owns its files and the private data directory, shared directories like
    // /usr/bin belong to the filesystem package
    let mut entries: Vec<_> = package
        .files
        .iter()
        .map(|file| Entry {
            path: file.path.clone(),
            mode: 0o100000 | file.mode,
            data: &file.data,
        })
        .collect();
    if let Some(data_dir) = &package.data_dir {
        let mut directories = BTreeSet::from([data_dir.clone()]);
        for file in &package.files {
            let mut path = file.path.as_str();
            while let Some((parent, _)) = path.rsplit_once('/')
                && parent.starts_with(&format!("{data_dir}/"))
            {
                directories.insert(parent.to_owned());
                path = parent;
            }
        }
        entries.extend(directories.into_iter().map(|path| Entry {
            path,
            mode: 0o040755,
            data: &[],
        }));
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let mut cpio = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        cpio_entry(
            &mut cpio,
            index + 1,
            entry.mode,
            package.mtime,
            &format!(".{}", entry.path),
            entry.data,
        );
    }
    cpio_entry(&mut cpio, 0, 0, 0, "TRAILER!!!", &[]);
    let payload = gzip::compress(&cpio);

    let mut dirnames = Vec::<String>::new();
    let mut dirindexes = Vec::new();
    let mut basenames = Vec::new();
    for entry in &entries {
        let (directory, basename) = entry.path.rsplit_once('/').expect("absolute path");
        let directory = format!("{directory}/");
        let index = dirnames
            .iter()
            .position(|name| *name == directory)
            .unwrap_or_else(|| {
                dirnames.push(directory);
                dirnames.len() - 1
            });
        dirindexes.push(index as u32);
        basenames.push(basename.to_owned());
    }

    let files = entries.len();
    let strings = |value: &str| Value::StringArray(vec![value.to_owned(); files]);
    let mut header = Header::default();
    header.add(
        TAG_HEADERI18NTABLE,
        Value::StringArray(vec!["C".to_owned()]),
    );
    header.add(TAG_NAME, package.name.as_str());
    header.add(TAG_VERSION, package.version.as_str());
    header.add(TAG_RELEASE, RELEASE);
    header.add(TAG_SUMMARY, Value::I18nString(package.summary.clone()));
    header.add(TAG_DESCRIPTION, Value::I18nString(package.summary.clone()));
    header.add(TAG_BUILDTIME, Value::Int32(vec![package.mtime]));
    header.add(
        TAG_SIZE,
        Value::Int32(vec![
            entries.iter().map(|entry| entry.data.len() as u32).sum(),
        ]),
    );
    header.add(TAG_LICENSE, package.license.as_str());
    header.add(TAG_PACKAGER, package.maintainer.as_str());
    header.add(TAG_GROUP, Value::I18nString("Unspecified".to_owned()));
    if let Some(url) = &package.url {
        header.add(TAG_URL, url.as_str());
    }
    header.add(TAG_OS, "linux");
    header.add(TAG_ARCH, package.arch.rpm);
    header.add(
        TAG_FILESIZES,
        Value::Int32(
            entries
                .iter()
                .map(|entry| entry.data.len() as u32)
                .collect(),
        ),
    );
    header.add(
        TAG_FILEMODES,
        Value::Int16(entries.iter().map(|entry| entry.mode as u16).collect()),
    );
    header.add(TAG_FILERDEVS, Value::Int16(vec![0; files]));
    header.add(TAG_FILEMTIMES, Value::Int32(vec![package.mtime; files]));
    header.add(
        TAG_FILEDIGESTS,
        Value::StringArray(
            entries
                .iter()
                .map(|entry| {
                    if entry.mode & 0o040000 != 0 {
                        String::new()
                    } else {
                        hex(&Sha256::digest(entry.data))
                    }
                })
                .collect(),
        ),
    );
    header.add(TAG_FILELINKTOS, strings(""));
    header.add(TAG_FILEFLAGS, Value::Int32(vec![0; files]));
    header.add(TAG_FILEUSERNAME, strings("root"));
    header.add(TAG_FILEGROUPNAME, strings("root"));
    header.add(
        TAG_SOURCERPM,
        format!("{}-{}-{RELEASE}.src.rpm", package.name, package.version).as_str(),
    );
    header.add(
        TAG_PROVIDENAME,
        Value::StringArray(vec![package.name.clone()]),
    );
    header.add(TAG_PROVIDEFLAGS, Value::Int32(vec![SENSE_EQUAL]));
    header.add(
        TAG_PROVIDEVERSION,
        Value::StringArray(vec![format!("{}-{RELEASE}", package.version)]),
    );
    let requires = RPMLIB_FEATURES
        .iter()
        .map(|(name, version)| {
            (
                (*name).to_owned(),
                SENSE_RPMLIB | SENSE_LESS | SENSE_EQUAL,
                (*version).to_owned(),
            )
        })
        .chain(
            requires
                .iter()
                .map(|name| (name.clone(), SENSE_FIND_REQUIRES, String::new())),
        )
        .collect::<Vec<_>>();
    header.add(
        TAG_REQUIRENAME,
        Value::StringArray(requires.iter().map(|(name, ..)| name.clone()).collect()),
    );
    header.add(
        TAG_REQUIREFLAGS,
        Value::Int32(requires.iter().map(|(_, flags, _)| *flags).collect()),
    );
    header.add(
        TAG_REQUIREVERSION,
        Value::StringArray(requires.into_iter().map(|(.., version)| version).collect()),
    );
    header.add(TAG_FILEDEVICES, Value::Int32(vec![1; files]));
    header.add(TAG_FILEINODES, Value::Int32((1..=files as u32).collect()));
    header.add(TAG_FILELANGS, strings(""));
    header.add(TAG_DIRINDEXES, Value::Int32(dirindexes));
    header.add(TAG_BASENAMES, Value::StringArray(basenames));
    header.add(TAG_DIRNAMES, Value::StringArray(dirnames));
    header.add(TAG_PAYLOADFORMAT, "cpio");
    header.add(TAG_PAYLOADCOMPRESSOR, "gzip");
    header.add(TAG_PAYLOADFLAGS, "6");
    header.add(TAG_FILEDIGESTALGO, Value::Int32(vec![DIGEST_SHA256]));
    header.add(TAG_ENCODING, "utf-8");
    header.add(
        TAG_PAYLOADSHA256,
        Value::StringArray(vec![hex(&Sha256::digest(&payload))]),
    );
    header.add(TAG_PAYLOADSHA256ALGO, Value::Int32(vec![DIGEST_SHA256]));
    let header = header.into_bytes(TAG_HEADERIMMUTABLE);

    let mut signature = Header::default();
    signature.add(SIGTAG_SHA256, hex(&Sha256::digest(&header)).as_str());
    signature.add(
        SIGTAG_SIZE,
        Value::Int32(vec![(header.len() + payload.len()) as u32]),
    );
    signature.add(SIGTAG_PAYLOADSIZE, Value::Int32(vec![cpio.len() as u32]));
    let signature = signature.into_bytes(SIGTAG_HEADERSIGNATURES);

    let mut rpm = vec![0u8; 96];
    rpm[..6].copy_from_slice(&[0xed, 0xab, 0xee, 0xdb, 3, 0]);
    let name = format!("{}-{}-{RELEASE}", package.name, package.version);
    let name = &name.as_bytes()[..name.len().min(65)];
    rpm[10..10 + name.len()].copy_from_slice(name);
    // Linux and a header style signature
    rpm[76..80].copy_from_slice(&[0, 1, 0, 5]);
    rpm.extend_from_slice(&signature);
    rpm.resize(rpm.len().next_multiple_of(8), 0);
    rpm.extend_from_slice(&header);
    rpm.extend_from_slice(&payload);
    rpm
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::linux::tests::package as fixture;

    struct Parsed {
        entries: Vec<(u32, u32, usize, usize)>,
        store: Vec<u8>,
        length: usize,
    }

    fn parse_header(bytes: &[u8]) -> Parsed {
        assert_eq!(&bytes[..8], &[0x8e, 0xad, 0xe8, 0x01, 0, 0, 0, 0]);
        let int = |offset: usize| {
            u32::from_be_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
        };
        let count = int(8) as usize;
        let size = int(12) as usize;
        let entries = (0..count)
            .map(|index| {
                let offset = 16 + index * 16;
                (
                    int(offset),
                    int(offset + 4),
                    int(offset + 8) as usize,
                    int(offset + 12) as usize,
                )
            })
            .collect();
        let start = 16 + count * 16;
        Parsed {
            entries,
            store: bytes[start..start + size].to_vec(),
            length: start + size,
        }
    }

    impl Parsed {
        fn strings(&self, tag: u32) -> Vec<String> {
            let &(_, kind, offset, count) = self
                .entries
                .iter()
                .find(|entry| entry.0 == tag)
                .unwrap_or_else(|| panic!("tag {tag}"));
            assert!(matches!(kind, STRING | STRING_ARRAY | I18NSTRING));
            self.store[offset..]
                .split(|&byte| byte == 0)
                .take(count)
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .collect()
        }
    }

    #[test]
    fn writes_signed_digests_regions_and_payload() {
        let rpm = package(
            &fixture(),
            &dependencies(&elf::Elf {
                machine: elf::EM_X86_64,
                is_64bit: true,
                needed: vec!["libc.so.6".to_owned()],
            }),
        );
        assert_eq!(&rpm[..4], &[0xed, 0xab, 0xee, 0xdb]);
        assert_eq!(&rpm[10..26], b"example-1.0.0~be");

        let signature = parse_header(&rpm[96..]);
        let header_start = (96 + signature.length).next_multiple_of(8);
        let header = parse_header(&rpm[header_start..]);
        let header_bytes = &rpm[header_start..header_start + header.length];
        let payload = &rpm[header_start + header.length..];

        // Both headers start with an immutable region whose trailer points back to the index
        for (parsed, region) in [
            (&signature, SIGTAG_HEADERSIGNATURES),
            (&header, TAG_HEADERIMMUTABLE),
        ] {
            let (tag, kind, offset, count) = parsed.entries[0];
            assert_eq!((tag, kind, count), (region, BIN, 16));
            let trailer = &parsed.store[offset..offset + 16];
            assert_eq!(&trailer[..4], &region.to_be_bytes());
            assert_eq!(
                i32::from_be_bytes(trailer[8..12].try_into().expect("four bytes")),
                -(parsed.entries.len() as i32 * 16)
            );
        }
        assert_eq!(
            signature.strings(SIGTAG_SHA256),
            [hex(&Sha256::digest(header_bytes))]
        );
        assert_eq!(
            header.strings(TAG_PAYLOADSHA256),
            [hex(&Sha256::digest(payload))]
        );

        assert_eq!(header.strings(TAG_NAME), ["example"]);
        assert_eq!(header.strings(TAG_VERSION), ["1.0.0~beta"]);
        assert_eq!(header.strings(TAG_ARCH), ["x86_64"]);
        assert_eq!(
            header.strings(TAG_DIRNAMES),
            [
                "/usr/bin/",
                "/usr/share/",
                "/usr/share/example/",
                &format!("/usr/share/example/{}/", "a".repeat(60)),
                &format!("/usr/share/example/{}/{}/", "a".repeat(60), "b".repeat(60)),
            ]
        );
        assert_eq!(
            header.strings(TAG_BASENAMES),
            [
                "example",
                "example",
                &"a".repeat(60),
                &"b".repeat(60),
                "file"
            ]
        );
        assert_eq!(header.strings(TAG_REQUIRENAME)[3], "libc.so.6()(64bit)");

        let cpio = gzip::decompress(payload).expect("gzip payload");
        assert!(cpio.starts_with(b"070701"));
        assert!(
            cpio.windows(17)
                .any(|window| window == b"./usr/bin/example")
        );
        assert!(cpio.windows(11).any(|window| window == b"TRAILER!!!\0"));
    }
}
