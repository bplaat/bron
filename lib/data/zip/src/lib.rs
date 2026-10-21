/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [zip](https://crates.io/crates/zip) crate

use std::fmt;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::time::SystemTime;

use crc::{CRC_32_ISO_HDLC, Crc};

use crate::metadata::{dos_timestamp, extra_timestamp};

mod metadata;
mod write;
pub use write::{append_file, append_stored_file};

const MAX_UNCOMPRESSED_SIZE: usize = 128 * 1024 * 1024;
const MAX_COMPRESSED_SIZE: usize = 128 * 1024 * 1024;
const MAX_TOTAL_UNCOMPRESSED_SIZE: usize = 256 * 1024 * 1024;
const MAX_ARCHIVE_SIZE: usize = 512 * 1024 * 1024;

static CRC32: Crc<u32> = Crc::<u32>::new(&CRC_32_ISO_HDLC);

bitflags::bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct GeneralPurposeFlags: u16 {
        const ENCRYPTED = 1 << 0;
        const DATA_DESCRIPTOR = 1 << 3;
        // Preserve all bits when comparing central and local headers.
        const _ = !0;
    }
}

// MARK: Entry metadata
struct CdEntry {
    name: String,
    name_bytes: Vec<u8>,
    flags: GeneralPurposeFlags,
    compression: u16,
    crc32: u32,
    compressed_size: u32,
    uncompressed_size: u32,
    local_header_offset: u32,
    boundary: u64,
    unix_mode: Option<u32>,
    is_dir: bool,
    modified: Option<SystemTime>,
    dos_date: u16,
    dos_time: u16,
}

// MARK: ZipFile
/// A file entry within a ZIP archive.
pub struct ZipFile {
    name: String,
    data: Cursor<Vec<u8>>,
    unix_mode: Option<u32>,
    is_dir: bool,
    modified: Option<SystemTime>,
}

impl ZipFile {
    /// Returns the name of the file.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the Unix mode, including file type bits, when supplied by a Unix creator.
    pub const fn unix_mode(&self) -> Option<u32> {
        self.unix_mode
    }

    /// Returns whether this entry represents a directory.
    pub const fn is_dir(&self) -> bool {
        self.is_dir
    }

    /// Returns whether this entry represents a Unix symbolic link.
    pub fn is_symlink(&self) -> bool {
        self.unix_mode
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
    }

    /// Returns whether this entry represents a regular file.
    pub fn is_file(&self) -> bool {
        !self.is_dir
            && self
                .unix_mode
                .is_none_or(|mode| matches!(mode & 0o170000, 0 | 0o100000))
    }

    /// Returns modification time, preferring Unix/NTFS extra fields over local DOS time.
    pub const fn last_modified(&self) -> Option<SystemTime> {
        self.modified
    }
}

impl Read for ZipFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.data.read(buf)
    }
}

// MARK: ZipArchive
/// A ZIP archive.
pub struct ZipArchive<R> {
    reader: R,
    entries: Vec<CdEntry>,
    extracted_size: usize,
    end: EndRecord,
}

fn read_u16_le(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().expect("slice error"))
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("slice error"))
}

impl<R: Read + Seek> ZipArchive<R> {
    /// Open a ZIP archive from a reader.
    pub fn new(mut reader: R) -> Result<Self, ZipError> {
        // Read all bytes for parsing
        let bytes = read_limited(&mut reader, MAX_ARCHIVE_SIZE)?;

        let end = end_record(&bytes)?;
        let cd_count = end.count;
        let cd_offset = end.directory_offset;
        let cd_end = end.offset;

        if cd_count > (cd_end - cd_offset) / 46 {
            return Err(ZipError::InvalidZip(
                "entry count exceeds central directory size",
            ));
        }

        // Parse central directory entries
        let mut entries = Vec::with_capacity(cd_count);
        let mut total_uncompressed_size = 0usize;
        let mut pos = cd_offset;
        for _ in 0..cd_count {
            if bytes.get(pos..pos + 4) != Some(&[0x50, 0x4b, 0x01, 0x02][..]) {
                return Err(ZipError::InvalidZip("invalid central directory signature"));
            }
            // Minimum central directory entry is 46 bytes; validate before reading fields
            if pos + 46 > cd_end {
                return Err(ZipError::InvalidZip("central directory entry truncated"));
            }
            let flags = GeneralPurposeFlags::from_bits_retain(read_u16_le(&bytes, pos + 8));
            if flags.bits() & 0x2061 != 0 {
                return Err(ZipError::InvalidZip("encrypted entries are unsupported"));
            }
            let compression = read_u16_le(&bytes, pos + 10);
            let crc32 = read_u32_le(&bytes, pos + 16);
            let compressed_size = read_u32_le(&bytes, pos + 20);
            let uncompressed_size = read_u32_le(&bytes, pos + 24);
            let name_len = read_u16_le(&bytes, pos + 28) as usize;
            let extra_len = read_u16_le(&bytes, pos + 30) as usize;
            let comment_len = read_u16_le(&bytes, pos + 32) as usize;
            let local_header_offset = read_u32_le(&bytes, pos + 42);
            if [compressed_size, uncompressed_size, local_header_offset].contains(&u32::MAX) {
                return Err(ZipError::InvalidZip("ZIP64 is unsupported"));
            }
            if read_u16_le(&bytes, pos + 34) != 0 {
                return Err(ZipError::InvalidZip("multi-disk archives are unsupported"));
            }
            let external = read_u32_le(&bytes, pos + 38);
            let mut unix_mode =
                if matches!(bytes[pos + 5], 3 | 19) && !matches!(external >> 16, 0 | 0xffff) {
                    Some(external >> 16)
                } else {
                    None
                };
            let dos_time = read_u16_le(&bytes, pos + 12);
            let dos_date = read_u16_le(&bytes, pos + 14);
            let name_end = pos
                .checked_add(46)
                .and_then(|v| v.checked_add(name_len))
                .ok_or(ZipError::InvalidZip("name length overflow"))?;
            if name_end > cd_end {
                return Err(ZipError::InvalidZip("entry name extends beyond archive"));
            }
            let name_bytes = bytes[pos + 46..name_end].to_vec();
            let name = if flags.bits() & 0x800 != 0 {
                String::from_utf8(name_bytes.clone())
                    .map_err(|_| ZipError::InvalidZip("invalid UTF-8 name"))?
            } else {
                String::from_utf8_lossy(&name_bytes).into_owned()
            };
            if name.is_empty() || name.contains('\0') {
                return Err(ZipError::InvalidZip("empty or NUL-containing name"));
            }
            let is_dir = name.ends_with('/')
                || external & 0x10 != 0
                || unix_mode.is_some_and(|mode| mode & 0o170000 == 0o040000);
            if is_dir && let Some(mode) = unix_mode {
                match mode & 0o170000 {
                    0 | 0o040000 => {}
                    // Some JAR writers put regular-file attributes on directory entries.
                    0o100000 => unix_mode = None,
                    _ => return Err(ZipError::InvalidZip("directory name and Unix type differ")),
                }
            }
            if is_dir && uncompressed_size != 0 {
                return Err(ZipError::InvalidZip("directory contains file data"));
            }
            // Reject path traversal sequences in entry names (both / and \ separators)
            for component in name.split(['/', '\\']) {
                if component == ".." {
                    return Err(ZipError::InvalidZip(
                        "entry name contains path traversal sequence",
                    ));
                }
            }
            if uncompressed_size as usize > MAX_UNCOMPRESSED_SIZE {
                return Err(ZipError::InvalidZip(
                    "uncompressed entry size exceeds limit",
                ));
            }
            if compressed_size as usize > MAX_COMPRESSED_SIZE {
                return Err(ZipError::InvalidZip("compressed entry size exceeds limit"));
            }
            total_uncompressed_size = total_uncompressed_size
                .checked_add(uncompressed_size as usize)
                .ok_or(ZipError::InvalidZip("total uncompressed size overflow"))?;
            if total_uncompressed_size > MAX_TOTAL_UNCOMPRESSED_SIZE {
                return Err(ZipError::InvalidZip(
                    "total uncompressed archive size exceeds limit",
                ));
            }
            let entry_end = name_end
                .checked_add(extra_len)
                .and_then(|v| v.checked_add(comment_len))
                .ok_or(ZipError::InvalidZip("entry length overflow"))?;
            if entry_end > cd_end {
                return Err(ZipError::InvalidZip(
                    "entry metadata extends beyond directory",
                ));
            }
            let modified = extra_timestamp(&bytes[name_end..name_end + extra_len])?;
            pos = entry_end;
            entries.push(CdEntry {
                name,
                name_bytes,
                flags,
                compression,
                crc32,
                compressed_size,
                uncompressed_size,
                local_header_offset,
                boundary: cd_offset as u64,
                unix_mode,
                is_dir,
                modified,
                dos_date,
                dos_time,
            });
        }

        if pos != cd_end {
            return Err(ZipError::InvalidZip(
                "central directory size or entry count differs",
            ));
        }
        let mut offsets = (0..entries.len()).collect::<Vec<_>>();
        offsets.sort_unstable_by_key(|&index| entries[index].local_header_offset);
        for (position, &index) in offsets.iter().enumerate() {
            let boundary = offsets.get(position + 1).map_or(cd_offset as u64, |&next| {
                u64::from(entries[next].local_header_offset)
            });
            if u64::from(entries[index].local_header_offset) + 30 > boundary {
                return Err(ZipError::InvalidZip(
                    "overlapping or out-of-bounds local headers",
                ));
            }
            entries[index].boundary = boundary;
        }

        // Seek back to start so the reader is in a consistent state
        reader.seek(SeekFrom::Start(0))?;

        Ok(ZipArchive {
            reader,
            entries,
            extracted_size: 0,
            end,
        })
    }

    /// Returns the number of entries in the archive.
    pub const fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the archive has no entries.
    pub const fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the entry at the given index.
    pub fn by_index(&mut self, index: usize) -> Result<ZipFile, ZipError> {
        let entry = self.entries.get(index).ok_or(ZipError::FileNotFound)?;
        let name = entry.name.clone();
        let name_bytes = entry.name_bytes.clone();
        let flags = entry.flags;
        let compression = entry.compression;
        let expected_crc32 = entry.crc32;
        let compressed_size = entry.compressed_size as usize;
        let uncompressed_size = entry.uncompressed_size as usize;
        let local_header_offset = entry.local_header_offset as u64;
        let boundary = entry.boundary;
        let unix_mode = entry.unix_mode;
        let is_dir = entry.is_dir;
        let central_modified = entry.modified;
        let dos_date = entry.dos_date;
        let dos_time = entry.dos_time;
        if self.extracted_size.saturating_add(uncompressed_size) > MAX_TOTAL_UNCOMPRESSED_SIZE {
            return Err(ZipError::InvalidZip(
                "total extracted archive size exceeds limit",
            ));
        }

        // Read and validate the local file header before trusting its data offset.
        self.reader.seek(SeekFrom::Start(local_header_offset))?;
        let mut local_header = [0; 30];
        self.reader.read_exact(&mut local_header)?;
        if local_header[..4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(ZipError::InvalidZip("invalid local file header signature"));
        }
        let local_flags = GeneralPurposeFlags::from_bits_retain(read_u16_le(&local_header, 6));
        let local_compression = read_u16_le(&local_header, 8);
        let local_crc32 = read_u32_le(&local_header, 14);
        let local_compressed_size = read_u32_le(&local_header, 18);
        let local_uncompressed_size = read_u32_le(&local_header, 22);
        let local_name_len = read_u16_le(&local_header, 26) as usize;
        let local_extra_len = read_u16_le(&local_header, 28) as u64;
        if local_flags != flags || local_compression != compression {
            return Err(ZipError::InvalidZip(
                "local and central directory metadata differ",
            ));
        }
        if !flags.contains(GeneralPurposeFlags::DATA_DESCRIPTOR)
            && (local_crc32 != expected_crc32
                || local_compressed_size as usize != compressed_size
                || local_uncompressed_size as usize != uncompressed_size)
        {
            return Err(ZipError::InvalidZip(
                "local and central directory sizes or checksum differ",
            ));
        }
        let mut local_name = vec![0; local_name_len];
        self.reader.read_exact(&mut local_name)?;
        if local_name != name_bytes {
            return Err(ZipError::InvalidZip(
                "local and central directory names differ",
            ));
        }
        let data_end = local_header_offset
            + 30
            + local_name_len as u64
            + local_extra_len
            + compressed_size as u64;
        if data_end > boundary {
            return Err(ZipError::InvalidZip("entry data overlaps another record"));
        }
        let mut extra = vec![0; local_extra_len as usize];
        self.reader.read_exact(&mut extra)?;
        let modified = central_modified
            .or(extra_timestamp(&extra)?)
            .or_else(|| dos_timestamp(dos_date, dos_time));

        // Read compressed data
        let mut compressed = vec![0u8; compressed_size];
        self.reader.read_exact(&mut compressed)?;

        if flags.contains(GeneralPurposeFlags::DATA_DESCRIPTOR) {
            if data_end + 12 > boundary {
                return Err(ZipError::InvalidZip("truncated data descriptor"));
            }
            let mut descriptor = [0; 12];
            self.reader.read_exact(&mut descriptor)?;
            let matches = |descriptor: &[u8]| {
                read_u32_le(descriptor, 0) == expected_crc32
                    && read_u32_le(descriptor, 4) as usize == compressed_size
                    && read_u32_le(descriptor, 8) as usize == uncompressed_size
            };
            if !matches(&descriptor) && read_u32_le(&descriptor, 0) == 0x08074b50 {
                if data_end + 16 > boundary {
                    return Err(ZipError::InvalidZip("truncated data descriptor"));
                }
                descriptor.copy_within(4..12, 0);
                self.reader.read_exact(&mut descriptor[8..12])?;
            }
            if !matches(&descriptor) {
                return Err(ZipError::InvalidZip("invalid data descriptor"));
            }
        }

        let data = match compression {
            0 => compressed, // stored
            8 => {
                if uncompressed_size > MAX_UNCOMPRESSED_SIZE {
                    return Err(ZipError::InvalidZip(
                        "uncompressed entry size exceeds limit",
                    ));
                }
                deflate::decompress(&compressed, uncompressed_size)
                    .map_err(|_| ZipError::DecompressError)?
            }
            m => return Err(ZipError::UnsupportedCompressionMethod(m)),
        };
        if data.len() != uncompressed_size {
            return Err(ZipError::InvalidZip(
                "uncompressed data size does not match directory",
            ));
        }
        if CRC32.checksum(&data) != expected_crc32 {
            return Err(ZipError::InvalidZip(
                "entry checksum does not match directory",
            ));
        }
        self.extracted_size += data.len();

        Ok(ZipFile {
            name,
            data: Cursor::new(data),
            unix_mode,
            is_dir,
            modified,
        })
    }
}

#[derive(Clone, Copy)]
struct EndRecord {
    offset: usize,
    directory_offset: usize,
    count: usize,
}

fn end_record(bytes: &[u8]) -> Result<EndRecord, ZipError> {
    let offset = (bytes.len().saturating_sub(65_557)..bytes.len().saturating_sub(21))
        .rev()
        .find(|&index| {
            bytes[index..index + 4] == *b"PK\x05\x06"
                && index + 22 + usize::from(read_u16_le(bytes, index + 20)) == bytes.len()
                && u64::from(read_u32_le(bytes, index + 16))
                    + u64::from(read_u32_le(bytes, index + 12))
                    == index as u64
        })
        .ok_or(ZipError::InvalidZip(
            "invalid or missing end of central directory",
        ))?;
    if read_u32_le(bytes, offset + 4) != 0
        || read_u16_le(bytes, offset + 8) != read_u16_le(bytes, offset + 10)
    {
        return Err(ZipError::InvalidZip("multi-disk archives are unsupported"));
    }
    let count = read_u16_le(bytes, offset + 10);
    if count == u16::MAX {
        return Err(ZipError::InvalidZip("ZIP64 is unsupported"));
    }
    Ok(EndRecord {
        offset,
        directory_offset: read_u32_le(bytes, offset + 16) as usize,
        count: usize::from(count),
    })
}

fn read_limited(reader: &mut impl Read, limit: usize) -> Result<Vec<u8>, ZipError> {
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(ZipError::InvalidZip("archive size exceeds limit"));
    }
    Ok(bytes)
}

// MARK: Error
/// ZIP error type.
#[derive(Debug)]
pub enum ZipError {
    /// An I/O error occurred.
    Io(io::Error),
    /// The ZIP file is invalid.
    InvalidZip(&'static str),
    /// The file number is out of range.
    FileNotFound,
    /// The compression method is unsupported.
    UnsupportedCompressionMethod(u16),
    /// Decompression failed.
    DecompressError,
}

impl fmt::Display for ZipError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZipError::Io(e) => write!(f, "I/O error: {e}"),
            ZipError::InvalidZip(msg) => write!(f, "Invalid ZIP: {msg}"),
            ZipError::FileNotFound => write!(f, "File not found"),
            ZipError::UnsupportedCompressionMethod(m) => {
                write!(f, "Unsupported compression method: {m}")
            }
            ZipError::DecompressError => write!(f, "Decompression failed"),
        }
    }
}

impl std::error::Error for ZipError {}

impl From<io::Error> for ZipError {
    fn from(e: io::Error) -> Self {
        ZipError::Io(e)
    }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    // zip -0 stored.zip hello.txt (hello.txt = "hello world", stored)
    #[rustfmt::skip]
    const STORED_ZIP: &[u8] = &[0x50, 0x4b, 0x03, 0x04, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x85, 0x11, 0x4a, 0x0d, 0x0b, 0x00, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x00, 0x09, 0x00, 0x1c, 0x00, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x09, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x77, 0x6f, 0x72, 0x6c, 0x64, 0x50, 0x4b, 0x01, 0x02, 0x1e, 0x03, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x85, 0x11, 0x4a, 0x0d, 0x0b, 0x00, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x00, 0x09, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa4, 0x81, 0x00, 0x00, 0x00, 0x00, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x05, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x05, 0x06, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x4f, 0x00, 0x00, 0x00, 0x4e, 0x00, 0x00, 0x00, 0x00, 0x00];

    // zip -9 deflate.zip lorem.txt (lorem.txt = "the quick brown fox..." x50, deflated 97%)
    #[rustfmt::skip]
    const DEFLATE_ZIP: &[u8] = &[0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x02, 0x00, 0x08, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x68, 0xac, 0x28, 0x64, 0x3f, 0x00, 0x00, 0x00, 0x98, 0x08, 0x00, 0x00, 0x09, 0x00, 0x1c, 0x00, 0x6c, 0x6f, 0x72, 0x65, 0x6d, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x09, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0xed, 0xca, 0x41, 0x02, 0x40, 0x20, 0x14, 0x04, 0xd0, 0x7d, 0xa7, 0x98, 0xab, 0x85, 0x28, 0xd2, 0x27, 0x3f, 0xca, 0xe9, 0x75, 0x09, 0xbb, 0x59, 0xbf, 0xa7, 0xde, 0xe1, 0x2c, 0x61, 0xdc, 0x30, 0x64, 0x79, 0x12, 0x66, 0xa9, 0x58, 0xcb, 0x7e, 0x5c, 0x90, 0xdb, 0x65, 0x68, 0xe7, 0x68, 0xdf, 0x86, 0x49, 0x16, 0xa3, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0x3f, 0xde, 0x0f, 0x50, 0x4b, 0x01, 0x02, 0x1e, 0x03, 0x14, 0x00, 0x02, 0x00, 0x08, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x68, 0xac, 0x28, 0x64, 0x3f, 0x00, 0x00, 0x00, 0x98, 0x08, 0x00, 0x00, 0x09, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xa4, 0x81, 0x00, 0x00, 0x00, 0x00, 0x6c, 0x6f, 0x72, 0x65, 0x6d, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x05, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x05, 0x06, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x4f, 0x00, 0x00, 0x00, 0x82, 0x00, 0x00, 0x00, 0x00, 0x00];

    // zip -0 multi.zip hello.txt && zip -9 multi.zip lorem.txt (mixed stored + deflate)
    #[rustfmt::skip]
    const MULTI_ZIP: &[u8] = &[0x50, 0x4b, 0x03, 0x04, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x85, 0x11, 0x4a, 0x0d, 0x0b, 0x00, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x00, 0x09, 0x00, 0x1c, 0x00, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x09, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x77, 0x6f, 0x72, 0x6c, 0x64, 0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x02, 0x00, 0x08, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x68, 0xac, 0x28, 0x64, 0x3f, 0x00, 0x00, 0x00, 0x98, 0x08, 0x00, 0x00, 0x09, 0x00, 0x1c, 0x00, 0x6c, 0x6f, 0x72, 0x65, 0x6d, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x09, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0xed, 0xca, 0x41, 0x02, 0x40, 0x20, 0x14, 0x04, 0xd0, 0x7d, 0xa7, 0x98, 0xab, 0x85, 0x28, 0xd2, 0x27, 0x3f, 0xca, 0xe9, 0x75, 0x09, 0xbb, 0x59, 0xbf, 0xa7, 0xde, 0xe1, 0x2c, 0x61, 0xdc, 0x30, 0x64, 0x79, 0x12, 0x66, 0xa9, 0x58, 0xcb, 0x7e, 0x5c, 0x90, 0xdb, 0x65, 0x68, 0xe7, 0x68, 0xdf, 0x86, 0x49, 0x16, 0xa3, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0xbc, 0x3f, 0xde, 0x0f, 0x50, 0x4b, 0x01, 0x02, 0x1e, 0x03, 0x0a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x85, 0x11, 0x4a, 0x0d, 0x0b, 0x00, 0x00, 0x00, 0x0b, 0x00, 0x00, 0x00, 0x09, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xa4, 0x81, 0x00, 0x00, 0x00, 0x00, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x05, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x01, 0x02, 0x1e, 0x03, 0x14, 0x00, 0x02, 0x00, 0x08, 0x00, 0x04, 0x82, 0x61, 0x5c, 0x68, 0xac, 0x28, 0x64, 0x3f, 0x00, 0x00, 0x00, 0x98, 0x08, 0x00, 0x00, 0x09, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xa4, 0x81, 0x4e, 0x00, 0x00, 0x00, 0x6c, 0x6f, 0x72, 0x65, 0x6d, 0x2e, 0x74, 0x78, 0x74, 0x55, 0x54, 0x05, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x05, 0x06, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x9e, 0x00, 0x00, 0x00, 0xd0, 0x00, 0x00, 0x00, 0x00, 0x00];

    // zip -9 keep.zip Keep/note.json Keep/empty.json (Keep/ subfolder, mixed compression)
    #[rustfmt::skip]
    const KEEP_ZIP: &[u8] = &[0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x02, 0x00, 0x08, 0x00, 0x04, 0x82, 0x61, 0x5c, 0xf3, 0xf9, 0xa3, 0x4c, 0x2d, 0x00, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x1c, 0x00, 0x4b, 0x65, 0x65, 0x70, 0x2f, 0x6e, 0x6f, 0x74, 0x65, 0x2e, 0x6a, 0x73, 0x6f, 0x6e, 0x55, 0x54, 0x09, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0xab, 0x56, 0x2a, 0xc9, 0x2c, 0xc9, 0x49, 0x55, 0xb2, 0x52, 0x0a, 0x49, 0x2d, 0x2e, 0x51, 0xf0, 0xcb, 0x2f, 0x49, 0x55, 0xd2, 0x51, 0x2a, 0x49, 0xad, 0x28, 0x71, 0xce, 0xcf, 0x2b, 0x49, 0xcd, 0x2b, 0x01, 0xca, 0x24, 0xe5, 0xa7, 0x54, 0x2a, 0x80, 0x84, 0x94, 0x6a, 0x01, 0x50, 0x4b, 0x03, 0x04, 0x0a, 0x00, 0x02, 0x00, 0x00, 0x00, 0x04, 0x82, 0x61, 0x5c, 0xd8, 0x98, 0x9a, 0x64, 0x1d, 0x00, 0x00, 0x00, 0x1d, 0x00, 0x00, 0x00, 0x0f, 0x00, 0x1c, 0x00, 0x4b, 0x65, 0x65, 0x70, 0x2f, 0x65, 0x6d, 0x70, 0x74, 0x79, 0x2e, 0x6a, 0x73, 0x6f, 0x6e, 0x55, 0x54, 0x09, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x7b, 0x22, 0x74, 0x69, 0x74, 0x6c, 0x65, 0x22, 0x3a, 0x22, 0x22, 0x2c, 0x22, 0x74, 0x65, 0x78, 0x74, 0x43, 0x6f, 0x6e, 0x74, 0x65, 0x6e, 0x74, 0x22, 0x3a, 0x22, 0x22, 0x7d, 0x50, 0x4b, 0x01, 0x02, 0x1e, 0x03, 0x14, 0x00, 0x02, 0x00, 0x08, 0x00, 0x04, 0x82, 0x61, 0x5c, 0xf3, 0xf9, 0xa3, 0x4c, 0x2d, 0x00, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x00, 0x0e, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xa4, 0x81, 0x00, 0x00, 0x00, 0x00, 0x4b, 0x65, 0x65, 0x70, 0x2f, 0x6e, 0x6f, 0x74, 0x65, 0x2e, 0x6a, 0x73, 0x6f, 0x6e, 0x55, 0x54, 0x05, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x01, 0x02, 0x1e, 0x03, 0x0a, 0x00, 0x02, 0x00, 0x00, 0x00, 0x04, 0x82, 0x61, 0x5c, 0xd8, 0x98, 0x9a, 0x64, 0x1d, 0x00, 0x00, 0x00, 0x1d, 0x00, 0x00, 0x00, 0x0f, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0xa4, 0x81, 0x75, 0x00, 0x00, 0x00, 0x4b, 0x65, 0x65, 0x70, 0x2f, 0x65, 0x6d, 0x70, 0x74, 0x79, 0x2e, 0x6a, 0x73, 0x6f, 0x6e, 0x55, 0x54, 0x05, 0x00, 0x03, 0x38, 0x58, 0xa4, 0x69, 0x75, 0x78, 0x0b, 0x00, 0x01, 0x04, 0xf5, 0x01, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x50, 0x4b, 0x05, 0x06, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0xa9, 0x00, 0x00, 0x00, 0xdb, 0x00, 0x00, 0x00, 0x00, 0x00];

    const LOREM: &str = "the quick brown fox jumps over the lazy dog\n";

    fn central_directory_offset(bytes: &[u8]) -> usize {
        bytes
            .windows(4)
            .position(|window| window == [0x50, 0x4b, 0x01, 0x02])
            .expect("central directory")
    }

    #[test]
    fn test_preserves_uninterpreted_header_flags() {
        let mut bytes = STORED_ZIP.to_vec();
        let central = central_directory_offset(&bytes);
        bytes[central + 9] |= 0x08; // UTF-8 names
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("open zip");
        assert!(archive.by_index(0).is_err());

        bytes[7] |= 0x08;
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("open zip");
        let mut file = archive.by_index(0).expect("matching flags");
        let mut content = String::new();
        file.read_to_string(&mut content).expect("read");
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_rejects_encrypted_entries() {
        let mut bytes = STORED_ZIP.to_vec();
        let central = central_directory_offset(&bytes);
        bytes[central + 8] |= 0x01;
        assert!(ZipArchive::new(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn test_stored_single_file() {
        let mut archive = ZipArchive::new(Cursor::new(STORED_ZIP)).expect("open zip");
        assert_eq!(archive.len(), 1);
        let mut file = archive.by_index(0).expect("by_index");
        assert_eq!(file.name(), "hello.txt");
        let mut content = String::new();
        file.read_to_string(&mut content).expect("read");
        assert_eq!(content, "hello world");
    }

    #[test]
    fn test_deflate_single_file() {
        let mut archive = ZipArchive::new(Cursor::new(DEFLATE_ZIP)).expect("open zip");
        assert_eq!(archive.len(), 1);
        let mut file = archive.by_index(0).expect("by_index");
        assert_eq!(file.name(), "lorem.txt");
        let mut content = String::new();
        file.read_to_string(&mut content).expect("read");
        assert_eq!(content, LOREM.repeat(50));
    }

    #[test]
    fn test_multi_file_mixed_compression() {
        let mut archive = ZipArchive::new(Cursor::new(MULTI_ZIP)).expect("open zip");
        assert_eq!(archive.len(), 2);

        let mut file0 = archive.by_index(0).expect("by_index 0");
        assert_eq!(file0.name(), "hello.txt");
        let mut content0 = String::new();
        file0.read_to_string(&mut content0).expect("read 0");
        assert_eq!(content0, "hello world");

        let mut file1 = archive.by_index(1).expect("by_index 1");
        assert_eq!(file1.name(), "lorem.txt");
        let mut content1 = String::new();
        file1.read_to_string(&mut content1).expect("read 1");
        assert_eq!(content1, LOREM.repeat(50));
    }

    #[test]
    fn test_keep_subfolder() {
        let mut archive = ZipArchive::new(Cursor::new(KEEP_ZIP)).expect("open zip");
        assert_eq!(archive.len(), 2);

        let mut note = archive.by_index(0).expect("by_index 0");
        assert_eq!(note.name(), "Keep/note.json");
        let mut note_content = String::new();
        note.read_to_string(&mut note_content).expect("read note");
        assert_eq!(
            note_content,
            r#"{"title":"Test Note","textContent":"body text"}"#
        );

        let mut empty = archive.by_index(1).expect("by_index 1");
        assert_eq!(empty.name(), "Keep/empty.json");
        let mut empty_content = String::new();
        empty
            .read_to_string(&mut empty_content)
            .expect("read empty");
        assert_eq!(empty_content, r#"{"title":"","textContent":""}"#);
    }

    #[test]
    fn test_is_empty() {
        let archive = ZipArchive::new(Cursor::new(STORED_ZIP)).expect("open zip");
        assert!(!archive.is_empty());
    }

    #[test]
    fn test_out_of_range() {
        let mut archive = ZipArchive::new(Cursor::new(STORED_ZIP)).expect("open zip");
        assert!(matches!(archive.by_index(99), Err(ZipError::FileNotFound)));
    }

    #[test]
    fn test_invalid_zip() {
        assert!(matches!(
            ZipArchive::new(Cursor::new(b"not a zip file")),
            Err(ZipError::InvalidZip(_))
        ));
    }

    #[test]
    fn test_rejects_archive_larger_than_limit() {
        assert!(matches!(
            read_limited(&mut Cursor::new([0; 5]), 4),
            Err(ZipError::InvalidZip("archive size exceeds limit"))
        ));
    }

    #[test]
    fn test_rejects_invalid_local_header() {
        let mut bytes = STORED_ZIP.to_vec();
        bytes[0] = 0;
        let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open zip");

        assert!(matches!(
            archive.by_index(0),
            Err(ZipError::InvalidZip("invalid local file header signature"))
        ));
    }

    #[test]
    fn test_rejects_incorrect_uncompressed_size() {
        let mut bytes = DEFLATE_ZIP.to_vec();
        let central = central_directory_offset(&bytes);
        bytes[22..26].copy_from_slice(&1u32.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
        let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open zip");

        assert!(matches!(
            archive.by_index(0),
            Err(ZipError::DecompressError)
        ));
    }

    #[test]
    fn test_rejects_incorrect_checksum() {
        let mut bytes = STORED_ZIP.to_vec();
        let central = central_directory_offset(&bytes);
        bytes[14..18].copy_from_slice(&0u32.to_le_bytes());
        bytes[central + 16..central + 20].copy_from_slice(&0u32.to_le_bytes());
        let mut archive = ZipArchive::new(Cursor::new(bytes)).expect("open zip");

        assert!(matches!(
            archive.by_index(0),
            Err(ZipError::InvalidZip(
                "entry checksum does not match directory"
            ))
        ));
    }

    #[test]
    fn rejects_malformed_end_records_and_directory_bounds() {
        for (relative, value) in [(4, 1u16), (8, 2), (10, 2), (20, 1)] {
            let mut bytes = STORED_ZIP.to_vec();
            let end = bytes.len() - 22;
            bytes[end + relative..end + relative + 2].copy_from_slice(&value.to_le_bytes());
            assert!(
                ZipArchive::new(Cursor::new(bytes)).is_err(),
                "field {relative}"
            );
        }
        let mut bytes = STORED_ZIP.to_vec();
        let end = bytes.len() - 22;
        bytes[end + 12..end + 16].copy_from_slice(&0u32.to_le_bytes());
        assert!(ZipArchive::new(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn accepts_end_signatures_inside_comments() {
        let mut bytes = STORED_ZIP.to_vec();
        let end = bytes.len() - 22;
        bytes[end + 20..end + 22].copy_from_slice(&22u16.to_le_bytes());
        bytes.extend_from_slice(b"PK\x05\x06");
        bytes.extend_from_slice(&[0; 18]);
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name(), "hello.txt");
    }

    #[test]
    fn rejects_truncated_extra_fields_and_overlapping_entries() {
        let mut bytes = STORED_ZIP.to_vec();
        let central = central_directory_offset(&bytes);
        let name_end = central + 46 + usize::from(read_u16_le(&bytes, central + 28));
        bytes[name_end + 2..name_end + 4].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(ZipArchive::new(Cursor::new(bytes)).is_err());
        let mut bytes = STORED_ZIP.to_vec();
        let local_name_end = 30 + usize::from(read_u16_le(&bytes, 26));
        bytes[local_name_end + 2..local_name_end + 4].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(
            ZipArchive::new(Cursor::new(bytes))
                .unwrap()
                .by_index(0)
                .is_err()
        );
        let mut bytes = MULTI_ZIP.to_vec();
        let central = central_directory_offset(&bytes);
        let second = central
            + 46
            + usize::from(read_u16_le(&bytes, central + 28))
            + usize::from(read_u16_le(&bytes, central + 30));
        bytes[second + 42..second + 46].copy_from_slice(&0u32.to_le_bytes());
        assert!(ZipArchive::new(Cursor::new(bytes)).is_err());
    }

    #[test]
    fn validates_signed_and_unsigned_data_descriptors() {
        let bytes = include_bytes!("../tests/fixtures/descriptor.zip");
        let signature = bytes
            .windows(4)
            .position(|value| value == b"PK\x07\x08")
            .unwrap();
        let mut corrupt = bytes.to_vec();
        corrupt[signature + 4] ^= 1;
        assert!(
            ZipArchive::new(Cursor::new(corrupt))
                .unwrap()
                .by_index(0)
                .is_err()
        );
        let mut unsigned = bytes.to_vec();
        unsigned.drain(signature..signature + 4);
        let end = unsigned.len() - 22;
        let offset = read_u32_le(&unsigned, end + 16) - 4;
        unsigned[end + 16..end + 20].copy_from_slice(&offset.to_le_bytes());
        let mut file = ZipArchive::new(Cursor::new(unsigned))
            .unwrap()
            .by_index(0)
            .unwrap();
        let mut data = Vec::new();
        file.read_to_end(&mut data).unwrap();
        assert_eq!(data, b"streamed data".repeat(100));
    }

    #[test]
    fn exposes_unix_metadata_and_rejects_zip64_sentinels() {
        let mut archive = ZipArchive::new(Cursor::new(STORED_ZIP)).unwrap();
        let file = archive.by_index(0).unwrap();
        assert_eq!(file.unix_mode(), Some(0o100644));
        assert!(file.is_file());
        assert!(!file.is_dir());
        assert!(!file.is_symlink());
        assert_eq!(
            file.last_modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            1772378168
        );
        let mut bytes = STORED_ZIP.to_vec();
        let central = central_directory_offset(&bytes);
        bytes[central + 20..central + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            ZipArchive::new(Cursor::new(bytes)),
            Err(ZipError::InvalidZip("ZIP64 is unsupported"))
        ));
    }

    #[test]
    fn rejects_every_truncated_prefix() {
        for end in 0..STORED_ZIP.len() {
            assert!(
                ZipArchive::new(Cursor::new(&STORED_ZIP[..end])).is_err(),
                "prefix {end}"
            );
        }
    }

    #[test]
    fn accepts_unspecified_and_regular_file_attributes_on_directory_entries() {
        let fixture = include_bytes!("../tests/fixtures/unix.zip");
        for mode in [0xffffu32, 0o100644] {
            let mut bytes = fixture.to_vec();
            let central = central_directory_offset(&bytes);
            bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
            let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
            let entry = archive.by_index(0).unwrap();
            assert!(entry.is_dir());
            assert_eq!(entry.unix_mode(), None);
        }
    }
}
