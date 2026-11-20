/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Image properties without decoding the pixels of common photo formats.

use alloc::vec::Vec;

#[cfg(any(feature = "jpeg", feature = "png"))]
use crate::Reader;
use crate::metadata::list_exif;
#[cfg(any(feature = "jpeg", feature = "png", feature = "webp"))]
use crate::metadata::list_xmp;
use crate::{DecodeError, Format, Metadata, Result};
#[cfg(any(feature = "jpeg", feature = "webp"))]
use crate::{MetadataKey, MetadataValue};

#[cfg(all(
    test,
    feature = "jpeg",
    feature = "png",
    feature = "tiff",
    feature = "webp"
))]
#[path = "../tests/support/metadata.rs"]
mod fixtures;

/// The natural display dimensions and metadata of an image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageInfo {
    /// Display width, following the format decoder's orientation handling.
    pub width: u32,
    /// Display height, following the format decoder's orientation handling.
    pub height: u32,
    /// Encoded bits per pixel, when the header specifies them.
    pub bit_depth: Option<u32>,
    /// Readable metadata and its original typed properties.
    pub metadata: Vec<Metadata>,
}

impl ImageInfo {
    pub(super) fn new(
        width: u32,
        height: u32,
        bit_depth: Option<u32>,
        metadata: Vec<Metadata>,
    ) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(DecodeError::InvalidHeader);
        }
        Ok(Self {
            width,
            height,
            bit_depth,
            metadata,
        })
    }

    #[cfg(any(feature = "jpeg", feature = "webp"))]
    pub(super) fn oriented(mut self) -> Self {
        let orientation = self
            .metadata
            .iter()
            .flat_map(Metadata::properties)
            .find_map(|property| match (&property.key, &property.value) {
                (MetadataKey::Orientation, MetadataValue::Integer(value)) => Some(*value),
                _ => None,
            });
        if orientation.is_some_and(|value| (5..=8).contains(&value)) {
            core::mem::swap(&mut self.width, &mut self.height);
        }
        self
    }
}

/// Inspects JPEG, PNG/APNG, TIFF and WebP headers without decoding their pixels. Other enabled
/// formats use their decoder. Header inspection does not validate the compressed pixel data.
pub fn inspect(data: &[u8]) -> Result<ImageInfo> {
    let Some(format) = Format::from_signature(data) else {
        return decoded_info(data);
    };
    #[allow(unreachable_patterns)]
    match format {
        #[cfg(feature = "jpeg")]
        Format::Jpeg => jpeg(data),
        #[cfg(feature = "png")]
        Format::Png => png(data),
        #[cfg(feature = "tiff")]
        Format::Tiff => crate::tiff::inspect(data),
        #[cfg(feature = "webp")]
        Format::WebP => webp(data),
        _ => decoded_info(data),
    }
}

fn decoded_info(data: &[u8]) -> Result<ImageInfo> {
    let image = crate::decode(data)?;
    // These pixels have already been oriented by the decoder.
    Ok(ImageInfo {
        width: image.width(),
        height: image.height(),
        bit_depth: None,
        metadata: image.metadata,
    })
}

#[cfg(feature = "jpeg")]
fn jpeg(data: &[u8]) -> Result<ImageInfo> {
    let mut reader = Reader::new(data.get(2..).ok_or(DecodeError::InvalidHeader)?);
    let mut dimensions = None;
    let mut metadata = Vec::new();
    loop {
        if reader.byte()? != 255 {
            return Err(DecodeError::InvalidHeader);
        }
        let mut marker = reader.byte()?;
        while marker == 255 {
            marker = reader.byte()?;
        }
        if matches!(marker, 0xda | 0xd9) {
            break;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            continue;
        }
        let length = reader
            .be16()?
            .checked_sub(2)
            .ok_or(DecodeError::InvalidHeader)?;
        let bytes = reader.take(usize::from(length))?;
        match marker {
            0xc0..=0xcf if !matches!(marker, 0xc4 | 0xc8 | 0xcc) => {
                let mut header = Reader::new(bytes);
                let depth = u32::from(header.byte()?);
                let height = u32::from(header.be16()?);
                let width = u32::from(header.be16()?);
                let channels = u32::from(header.byte()?);
                if depth == 0 || channels == 0 {
                    return Err(DecodeError::InvalidHeader);
                }
                header.take(channels as usize * 3)?;
                dimensions = Some((width, height, depth * channels));
            }
            0xe0 => {
                if let Some(jfif) = bytes.strip_prefix(b"JFIF\0") {
                    metadata.extend(crate::jpeg::jfif_resolution(jfif));
                }
            }
            0xe1 => {
                if let Some(exif) = bytes.strip_prefix(b"Exif\0\0") {
                    list_exif(exif, &mut metadata);
                } else if let Some(xmp) = bytes.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0") {
                    list_xmp(xmp, &mut metadata);
                }
            }
            _ => {}
        }
    }
    let (width, height, depth) = dimensions.ok_or(DecodeError::InvalidHeader)?;
    ImageInfo::new(width, height, Some(depth), metadata).map(ImageInfo::oriented)
}

#[cfg(feature = "png")]
fn png(data: &[u8]) -> Result<ImageInfo> {
    let mut reader = Reader::new(data.get(8..).ok_or(DecodeError::InvalidHeader)?);
    let (kind, bytes) = crate::png::chunk(&mut reader)?;
    if kind != b"IHDR" || bytes.len() != 13 {
        return Err(DecodeError::InvalidHeader);
    }
    let mut header = Reader::new(bytes);
    let (width, height) = (header.be32()?, header.be32()?);
    let depth = header.byte()?;
    let color = header.byte()?;
    let channels = match color {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(DecodeError::InvalidHeader),
    };
    let valid_depth = match color {
        0 => matches!(depth, 1 | 2 | 4 | 8 | 16),
        3 => matches!(depth, 1 | 2 | 4 | 8),
        _ => matches!(depth, 8 | 16),
    };
    if !valid_depth || header.byte()? != 0 || header.byte()? != 0 || header.byte()? > 1 {
        return Err(DecodeError::InvalidHeader);
    }
    let mut metadata = Vec::new();
    let mut budget = crate::Budget::default();
    // Metadata expansion is capped independently of the much larger pixel-decoding budget.
    budget.claim(crate::MAX_BYTES - 16 * 1024 * 1024)?;
    loop {
        let (kind, bytes) = crate::png::chunk(&mut reader)?;
        match kind {
            b"IEND" => break,
            b"eXIf" => list_exif(bytes, &mut metadata),
            b"pHYs" => metadata.extend(crate::metadata::png_resolution(bytes)),
            b"tEXt" | b"zTXt" | b"iTXt" => {
                if let Some((keyword, text)) = crate::png::text_entry(kind, bytes, &mut budget) {
                    if keyword == "XML:com.adobe.xmp" {
                        list_xmp(text.as_bytes(), &mut metadata);
                    } else {
                        metadata.push(Metadata::new(crate::MetadataGroup::Text, keyword, text));
                    }
                }
            }
            _ => {}
        }
    }
    ImageInfo::new(width, height, Some(u32::from(depth) * channels), metadata)
}

#[cfg(feature = "webp")]
fn webp(data: &[u8]) -> Result<ImageInfo> {
    let length = u32::from_le_bytes(
        data.get(4..8)
            .ok_or(DecodeError::InvalidHeader)?
            .try_into()
            .expect("four bytes"),
    ) as usize;
    let end = length.checked_add(8).ok_or(DecodeError::InvalidHeader)?;
    let mut bytes = data.get(12..end).ok_or(DecodeError::InvalidHeader)?;
    let mut dimensions = None;
    let mut metadata = Vec::new();
    while !bytes.is_empty() {
        let kind = bytes.get(..4).ok_or(DecodeError::InvalidHeader)?;
        let length = u32::from_le_bytes(
            bytes
                .get(4..8)
                .ok_or(DecodeError::InvalidHeader)?
                .try_into()
                .expect("four bytes"),
        ) as usize;
        let payload = bytes
            .get(
                8..8usize
                    .checked_add(length)
                    .ok_or(DecodeError::InvalidHeader)?,
            )
            .ok_or(DecodeError::InvalidHeader)?;
        let le24 = |bytes: &[u8]| -> u32 {
            u32::from(bytes[0]) | u32::from(bytes[1]) << 8 | u32::from(bytes[2]) << 16
        };
        match kind {
            b"VP8X" if payload.len() == 10 => {
                dimensions = Some((le24(&payload[4..7]) + 1, le24(&payload[7..10]) + 1))
            }
            b"VP8 "
                if dimensions.is_none()
                    && payload.len() >= 10
                    && payload[3..6] == [0x9d, 0x01, 0x2a] =>
            {
                dimensions = Some((
                    u32::from(u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff),
                    u32::from(u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff),
                ))
            }
            b"VP8L" if dimensions.is_none() && payload.len() >= 5 && payload[0] == 0x2f => {
                let bits = u32::from_le_bytes(payload[1..5].try_into().expect("four bytes"));
                dimensions = Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1));
            }
            b"EXIF" => list_exif(
                payload.strip_prefix(b"Exif\0\0").unwrap_or(payload),
                &mut metadata,
            ),
            b"XMP " => list_xmp(payload, &mut metadata),
            _ => {}
        }
        let next = length
            .checked_add(8)
            .and_then(|next| next.checked_add(length & 1))
            .ok_or(DecodeError::InvalidHeader)?;
        bytes = bytes.get(next..).ok_or(DecodeError::InvalidHeader)?;
    }
    let (width, height) = dimensions.ok_or(DecodeError::InvalidHeader)?;
    ImageInfo::new(width, height, None, metadata).map(ImageInfo::oriented)
}

#[cfg(all(
    test,
    feature = "jpeg",
    feature = "png",
    feature = "tiff",
    feature = "webp"
))]
mod tests {
    use super::*;

    #[test]
    fn photo_headers_preserve_typed_metadata_without_pixel_data() {
        for little in [false, true] {
            let exif = fixtures::photo_tiff(little);
            for data in [
                exif.clone(),
                fixtures::photo_jpeg(&exif),
                fixtures::photo_png(&exif),
                fixtures::photo_webp(&exif),
            ] {
                assert!(
                    crate::decode(&data).is_err(),
                    "fixture must have no decodable pixels"
                );
                let info = inspect(&data).expect("inspect headers");
                let expected = if Format::from_signature(&data) == Some(Format::Png) {
                    (640, 480)
                } else {
                    (480, 640)
                };
                assert_eq!((info.width, info.height), expected);
                let properties: Vec<_> = info
                    .metadata
                    .iter()
                    .flat_map(Metadata::properties)
                    .collect();
                let find = |key| {
                    &properties
                        .iter()
                        .find(|property| property.key == key)
                        .expect("typed property")
                        .value
                };
                assert_eq!(
                    find(MetadataKey::CameraMake),
                    &MetadataValue::Text("Acme".into())
                );
                assert_eq!(
                    find(MetadataKey::Title),
                    &MetadataValue::Text("A & B 🐈".into())
                );
                assert_eq!(find(MetadataKey::Iso), &MetadataValue::Integer(200));
                assert_eq!(
                    find(MetadataKey::ExposureTime),
                    &MetadataValue::Rational {
                        numerator: 1,
                        denominator: 125
                    }
                );
                assert_eq!(
                    find(MetadataKey::ExposureBias),
                    &MetadataValue::Rational {
                        numerator: -1,
                        denominator: 3
                    }
                );
                assert_eq!(
                    find(MetadataKey::FNumber),
                    &MetadataValue::Rational {
                        numerator: 28,
                        denominator: 10
                    }
                );
                assert!(
                    matches!(find(MetadataKey::DateTaken), MetadataValue::DateTime(date) if date.year == 2024 && date.month == 2 && date.day == 29 && date.offset_minutes == Some(120))
                );
            }
        }
    }

    #[test]
    fn inspection_handles_truncated_and_invalid_metadata() {
        let exif = fixtures::photo_tiff(true);
        for data in [
            exif.clone(),
            fixtures::photo_jpeg(&exif),
            fixtures::photo_png(&exif),
            fixtures::photo_webp(&exif),
        ] {
            for end in 0..data.len() {
                let _ = inspect(&data[..end]);
            }
        }
        let mut invalid_date = exif.clone();
        let at = invalid_date
            .windows(10)
            .position(|bytes| bytes == b"2024:02:29")
            .expect("date");
        invalid_date[at + 3] = b'3';
        let info = inspect(&invalid_date).expect("other metadata remains usable");
        assert!(
            !info
                .metadata
                .iter()
                .flat_map(Metadata::properties)
                .any(|property| property.key == MetadataKey::DateTaken)
        );
    }

    #[test]
    fn inspected_dimensions_match_decoded_examples() {
        let examples = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../apps/desktop/imageview/examples");
        for name in [
            "dice.jfif",
            "dice.png",
            "ball.apng",
            "dice.tif",
            "dice.webp",
            "ball.webp",
            "dice-lossy.webp",
            "dice.qoi",
        ] {
            let data = std::fs::read(examples.join(name)).expect("example");
            let decoded = crate::decode(&data).expect("decode");
            let info = inspect(&data).expect("inspect");
            assert_eq!(
                (info.width, info.height),
                (decoded.width(), decoded.height()),
                "{name}"
            );
        }
    }

    #[test]
    fn png_exif_dimensions_match_the_displayed_pixels() {
        let bitmap = crate::Bitmap::new(3, 2, vec![255; 24]).expect("bitmap");
        let mut bytes = crate::encode(&bitmap, Format::Png).expect("PNG");
        bytes.truncate(bytes.len() - 12);
        fixtures::png_chunk(&mut bytes, b"eXIf", &fixtures::photo_tiff(true));
        fixtures::png_chunk(&mut bytes, b"IEND", &[]);
        let decoded = crate::decode(&bytes).expect("decode");
        let info = inspect(&bytes).expect("inspect");
        assert_eq!(
            (info.width, info.height),
            (decoded.width(), decoded.height())
        );
        assert_eq!((info.width, info.height), (3, 2));
    }
}
