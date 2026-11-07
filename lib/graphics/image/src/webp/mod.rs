/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;
use core::time::Duration;

use super::color::{ColorModel, Transform};
use super::metadata::{ExifData, orient};
use super::{Budget, DecodeError, Format, Image, LoopCount, Result, pixel_len};

mod lossless;
mod lossy;

type Pixel = [u8; 4];

struct Chunks<'a> {
    data: &'a [u8],
}
impl<'a> Chunks<'a> {
    fn next(&mut self) -> Result<Option<(&'a [u8], &'a [u8])>> {
        if self.data.is_empty() {
            return Ok(None);
        }
        let header = self.data.get(..8).ok_or(DecodeError::InvalidData)?;
        let length = u32::from_le_bytes(header[4..8].try_into().expect("four bytes")) as usize;
        let end = length.checked_add(8).ok_or(DecodeError::ImageTooLarge)?;
        let payload = self.data.get(8..end).ok_or(DecodeError::InvalidData)?;
        self.data = self
            .data
            .get(end + (length & 1)..)
            .ok_or(DecodeError::InvalidData)?;
        Ok(Some((&header[..4], payload)))
    }
}

fn le24(bytes: &[u8]) -> Result<usize> {
    let bytes = bytes.get(..3).ok_or(DecodeError::InvalidData)?;
    Ok(usize::from(bytes[0]) | (usize::from(bytes[1]) << 8) | (usize::from(bytes[2]) << 16))
}

fn bitmap(data: &[u8], budget: &mut Budget) -> Result<(usize, usize, Vec<Pixel>)> {
    let mut chunks = Chunks { data };
    let mut image = None;
    let mut alpha = None;
    while let Some((kind, payload)) = chunks.next()? {
        match kind {
            b"ALPH" if alpha.is_none() => alpha = Some(payload),
            b"VP8 " if image.is_none() => image = Some(lossy::decode(payload, budget)?),
            b"VP8L" if image.is_none() => image = Some(lossless::decode(payload, budget)?),
            _ => return Err(DecodeError::InvalidData),
        }
    }
    let (width, height, mut pixels) = image.ok_or(DecodeError::InvalidData)?;
    if let Some(data) = alpha {
        let (&flags, data) = data.split_first().ok_or(DecodeError::InvalidData)?;
        if flags & 0xc0 != 0 || flags & 3 > 1 {
            return Err(DecodeError::UnsupportedFeature);
        }
        let mut values = budget.zeroed::<u8>(width * height)?;
        if flags & 3 == 0 {
            if data.len() != values.len() {
                return Err(DecodeError::InvalidData);
            }
            values.copy_from_slice(data);
        } else {
            let decoded = lossless::alpha(data, width, height, budget)?;
            for (value, p) in values.iter_mut().zip(decoded) {
                *value = p[1];
            }
        }
        let filter = (flags >> 2) & 3;
        for y in 0..height {
            for x in 0..width {
                let i = y * width + x;
                let prediction = if filter == 0 {
                    0
                } else if y == 0 {
                    if x == 0 { 0 } else { values[i - 1] }
                } else if x == 0 {
                    values[i - width]
                } else {
                    match filter {
                        1 => values[i - 1],
                        2 => values[i - width],
                        _ => (i32::from(values[i - 1]) + i32::from(values[i - width])
                            - i32::from(values[i - width - 1]))
                        .clamp(0, 255) as u8,
                    }
                };
                values[i] = values[i].wrapping_add(prediction);
                pixels[i][3] = values[i];
            }
        }
    }
    Ok((width, height, pixels))
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    if data.get(..4) != Some(b"RIFF") || data.get(8..12) != Some(b"WEBP") {
        return Err(DecodeError::InvalidHeader);
    }
    let length = u32::from_le_bytes(data[4..8].try_into().expect("four bytes")) as usize;
    let end = length.checked_add(8).ok_or(DecodeError::ImageTooLarge)?;
    let data = data.get(12..end).ok_or(DecodeError::InvalidData)?;
    let mut budget = Budget::default();
    budget.claim(data.len())?;
    let mut orientation = 1;
    let mut dimensions = None;
    let mut transform = None;
    let mut loop_count = LoopCount::Finite(1);
    let mut animated = false;
    let mut chunks = Chunks { data };
    while let Some((kind, payload)) = chunks.next()? {
        match kind {
            b"VP8X" => {
                if payload.len() != 10 || payload[0] & 0xc1 != 0 || payload[1..4] != [0; 3] {
                    return Err(DecodeError::InvalidHeader);
                }
                let (width, height) = (le24(&payload[4..])? + 1, le24(&payload[7..])? + 1);
                pixel_len(width as u32, height as u32)?;
                dimensions = Some((width, height));
                animated = payload[0] & 2 != 0;
            }
            b"EXIF" => {
                let data = payload.strip_prefix(b"Exif\0\0").unwrap_or(payload);
                if let Some(value) = ExifData::new(data).and_then(|exif| exif.orientation()) {
                    orientation = value;
                }
            }
            b"ICCP" => transform = Transform::icc(payload, ColorModel::Rgb),
            b"ANIM" if animated => {
                if payload.len() != 6 {
                    return Err(DecodeError::InvalidData);
                }
                let count = u16::from_le_bytes([payload[4], payload[5]]);
                loop_count = if count == 0 {
                    LoopCount::Infinite
                } else {
                    LoopCount::Finite(u32::from(count))
                };
            }
            _ => {}
        }
    }
    let mut frames = Vec::new();
    if !animated {
        let mut bytes = Vec::new();
        let mut chunks = Chunks { data };
        while let Some((kind, payload)) = chunks.next()? {
            if matches!(kind, b"ALPH" | b"VP8 " | b"VP8L") {
                budget.claim(payload.len() + 9)?;
                bytes.extend_from_slice(kind);
                bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                bytes.extend_from_slice(payload);
                if payload.len() & 1 != 0 {
                    bytes.push(0);
                }
            }
        }
        let (width, height, mut pixels) = bitmap(&bytes, &mut budget)?;
        if dimensions.is_some_and(|size| size != (width, height)) {
            return Err(DecodeError::InvalidHeader);
        }
        if let Some(t) = transform {
            t.apply(pixels.as_flattened_mut());
        }
        let (width, height, pixels) = orient(
            width as u32,
            height as u32,
            pixels.into_flattened(),
            orientation,
            &mut budget,
        )?;
        return Ok(Image::still(Format::WebP, width, height, pixels));
    }
    let (width, height) = dimensions.ok_or(DecodeError::InvalidHeader)?;
    let len = pixel_len(width as u32, height as u32)?;
    let mut canvas = budget.zeroed::<Pixel>(len / 4)?;
    let mut chunks = Chunks { data };
    while let Some((kind, payload)) = chunks.next()? {
        if kind != b"ANMF" {
            continue;
        }
        let header = payload.get(..16).ok_or(DecodeError::InvalidData)?;
        let x = le24(header)? * 2;
        let y = le24(&header[3..])? * 2;
        let w = le24(&header[6..])? + 1;
        let h = le24(&header[9..])? + 1;
        let delay = Duration::from_millis(le24(&header[12..])? as u64);
        if header[15] & !3 != 0 || w > width || h > height || x > width - w || y > height - h {
            return Err(DecodeError::InvalidData);
        }
        let (fw, fh, frame) = bitmap(&payload[16..], &mut budget)?;
        if (fw, fh) != (w, h) {
            return Err(DecodeError::InvalidData);
        }
        for row in 0..h {
            for column in 0..w {
                let p = &mut canvas[(y + row) * width + x + column];
                let source = frame[row * w + column];
                *p = if header[15] & 2 != 0 {
                    source
                } else {
                    blend(source, *p)
                };
            }
        }
        let mut output = budget.zeroed::<u8>(len)?;
        output.copy_from_slice(canvas.as_flattened());
        if let Some(t) = &transform {
            t.apply(&mut output);
        }
        let (frame_width, frame_height, output) = orient(
            width as u32,
            height as u32,
            output,
            orientation,
            &mut budget,
        )?;
        budget.frame(&mut frames, frame_width, frame_height, output, delay)?;
        if header[15] & 1 != 0 {
            for row in 0..h {
                // Match libwebp and browsers: disposal clears to transparent, regardless
                // of the optional animation background color.
                canvas[(y + row) * width + x..(y + row) * width + x + w].fill([0; 4]);
            }
        }
    }
    if frames.is_empty() {
        return Err(DecodeError::InvalidData);
    }
    Ok(Image {
        format: Format::WebP,
        frames,
        largest_frame: 0,
        is_animated: true,
        loop_count,
        text: Vec::new(),
    })
}

fn blend(source: Pixel, target: Pixel) -> Pixel {
    let a = u32::from(source[3]);
    let b = u32::from(target[3]) * (255 - a);
    let alpha = a * 255 + b;
    if alpha == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for i in 0..3 {
        out[i] =
            ((u32::from(source[i]) * a * 255 + u32::from(target[i]) * b + alpha / 2) / alpha) as u8;
    }
    out[3] = ((alpha + 127) / 255) as u8;
    out
}
// MARK: Tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_lossless_and_alpha_match_libwebp() {
        for name in ["lossless", "lossy", "lossless-alpha", "lossy-alpha"] {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
            let bytes =
                std::fs::read(root.join(format!("images/webp/{name}.webp"))).expect("WebP fixture");
            let image = crate::decode(&bytes).expect("WebP image");
            assert_eq!(image.format(), Format::WebP);
            assert_eq!((image.width(), image.height()), (16, 16));
            assert!(!image.is_animated());
            assert_eq!(image.frames().len(), 1);
            assert_eq!(image.frames()[0].delay(), Duration::ZERO);
            assert_eq!(image.loop_count(), LoopCount::Finite(1));
            let reference = std::fs::read(root.join(format!("reference/webp/{name}.webp.rgba")))
                .expect("libwebp reference");
            assert_eq!(image.pixels().len(), reference.len(), "{name}");
            for (index, (actual, expected)) in image.pixels().iter().zip(&reference).enumerate() {
                // VP8 implementations can differ slightly in YUV rounding/upsampling.
                let tolerance = if name.starts_with("lossy") { 3 } else { 0 };
                assert!(
                    actual.abs_diff(*expected) <= tolerance,
                    "{name}: {actual} vs {expected} at sample {index}"
                );
            }
        }
    }

    #[test]
    fn odd_sizes_modes_and_palettes_match_references() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        for name in ["odd", "noise", "simple", "palette", "flat"] {
            let actual = crate::decode(
                &std::fs::read(root.join(format!("images/webp/{name}.webp"))).expect("WebP"),
            )
            .expect("decode WebP");
            let expected = std::fs::read(root.join(format!("reference/webp/{name}.webp.rgba")))
                .expect("libwebp reference");
            assert_eq!(actual.pixels().len(), expected.len(), "{name}");
            let limit = if matches!(name, "palette" | "flat") {
                0
            } else {
                3
            };
            for (index, (a, b)) in actual.pixels().iter().zip(&expected).enumerate() {
                assert!(a.abs_diff(*b) <= limit, "{name} sample {index}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn animation_keeps_frames_timings_and_loops() {
        let image = crate::decode(include_bytes!("../../tests/images/webp/animated.webp"))
            .expect("animated WebP");
        assert!(image.is_animated());
        assert_eq!(image.frames().len(), 2);
        assert_eq!(image.frames()[0].delay(), Duration::from_millis(100));
        assert_eq!(image.frames()[1].delay(), Duration::from_millis(200));
        assert_eq!(image.loop_count(), LoopCount::Finite(3));
        let first = crate::decode(include_bytes!("../../tests/images/webp/lossless.webp"))
            .expect("first frame");
        // The decoder's integer alpha compositing can round a channel down by one.
        assert!(
            image.frames()[0]
                .pixels()
                .iter()
                .zip(first.pixels())
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
    }

    #[test]
    fn disposed_ball_frames_stay_transparent_despite_opaque_background() {
        let bytes = include_bytes!("../../../../../apps/desktop/imageview/examples/ball.webp");
        let image = crate::decode(bytes).expect("animated ball WebP");
        assert_eq!((image.width(), image.height()), (100, 100));
        assert_eq!(image.frames().len(), 20);
        assert_eq!(image.loop_count(), LoopCount::Infinite);
        // The first rectangle extends above the second. Disposal must leave that
        // strip transparent, even though ANIM declares an opaque white background.
        for frame in &image.frames()[1..4] {
            for y in 30..34 {
                for x in 30..69 {
                    assert_eq!(frame.pixels()[(y * 100 + x) * 4 + 3], 0);
                }
            }
        }
        for frame in image.frames() {
            assert_eq!(frame.delay(), Duration::from_millis(75));
        }
        // Twenty composited 100x100 RGBA frames, decoded with libwebp's WebPAnimDecoder and
        // stacked vertically.
        let reference = include_bytes!("../../tests/reference/webp/ball.webp.rgba");
        assert_eq!(reference.len(), 20 * 100 * 100 * 4);
        for (index, (frame, expected)) in image
            .frames()
            .iter()
            .zip(reference.as_chunks::<{ 100 * 100 * 4 }>().0)
            .enumerate()
        {
            assert_eq!(frame.pixels(), expected, "frame {index}");
        }
        // The animation background is a display hint and must not change disposal.
        let mut changed_background = bytes.to_vec();
        let anim = bytes
            .windows(4)
            .position(|kind| kind == b"ANIM")
            .expect("animation header");
        changed_background[anim + 8..anim + 12].copy_from_slice(&[31, 63, 127, 255]);
        assert_eq!(
            crate::decode(&changed_background).expect("colored background"),
            image
        );
    }

    #[test]
    fn embedded_icc_is_converted_to_srgb() {
        let image = crate::decode(include_bytes!("../../tests/images/webp/icc-p3.webp"))
            .expect("WebP with ICC");
        assert_eq!(
            image.pixels(),
            include_bytes!("../../tests/reference/color/png/icc-p3-v4.png.rgba")
        );
    }

    fn with_exif(source: &[u8], metadata: &[u8]) -> Vec<u8> {
        let image = crate::decode(source).expect("source WebP");
        let mut bytes = source.to_vec();
        if &bytes[12..16] != b"VP8X" {
            let mut header = b"VP8X\x0a\0\0\0\x08\0\0\0".to_vec();
            header.extend_from_slice(&(image.width() - 1).to_le_bytes()[..3]);
            header.extend_from_slice(&(image.height() - 1).to_le_bytes()[..3]);
            bytes.splice(12..12, header);
        } else {
            bytes[20] |= 8;
        }
        bytes.extend_from_slice(b"EXIF");
        bytes.extend_from_slice(&(metadata.len() as u32).to_le_bytes());
        bytes.extend_from_slice(metadata);
        if metadata.len() & 1 != 0 {
            bytes.push(0);
        }
        let length = bytes.len() as u32 - 8;
        bytes[4..8].copy_from_slice(&length.to_le_bytes());
        bytes
    }

    #[test]
    fn exif_orients_still_images_and_composited_animation_frames() {
        for source in [
            include_bytes!("../../tests/images/webp/odd.webp").as_slice(),
            include_bytes!("../../tests/images/webp/animated.webp").as_slice(),
        ] {
            let expected = crate::decode(source).expect("source WebP");
            for little in [false, true] {
                for prefix in [false, true] {
                    for orientation in 1u16..=8 {
                        let mut exif = if prefix {
                            b"Exif\0\0".to_vec()
                        } else {
                            Vec::new()
                        };
                        if little {
                            exif.extend_from_slice(
                                b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0",
                            );
                            exif.extend_from_slice(&orientation.to_le_bytes());
                            exif.extend_from_slice(&[0; 6]);
                        } else {
                            exif.extend_from_slice(
                                b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01",
                            );
                            exif.extend_from_slice(&orientation.to_be_bytes());
                            exif.extend_from_slice(&[0; 6]);
                        }
                        let actual =
                            crate::decode(&with_exif(source, &exif)).expect("oriented WebP");
                        let (w, h) = (expected.width(), expected.height());
                        let size = if orientation >= 5 { (h, w) } else { (w, h) };
                        assert_eq!((actual.width(), actual.height()), size);
                        assert_eq!(actual.frames().len(), expected.frames().len());
                        assert_eq!(actual.is_animated(), expected.is_animated());
                        assert_eq!(actual.loop_count(), expected.loop_count());
                        for (actual, expected) in actual.frames().iter().zip(expected.frames()) {
                            assert_eq!(actual.delay(), expected.delay());
                            for y in 0..size.1 {
                                for x in 0..size.0 {
                                    let (sx, sy) = match orientation {
                                        1 => (x, y),
                                        2 => (w - 1 - x, y),
                                        3 => (w - 1 - x, h - 1 - y),
                                        4 => (x, h - 1 - y),
                                        5 => (y, x),
                                        6 => (y, h - 1 - x),
                                        7 => (w - 1 - y, h - 1 - x),
                                        8 => (w - 1 - y, x),
                                        _ => unreachable!(),
                                    };
                                    let a = ((y * size.0 + x) * 4) as usize;
                                    let b = ((sy * w + sx) * 4) as usize;
                                    assert_eq!(
                                        &actual.pixels()[a..a + 4],
                                        &expected.pixels()[b..b + 4]
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn malformed_or_invalid_exif_orientation_is_ignored() {
        let source = include_bytes!("../../tests/images/webp/odd.webp");
        let expected = crate::decode(source).expect("source WebP");
        for metadata in [b"".as_slice(), b"Exif\0\0", b"MM\0\x2a\xff\xff\xff\xff"] {
            assert_eq!(
                crate::decode(&with_exif(source, metadata)).expect("WebP"),
                expected
            );
        }
        for value in [0u16, 9, 65535] {
            let mut metadata = b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
            metadata.extend_from_slice(&value.to_be_bytes());
            metadata.extend_from_slice(&[0; 6]);
            assert_eq!(
                crate::decode(&with_exif(source, &metadata)).expect("WebP"),
                expected
            );
        }
    }

    #[test]
    fn detects_webp_and_rejects_encoding() {
        assert_eq!(Format::from_extension("WEBP"), Some(Format::WebP));
        assert_eq!(Format::from_signature(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(
            Format::from_signature(b"RIFF\0\0\0\0WEBP"),
            Some(Format::WebP)
        );
        assert!(!Format::WebP.can_encode());
        let bitmap = crate::Bitmap::new(1, 1, vec![0; 4]).expect("bitmap");
        assert_eq!(
            crate::encode(&bitmap, Format::WebP),
            Err(crate::EncodeError::UnsupportedFormat)
        );
    }

    #[test]
    fn truncated_and_invalid_webp_fail() {
        let bytes = include_bytes!("../../tests/images/webp/lossless.webp");
        for end in 12..bytes.len() {
            assert!(crate::decode(&bytes[..end]).is_err(), "length {end}");
        }
        assert!(crate::decode(b"RIFF\x04\0\0\0WEBP").is_err());
        assert!(crate::decode(b"RIFF\xff\xff\xff\xffWEBP").is_err());
        let mut oversized = include_bytes!("../../tests/images/webp/animated.webp").to_vec();
        assert_eq!(&oversized[12..16], b"VP8X");
        oversized[24..30].fill(255);
        assert_eq!(crate::decode(&oversized), Err(DecodeError::ImageTooLarge));
    }
}

#[cfg(test)]
mod corruption_tests {
    #[test]
    fn damaged_webp_does_not_panic() {
        for source in [
            include_bytes!("../../tests/images/webp/noise.webp").as_slice(),
            include_bytes!("../../tests/images/webp/palette.webp").as_slice(),
            include_bytes!("../../tests/images/webp/animated.webp").as_slice(),
        ] {
            for index in (0..source.len()).step_by(11) {
                let mut data = source.to_vec();
                data[index] ^= 255;
                let _ = crate::decode(&data);
            }
        }
    }
}
