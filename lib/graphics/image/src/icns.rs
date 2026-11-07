/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;
use core::time::Duration;

use super::{
    Budget, DecodeError, Format, Image, LoopCount, Reader, Result, jpeg2000, pixel_len, png,
};

// Modern ICNS entries contain PNG or JPEG 2000. Older RGB entries have separate masks.
const fn dimension(kind: &[u8]) -> Option<u32> {
    Some(match kind {
        b"is32" | b"icp4" | b"ic04" => 16,
        b"il32" | b"icp5" | b"ic11" | b"ic05" => 32,
        b"ih32" => 48,
        b"icp6" | b"ic12" => 64,
        b"it32" | b"ic07" => 128,
        b"ic08" | b"ic13" => 256,
        b"ic09" | b"ic14" => 512,
        b"ic10" => 1024,
        _ => return None,
    })
}

fn entry<'a>(r: &mut Reader<'a>) -> Result<(&'a [u8], &'a [u8])> {
    let kind = r.take(4)?;
    let length = (r.be32()? as usize)
        .checked_sub(8)
        .ok_or(DecodeError::InvalidData)?;
    Ok((kind, r.take(length)?))
}

// An icon entry of a file: its kind, payload and pixel size.
type Icon<'a> = (&'a [u8], &'a [u8], u32);

// Returns the entries after the header, and the icons among them in file order.
fn icons(data: &[u8]) -> Result<(&[u8], Vec<Icon<'_>>)> {
    let mut r = Reader::new(data);
    if r.take(4)? != b"icns" {
        return Err(DecodeError::InvalidHeader);
    }
    let length = r.be32()? as usize;
    if length < 8 {
        return Err(DecodeError::InvalidHeader);
    }
    let data = data.get(8..length).ok_or(DecodeError::InvalidData)?;
    let mut r = Reader::new(data);
    let mut icons = Vec::new();
    while r.remaining() != 0 {
        let (kind, payload) = entry(&mut r)?;
        if let Some(size) = dimension(kind) {
            icons.push((kind, payload, size));
        }
    }
    Ok((data, icons))
}

// Decodes every icon as a frame in file order, leaving out those that fail. The largest frame
// has the most pixels.
pub(super) fn decode(data: &[u8]) -> Result<Image> {
    let (data, icons) = icons(data)?;
    let mut frames = Vec::new();
    let mut largest: Option<(usize, u32)> = None;
    let mut error = None;
    // All frames share the decoding budget.
    let mut budget = Budget::default();
    for icon in icons {
        let size = icon.2;
        match decode_icon(data, icon) {
            Ok(mut image) => {
                budget.claim(image.frames[0].bitmap.data.len())?;
                if largest.is_none_or(|(_, largest)| size > largest) {
                    largest = Some((frames.len(), size));
                }
                let mut frame = image.frames.swap_remove(0);
                frame.delay = Duration::ZERO;
                frames.push(frame);
            }
            Err(icon_error) => _ = error.get_or_insert(icon_error),
        }
    }
    let Some((largest_frame, _)) = largest else {
        return Err(error.unwrap_or(DecodeError::UnsupportedFeature));
    };
    Ok(Image {
        format: Format::Icns,
        frames,
        largest_frame,
        is_animated: false,
        loop_count: LoopCount::Finite(1),
        metadata: Vec::new(),
    })
}

// Decodes an icon, with the masks of legacy RGB icons from the other entries.
fn decode_icon(data: &[u8], (kind, payload, size): Icon<'_>) -> Result<Image> {
    let decoded = match Format::from_signature(payload) {
        Some(Format::Png) => Some(png::decode(payload)?),
        Some(Format::Jpeg2000) => Some(jpeg2000::decode(payload)?),
        _ => None,
    };
    if let Some(image) = decoded {
        if image.width() != size || image.height() != size {
            return Err(DecodeError::InvalidHeader);
        }
        return Ok(image);
    }
    let (channels, mask_kind): (usize, &[u8]) = match kind {
        b"is32" => (3, b"s8mk"),
        b"il32" => (3, b"l8mk"),
        b"ih32" => (3, b"h8mk"),
        b"it32" => (3, b"t8mk"),
        b"ic04" | b"ic05" => (4, b""),
        _ => return Err(DecodeError::UnsupportedFeature),
    };
    let len = pixel_len(size, size)?;
    let mut payload = payload;
    // Only compressed 128x128 RGB entries carry the four-byte prefix.
    if kind == b"it32" && payload.len() != len {
        if !payload.starts_with(&[0; 4]) {
            return Err(DecodeError::InvalidData);
        }
        payload = &payload[4..];
    }
    if channels == 4 {
        payload = payload
            .strip_prefix(b"ARGB")
            .ok_or(DecodeError::InvalidData)?;
    }
    let count = len / 4;
    let mut budget = Budget::default();
    let mut pixels = budget.zeroed::<u8>(len)?;
    // Uncompressed legacy RGB uses interleaved XRGB bytes. Compressed data is planar.
    if channels == 3 && payload.len() == len {
        for (source, target) in payload
            .as_chunks::<4>()
            .0
            .iter()
            .zip(pixels.as_chunks_mut::<4>().0)
        {
            target.copy_from_slice(&[source[1], source[2], source[3], 255]);
        }
    } else {
        let mut r = Reader::new(payload);
        for channel in 0..channels {
            let output_channel = if channels == 4 {
                (channel + 3) % 4
            } else {
                channel
            };
            let mut position = 0;
            while position < count {
                let control = r.byte()?;
                let run = if control < 128 {
                    usize::from(control) + 1
                } else {
                    usize::from(control) - 125
                };
                if run > count - position {
                    return Err(DecodeError::InvalidData);
                }
                if control < 128 {
                    for value in r.take(run)? {
                        pixels[position * 4 + output_channel] = *value;
                        position += 1;
                    }
                } else {
                    let value = r.byte()?;
                    for _ in 0..run {
                        pixels[position * 4 + output_channel] = value;
                        position += 1;
                    }
                }
            }
        }
        if r.remaining() != 0 {
            return Err(DecodeError::InvalidData);
        }
    }
    if channels == 3 {
        let mut mask = None;
        let mut r = Reader::new(data);
        while r.remaining() != 0 {
            let (kind, payload) = entry(&mut r)?;
            if kind == mask_kind {
                if payload.len() != count {
                    return Err(DecodeError::InvalidData);
                }
                mask = Some(payload);
            }
        }
        for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            pixel[3] = mask.map_or(255, |mask| mask[i]);
        }
    }
    Ok(Image::still(Format::Icns, size, size, pixels))
}

// MARK: Tests

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(entries: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut bytes = b"icns".to_vec();
        let length = 8 + entries
            .iter()
            .map(|(_, data)| data.len() + 8)
            .sum::<usize>();
        bytes.extend_from_slice(&(length as u32).to_be_bytes());
        for (kind, data) in entries {
            bytes.extend_from_slice(*kind);
            bytes.extend_from_slice(&((data.len() + 8) as u32).to_be_bytes());
            bytes.extend_from_slice(data);
        }
        bytes
    }

    fn png(size: u32) -> Vec<u8> {
        let bitmap =
            crate::Bitmap::new(size, size, [16, 32, 64, 128].repeat((size * size) as usize))
                .expect("bitmap");
        crate::encode(&bitmap, Format::Png).expect("PNG")
    }

    #[test]
    fn selects_largest_pixel_dimensions_including_retina() {
        for entries in [
            vec![(b"icp4", vec![0]), (b"icp5", png(32))],
            vec![(b"icp5", png(32)), (b"icp4", vec![0])],
            vec![(b"icp5", vec![0]), (b"ic12", png(64))],
        ] {
            let expected = if entries[1].0 == b"ic12" { 64 } else { 32 };
            let image = crate::decode(&icon(&entries)).expect("ICNS");
            assert_eq!(image.format(), Format::Icns);
            assert_eq!((image.width(), image.height()), (expected, expected));
            assert_eq!(
                image.pixels(),
                [16, 32, 64, 128].repeat((expected * expected) as usize)
            );
            assert_eq!(image.frames().len(), 1);
            assert!(!image.is_animated());
            assert_eq!(image.frames()[0].delay(), Duration::ZERO);
        }
    }

    #[test]
    fn frames_are_every_icon_in_file_order() {
        let rgb = [plane(16), plane(32), plane(64)].concat();
        let data = icon(&[
            (b"ic12", png(64)),
            (b"info", vec![0]),
            (b"is32", rgb),
            (b"s8mk", vec![128; 256]),
            (b"icp4", vec![0]),
            (b"icp5", png(32)),
        ]);
        let image = crate::decode(&data).expect("ICNS");
        let sizes: Vec<_> = image
            .frames()
            .iter()
            .map(|frame| frame.bitmap().width())
            .collect();
        assert_eq!(sizes, [64, 16, 32]);
        assert_eq!((image.width(), image.height()), (64, 64));
        assert_eq!(image.frames()[1].pixels(), [16, 32, 64, 128].repeat(256));
        assert_eq!(
            crate::decode(&icon(&[(b"info", vec![0])])),
            Err(DecodeError::UnsupportedFeature)
        );
    }

    #[test]
    fn largest_jpeg2000_entry_decodes() {
        let payload = include_bytes!("../tests/images/jpeg2000/rgba.jp2");
        let expected = jpeg2000::decode(payload).expect("JP2");
        let image = crate::decode(&icon(&[(b"icp4", png(16)), (b"icp5", payload.to_vec())]))
            .expect("JPEG 2000 ICNS");
        assert_eq!(image.format(), Format::Icns);
        assert_eq!((image.width(), image.height()), (32, 32));
        assert_eq!(image.pixels(), expected.pixels());
        assert!(!image.is_animated());
        assert_eq!(image.frames()[0].delay(), Duration::ZERO);
        assert!(crate::decode(&icon(&[(b"icp4", payload.to_vec())])).is_err());
    }

    fn plane(value: u8) -> Vec<u8> {
        // Two 128-pixel repetitions fill one 16x16 plane.
        vec![253, value, 253, value]
    }

    #[test]
    fn legacy_rgb_and_masks_decode_in_either_order() {
        let rgb = [plane(16), plane(32), plane(64)].concat();
        for entries in [
            vec![(b"is32", rgb.clone()), (b"s8mk", vec![128; 256])],
            vec![(b"s8mk", vec![128; 256]), (b"is32", rgb)],
            vec![
                (b"is32", [0, 16, 32, 64].repeat(256)),
                (b"s8mk", vec![128; 256]),
            ],
        ] {
            let image = crate::decode(&icon(&entries)).expect("legacy ICNS");
            assert_eq!(image.pixels(), [16, 32, 64, 128].repeat(256));
        }
        let image = crate::decode(&icon(&[(
            b"is32",
            [plane(16), plane(32), plane(64)].concat(),
        )]))
        .expect("opaque ICNS");
        assert_eq!(image.pixels(), [16, 32, 64, 255].repeat(256));
    }

    #[test]
    fn uncompressed_128_pixel_rgb_has_no_reserved_prefix() {
        let image = crate::decode(&icon(&[
            (b"it32", [0, 255, 128, 64].repeat(128 * 128)),
            (b"t8mk", vec![128; 128 * 128]),
        ]))
        .expect("uncompressed 128x128 ICNS");
        assert_eq!((image.width(), image.height()), (128, 128));
        assert_eq!(image.pixels(), [255, 128, 64, 128].repeat(128 * 128));
    }

    #[test]
    fn argb_planes_decode() {
        let data = [
            b"ARGB".to_vec(),
            plane(128),
            plane(16),
            plane(32),
            plane(64),
        ]
        .concat();
        let image = crate::decode(&icon(&[(b"ic04", data)])).expect("ARGB ICNS");
        assert_eq!(image.pixels(), [16, 32, 64, 128].repeat(256));
    }

    #[test]
    fn detects_icns_and_rejects_encoding() {
        assert_eq!(Format::from_extension("ICNS"), Some(Format::Icns));
        assert_eq!(Format::from_signature(b"icns"), Some(Format::Icns));
        assert!(!Format::Icns.can_encode());
        let bitmap = crate::Bitmap::new(1, 1, vec![0; 4]).expect("bitmap");
        assert_eq!(
            crate::encode(&bitmap, Format::Icns),
            Err(crate::EncodeError::UnsupportedFormat)
        );
    }

    #[test]
    fn malformed_entries_and_unsupported_payloads_fail() {
        for bytes in [
            b"icns\0\0\0\x07".to_vec(),
            b"icns\0\0\0\x10icp4\0\0\0\0".to_vec(),
            icon(&[(b"is32", vec![255, 0])]),
            icon(&[
                (b"is32", [plane(16), plane(32), plane(64)].concat()),
                (b"s8mk", vec![0]),
            ]),
            icon(&[(b"icp4", png(32))]),
            icon(&[(b"icp4", vec![0])]),
        ] {
            assert!(crate::decode(&bytes).is_err());
        }
        let bytes = icon(&[(b"icp4", png(16))]);
        for end in 4..bytes.len() {
            assert!(crate::decode(&bytes[..end]).is_err(), "length {end}");
        }
    }
}
