/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use core::time::Duration;

use super::{Budget, DecodeError, Format, Image, LoopCount, Reader, Result, bmp, png};

// MARK: Decoder
#[derive(Clone, Copy)]
struct Entry {
    width: u32,
    height: u32,
    colors: usize,
    depth: u16,
    size: usize,
    offset: usize,
}

impl Entry {
    const fn rank(self) -> (u64, u16, usize) {
        (
            self.width as u64 * self.height as u64,
            self.depth,
            self.size,
        )
    }
}

// Reads the directory of the icons in the file.
fn entries(data: &[u8]) -> Result<Vec<Entry>> {
    let mut r = Reader::new(data);
    if r.le16()? != 0 || r.le16()? != 1 {
        return Err(DecodeError::InvalidHeader);
    }
    let count = r.le16()?;
    if count == 0 {
        return Err(DecodeError::InvalidHeader);
    }
    (0..count)
        .map(|_| {
            let width = dimension(r.byte()?);
            let height = dimension(r.byte()?);
            let colors = r.byte()? as usize;
            if r.byte()? != 0 {
                return Err(DecodeError::InvalidHeader);
            }
            r.le16()?; // Color planes.
            Ok(Entry {
                width,
                height,
                colors,
                depth: r.le16()?,
                size: r.le32()? as usize,
                offset: r.le32()? as usize,
            })
        })
        .collect()
}

// Decodes every icon as a frame in file order, leaving out those that fail. The largest frame
// has the most pixels, then the most bits per pixel.
pub(super) fn decode(data: &[u8]) -> Result<Image> {
    let mut frames = Vec::new();
    let mut largest: Option<(usize, Entry)> = None;
    let mut error = None;
    // All frames share the decoding budget, entries with the same data are decoded once.
    let mut budget = Budget::default();
    let mut decoded = BTreeSet::new();
    for entry in entries(data)? {
        if !decoded.insert((entry.offset, entry.size)) {
            continue;
        }
        match decode_entry(data, entry) {
            Ok(mut image) => {
                budget.claim(image.frames[0].bitmap.data.len())?;
                if largest.is_none_or(|(_, largest)| entry.rank() > largest.rank()) {
                    largest = Some((frames.len(), entry));
                }
                let mut frame = image.frames.swap_remove(0);
                frame.delay = Duration::ZERO;
                frames.push(frame);
            }
            Err(entry_error) => _ = error.get_or_insert(entry_error),
        }
    }
    let Some((largest_frame, _)) = largest else {
        return Err(error.unwrap_or(DecodeError::InvalidHeader));
    };
    Ok(Image {
        format: Format::Ico,
        frames,
        largest_frame,
        is_animated: false,
        loop_count: LoopCount::Finite(1),
        metadata: Vec::new(),
    })
}

fn decode_entry(data: &[u8], entry: Entry) -> Result<Image> {
    let end = entry
        .offset
        .checked_add(entry.size)
        .ok_or(DecodeError::InvalidData)?;
    let payload = data
        .get(entry.offset..end)
        .filter(|bytes| !bytes.is_empty())
        .ok_or(DecodeError::InvalidData)?;
    if payload.starts_with(b"\x89PNG\r\n\x1a\n") {
        let image = png::decode(payload)?;
        if image.width() != entry.width || image.height() != entry.height {
            return Err(DecodeError::InvalidHeader);
        }
        Ok(image)
    } else {
        bmp::decode_icon(payload, entry.width, entry.height, entry.colors)
    }
}

const fn dimension(value: u8) -> u32 {
    if value == 0 { 256 } else { value as u32 }
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    fn icon(entries: &[(u8, u8, u16, Vec<u8>)]) -> Vec<u8> {
        let mut out = vec![0, 0, 1, 0];
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        let mut offset = 6 + entries.len() * 16;
        for (width, height, depth, payload) in entries {
            out.extend_from_slice(&[*width, *height, 0, 0]);
            out.extend_from_slice(&1u16.to_le_bytes());
            out.extend_from_slice(&depth.to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += payload.len();
        }
        for (_, _, _, payload) in entries {
            out.extend_from_slice(payload);
        }
        out
    }

    fn dib32(alpha: [u8; 2], mask: u8) -> Vec<u8> {
        let mut out = 40u32.to_le_bytes().to_vec();
        out.extend_from_slice(&2i32.to_le_bytes());
        out.extend_from_slice(&2i32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&[0; 16]);
        out.extend_from_slice(&[0, 0, 255, alpha[0], 0, 255, 0, alpha[1]]);
        out.extend_from_slice(&[mask, 0, 0, 0]);
        out
    }

    #[test]
    fn dib_alpha_and_legacy_mask() {
        let image = decode(&icon(&[(2, 1, 32, dib32([128, 255], 0x40))])).expect("alpha icon");
        assert_eq!(image.format(), Format::Ico);
        assert_eq!(image.pixels(), &[255, 0, 0, 128, 0, 255, 0, 255]);

        let image = decode(&icon(&[(2, 1, 32, dib32([0, 0], 0x40))])).expect("masked icon");
        assert_eq!(image.pixels(), &[255, 0, 0, 255, 0, 0, 0, 0]);
    }

    #[test]
    fn dib_without_alpha_requires_a_complete_mask() {
        let mut dib = dib32([0, 0], 0);
        dib.truncate(dib.len() - 4);
        assert_eq!(
            decode(&icon(&[(2, 1, 32, dib)])),
            Err(DecodeError::InvalidData)
        );
    }

    #[test]
    fn largest_png_is_the_only_payload_decoded() {
        let png = include_bytes!("../tests/images/png/interlaced/basi2c08.png").to_vec();
        let expected = png::decode(&png).expect("PNG payload");
        let image = decode(&icon(&[(1, 1, 32, vec![0]), (32, 32, 32, png)])).expect("PNG icon");
        assert_eq!(image.frames().len(), 1);
        assert_eq!((image.width(), image.height()), (32, 32));
        assert_eq!(image.format(), Format::Ico);
        assert_eq!(image.pixels(), expected.pixels());
    }

    #[test]
    fn frames_are_every_icon_in_file_order() {
        let png = include_bytes!("../tests/images/png/interlaced/basi2c08.png").to_vec();
        let data = icon(&[
            (2, 1, 32, dib32([128, 255], 0)),
            (32, 32, 32, png),
            (1, 1, 32, vec![0]),
            (32, 32, 24, dib32([255, 255], 0)),
        ]);
        let image = decode(&data).expect("ICO");
        let sizes: Vec<_> = image
            .frames()
            .iter()
            .map(|frame| (frame.bitmap().width(), frame.bitmap().height()))
            .collect();
        // The broken icon is left out, of equally large ones the deepest is the largest.
        assert_eq!(sizes, [(2, 1), (32, 32)]);
        assert_eq!((image.width(), image.height()), (32, 32));
        assert_eq!(image.pixels(), image.frames()[1].pixels());
        assert!(!image.is_animated());
        assert_eq!(
            decode(&icon(&[(1, 1, 32, vec![0])])),
            Err(DecodeError::InvalidData)
        );
    }

    #[test]
    fn entries_with_the_same_data_are_one_frame() {
        let dib = dib32([128, 255], 0);
        let data = icon(&[(2, 1, 32, dib.clone()), (2, 1, 32, dib)]);
        // Point the second entry at the data of the first.
        let mut data = data;
        let first_offset = data[18..22].to_vec();
        data[34..38].copy_from_slice(&first_offset);
        assert_eq!(decode(&data).expect("ICO").frames().len(), 1);
    }

    #[test]
    fn directory_entry_reserved_byte_must_be_zero() {
        let mut data = vec![0, 0, 1, 0, 1, 0, 1, 1, 0, 1];
        data.extend_from_slice(&[0; 12]);
        assert_eq!(decode(&data), Err(DecodeError::InvalidHeader));
    }
}
