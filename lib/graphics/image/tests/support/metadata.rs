/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Metadata-only containers: deliberately no pixel payload, so tests detect accidental decoding.

/// TIFF with a primary IFD and an EXIF IFD, in either byte order.
pub(super) fn photo_tiff(little: bool) -> Vec<u8> {
    let word = |value: u16| {
        if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
        .to_vec()
    };
    let long = |value: u32| {
        if little {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        }
        .to_vec()
    };
    let fraction = |numerator: i32, denominator: i32| {
        [long(numerator as u32), long(denominator as u32)].concat()
    };
    let title: Vec<u8> = "A & B 🐈\0"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let mut first = vec![
        (0x0100, 4, 1, long(640)),
        (0x0101, 4, 1, long(480)),
        (0x0102, 3, 3, [word(8), word(8), word(8)].concat()),
        (0x010e, 2, 13, b"A test photo\0".to_vec()),
        (0x010f, 2, 5, b"Acme\0".to_vec()),
        (0x0110, 2, 9, b"Camera X\0".to_vec()),
        (0x0112, 3, 1, word(6)),
        (0x0115, 3, 1, word(3)),
        (0x011a, 5, 1, fraction(300, 1)),
        (0x011b, 5, 1, fraction(600, 2)),
        (0x0128, 3, 1, word(2)),
        (0x013b, 2, 9, b"Jane Doe\0".to_vec()),
        (0x8298, 2, 24, b"Copyright 2026 Jane Doe\0".to_vec()),
        (0x8769, 4, 1, long(0)),
        (0x9c9b, 1, title.len() as u32, title),
    ];
    // The declared ASCII lengths are derived here so adding text cannot corrupt the fixture.
    for (_, kind, count, data) in &mut first {
        if *kind == 2 {
            *count = data.len() as u32;
        }
    }
    let exif = vec![
        (0x829a, 5, 1, fraction(1, 125)),
        (0x829d, 5, 1, fraction(28, 10)),
        (0x8827, 3, 1, word(200)),
        (0x9003, 2, 20, b"2024:02:29 12:34:56\0".to_vec()),
        (0x9011, 2, 7, b"+02:00\0".to_vec()),
        (0x9202, 5, 1, fraction(2_970_854, 1_000_000)),
        (0x9204, 10, 1, fraction(-1, 3)),
        (0x920a, 5, 1, fraction(351, 10)),
    ];
    let exif_at = 8 + 2 + first.len() * 12 + 4;
    first
        .iter_mut()
        .find(|entry| entry.0 == 0x8769)
        .expect("EXIF pointer")
        .3 = long(exif_at as u32);
    let mut bytes = vec![0; exif_at + 2 + exif.len() * 12 + 4];
    bytes[..8].copy_from_slice(if little {
        b"II\x2a\0\x08\0\0\0"
    } else {
        b"MM\0\x2a\0\0\0\x08"
    });
    for (at, entries) in [(8, first), (exif_at, exif)] {
        bytes[at..at + 2].copy_from_slice(&word(entries.len() as u16));
        for (index, (tag, kind, count, data)) in entries.into_iter().enumerate() {
            let at = at + 2 + index * 12;
            bytes[at..at + 2].copy_from_slice(&word(tag));
            bytes[at + 2..at + 4].copy_from_slice(&word(kind));
            bytes[at + 4..at + 8].copy_from_slice(&long(count));
            if data.len() <= 4 {
                bytes[at + 8..at + 8 + data.len()].copy_from_slice(&data);
            } else {
                let offset = bytes.len();
                bytes[at + 8..at + 12].copy_from_slice(&long(offset as u32));
                bytes.extend_from_slice(&data);
                if !bytes.len().is_multiple_of(2) {
                    bytes.push(0);
                }
            }
        }
    }
    bytes
}

/// JPEG headers carrying the TIFF's EXIF metadata.
pub(super) fn photo_jpeg(exif: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xff, 0xd8, 0xff, 0xe1];
    bytes.extend_from_slice(&((exif.len() + 8) as u16).to_be_bytes());
    bytes.extend_from_slice(b"Exif\0\0");
    bytes.extend_from_slice(exif);
    bytes.extend_from_slice(&[
        0xff, 0xc0, 0, 17, 8, 1, 224, 2, 128, 3, 1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 0xff, 0xda,
    ]);
    bytes
}

/// A PNG chunk with its CRC, also used for malformed-data tests.
pub(super) fn png_chunk(bytes: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
    bytes.extend_from_slice(kind);
    bytes.extend_from_slice(data);
    let mut crc = !0u32;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    bytes.extend_from_slice(&(!crc).to_be_bytes());
}

/// PNG headers carrying the TIFF's EXIF metadata.
pub(super) fn photo_png(exif: &[u8]) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    png_chunk(
        &mut bytes,
        b"IHDR",
        &[0, 0, 2, 128, 0, 0, 1, 224, 8, 2, 0, 0, 0],
    );
    png_chunk(&mut bytes, b"eXIf", exif);
    png_chunk(&mut bytes, b"IEND", &[]);
    bytes
}

/// WebP headers carrying the TIFF's EXIF metadata.
pub(super) fn photo_webp(exif: &[u8]) -> Vec<u8> {
    let mut bytes = b"RIFF\0\0\0\0WEBPVP8X\x0a\0\0\0\x08\0\0\0\x7f\x02\0\xdf\x01\0EXIF".to_vec();
    bytes.extend_from_slice(&(exif.len() as u32).to_le_bytes());
    bytes.extend_from_slice(exif);
    if !exif.len().is_multiple_of(2) {
        bytes.push(0);
    }
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes
}
