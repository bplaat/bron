/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::io::{Read, Seek, SeekFrom, Write};

use crate::{
    CRC32, MAX_ARCHIVE_SIZE, MAX_TOTAL_UNCOMPRESSED_SIZE, MAX_UNCOMPRESSED_SIZE, ZipArchive,
    ZipError,
};

/// Appends a file to a single-disk, non-ZIP64 archive, preserving existing entries verbatim.
/// Returns an error if the name already exists. The reader must also support writing and seeking.
pub fn append_file<R: Read + Write + Seek>(
    mut reader: R,
    name: &str,
    data: &[u8],
) -> Result<(), ZipError> {
    if name.is_empty()
        || name.starts_with(['/', '\\'])
        || name.contains(['\\', ':'])
        || name.split('/').any(|part| matches!(part, "." | ".." | ""))
    {
        return Err(ZipError::InvalidZip("invalid file name"));
    }
    let name_len = u16::try_from(name.len()).map_err(|_| ZipError::InvalidZip("name too long"))?;
    let size = u32::try_from(data.len()).map_err(|_| ZipError::InvalidZip("file too large"))?;
    if data.len() > MAX_UNCOMPRESSED_SIZE {
        return Err(ZipError::InvalidZip(
            "uncompressed entry size exceeds limit",
        ));
    }
    reader.rewind()?;
    let mut archive = ZipArchive::new(&mut reader)?;
    let count = u16::try_from(archive.len() + 1)
        .ok()
        .filter(|count| *count != u16::MAX)
        .ok_or(ZipError::InvalidZip("too many entries"))?;
    for index in 0..archive.len() {
        if archive.by_index(index)?.name() == name {
            return Err(ZipError::InvalidZip("file already exists"));
        }
    }
    if archive.extracted_size + data.len() > MAX_TOTAL_UNCOMPRESSED_SIZE {
        return Err(ZipError::InvalidZip(
            "total uncompressed archive size exceeds limit",
        ));
    }
    let end = archive.end;
    drop(archive);

    let length = reader.seek(SeekFrom::End(0))?;
    let mut comment = vec![0; (length - end.offset as u64 - 20) as usize];
    reader.seek(SeekFrom::Start(end.offset as u64 + 20))?;
    reader.read_exact(&mut comment)?;
    let mut directory = vec![0; end.offset - end.directory_offset];
    reader.seek(SeekFrom::Start(end.directory_offset as u64))?;
    reader.read_exact(&mut directory)?;
    let offset = u32::try_from(length).map_err(|_| ZipError::InvalidZip("archive too large"))?;
    let compressed = miniz_oxide::deflate::compress_to_vec(data, 6);
    let (method, payload) = if compressed.len() < data.len() {
        (8u16, compressed.as_slice())
    } else {
        (0u16, data)
    };
    let compressed_size = payload.len() as u32;
    let checksum = CRC32.checksum(data);
    let mut header = Vec::new();
    // Version, UTF-8 flag, compression, and a fixed DOS date (1980-01-01).
    for value in [20u16, 0x800, method, 0, 0x21] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    for value in [checksum, compressed_size, size] {
        header.extend_from_slice(&value.to_le_bytes());
    }
    header.extend_from_slice(&name_len.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());

    let new_directory_offset =
        length + 4 + header.len() as u64 + name.len() as u64 + payload.len() as u64;
    let new_directory_size = directory.len() as u64 + 46 + name.len() as u64;
    if new_directory_offset + new_directory_size + 22 + (comment.len() - 2) as u64
        > MAX_ARCHIVE_SIZE as u64
    {
        return Err(ZipError::InvalidZip("archive size exceeds limit"));
    }
    let new_directory_offset = u32::try_from(new_directory_offset)
        .map_err(|_| ZipError::InvalidZip("archive too large"))?;
    let new_directory_size = u32::try_from(new_directory_size)
        .map_err(|_| ZipError::InvalidZip("directory too large"))?;
    reader.seek(SeekFrom::End(0))?;
    reader.write_all(b"PK\x03\x04")?;
    reader.write_all(&header)?;
    reader.write_all(name.as_bytes())?;
    reader.write_all(payload)?;
    reader.write_all(&directory)?;
    reader.write_all(b"PK\x01\x02")?;
    reader.write_all(&20u16.to_le_bytes())?;
    reader.write_all(&header)?;
    reader.write_all(&[0; 10])?;
    reader.write_all(&offset.to_le_bytes())?;
    reader.write_all(name.as_bytes())?;
    reader.write_all(b"PK\x05\x06")?;
    reader.write_all(&[0; 4])?;
    reader.write_all(&count.to_le_bytes())?;
    reader.write_all(&count.to_le_bytes())?;
    reader.write_all(&new_directory_size.to_le_bytes())?;
    reader.write_all(&new_directory_offset.to_le_bytes())?;
    // Preserve the archive comment.
    reader.write_all(&comment)?;
    reader.flush()?;
    Ok(())
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn appends_compressed_and_stored_files_preserving_existing_bytes() {
        let mut bytes = b"PK\x05\x06".to_vec();
        bytes.extend_from_slice(&[0; 18]);
        let mut archive = Cursor::new(bytes);
        let data = vec![42; 4096];
        append_file(&mut archive, "resources.bin", &data).unwrap();
        let original = archive.get_ref().clone();
        append_file(&mut archive, "classes.dex", b"dex").unwrap();
        assert!(archive.get_ref().starts_with(&original));
        archive.set_position(0);
        let mut reader = ZipArchive::new(&mut archive).unwrap();
        assert_eq!(reader.len(), 2);
        let mut extracted = Vec::new();
        reader
            .by_index(0)
            .unwrap()
            .read_to_end(&mut extracted)
            .unwrap();
        assert_eq!(extracted, data);
        extracted.clear();
        reader
            .by_index(1)
            .unwrap()
            .read_to_end(&mut extracted)
            .unwrap();
        assert_eq!(extracted, b"dex");
        drop(reader);
        archive.set_position(0);
        assert!(append_file(&mut archive, "classes.dex", b"duplicate").is_err());
        archive.set_position(0);
        assert!(append_file(&mut archive, "../escape", b"bad").is_err());
    }

    #[test]
    fn preserves_comments_and_rejects_unsafe_names_and_multi_disk_archives() {
        let mut bytes = b"PK\x05\x06".to_vec();
        bytes.extend_from_slice(&[0; 16]);
        bytes.extend_from_slice(&7u16.to_le_bytes());
        bytes.extend_from_slice(b"comment");
        for name in [
            "",
            "/absolute",
            "../escape",
            "a/../escape",
            "C:/escape",
            "a\\b",
            "a//b",
        ] {
            let mut archive = Cursor::new(bytes.clone());
            assert!(append_file(&mut archive, name, b"data").is_err(), "{name}");
            assert_eq!(archive.into_inner(), bytes);
        }
        let mut archive = Cursor::new(bytes.clone());
        append_file(&mut archive, "file", b"contents").unwrap();
        assert!(archive.get_ref().ends_with(b"comment"));
        archive.set_position(0);
        assert_eq!(ZipArchive::new(archive).unwrap().len(), 1);
        bytes[4] = 1;
        let mut archive = Cursor::new(bytes.clone());
        assert!(append_file(&mut archive, "file", b"contents").is_err());
        assert_eq!(archive.into_inner(), bytes);
    }
}
