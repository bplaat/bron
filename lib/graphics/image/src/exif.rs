/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! EXIF metadata of JPEG files. EXIF is stored as directories of tagged values with a byte
//! order mark, the layout of TIFF files.

use alloc::vec::Vec;
use core::ops::Range;

pub(crate) const ORIENTATION: u16 = 0x112;
pub(crate) const EXIF_DIRECTORY: u16 = 0x8769;
pub(crate) const COLOR_SPACE: u16 = 0xa001;
pub(crate) const INTEROPERABILITY_DIRECTORY: u16 = 0xa005;
const PIXEL_WIDTH: u16 = 0xa002;
const PIXEL_HEIGHT: u16 = 0xa003;
pub(crate) const ASCII: u16 = 2;
pub(crate) const SHORT: u16 = 3;
pub(crate) const LONG: u16 = 4;
const SRGB: u32 = 1;

/// How stored pixels are turned to show them, as numbered by the EXIF orientation tag.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Orientation {
    /// Shown as stored.
    #[default]
    Normal = 1,
    /// Mirrored left to right.
    FlipHorizontal = 2,
    /// Turned half around.
    Rotate180 = 3,
    /// Mirrored top to bottom.
    FlipVertical = 4,
    /// Mirrored along the diagonal from the top left.
    Transpose = 5,
    /// Turned a quarter clockwise.
    Rotate90 = 6,
    /// Mirrored along the diagonal from the top right.
    Transverse = 7,
    /// Turned a quarter counterclockwise.
    Rotate270 = 8,
}

const ORIENTATIONS: [Orientation; 8] = [
    Orientation::Normal,
    Orientation::FlipHorizontal,
    Orientation::Rotate180,
    Orientation::FlipVertical,
    Orientation::Transpose,
    Orientation::Rotate90,
    Orientation::Transverse,
    Orientation::Rotate270,
];

impl Orientation {
    /// The orientation of an EXIF tag value.
    pub const fn from_exif(value: u16) -> Option<Self> {
        if value >= 1 && value <= 8 {
            Some(ORIENTATIONS[value as usize - 1])
        } else {
            None
        }
    }

    /// The EXIF tag value.
    pub const fn exif(self) -> u16 {
        self as u16
    }

    // Where the orientation moves a pixel at a position from the middle, x right and y down.
    const fn matrix(self) -> [[i8; 2]; 2] {
        match self {
            Self::Normal => [[1, 0], [0, 1]],
            Self::FlipHorizontal => [[-1, 0], [0, 1]],
            Self::Rotate180 => [[-1, 0], [0, -1]],
            Self::FlipVertical => [[1, 0], [0, -1]],
            Self::Transpose => [[0, 1], [1, 0]],
            Self::Rotate90 => [[0, -1], [1, 0]],
            Self::Transverse => [[0, -1], [-1, 0]],
            Self::Rotate270 => [[0, 1], [-1, 0]],
        }
    }

    /// This orientation followed by another one.
    pub fn then(self, next: Self) -> Self {
        let (a, b) = (next.matrix(), self.matrix());
        let combined: [[i8; 2]; 2] = core::array::from_fn(|row| {
            core::array::from_fn(|column| a[row][0] * b[0][column] + a[row][1] * b[1][column])
        });
        ORIENTATIONS
            .into_iter()
            .find(|orientation| orientation.matrix() == combined)
            .unwrap_or_default()
    }
}

// A metadata segment of a JPEG file: its marker and contents.
struct Segment {
    marker: u8,
    contents: Range<usize>,
}

// The segments before the pixel data, or `None` for a file that is not a JPEG.
fn segments(jpeg: &[u8]) -> Option<Vec<Segment>> {
    if !jpeg.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut segments = Vec::new();
    let mut position = 2;
    while let Some(&[0xff, marker, high, low]) = jpeg.get(position..position + 4) {
        // Pixel data follows the start of scan, without metadata segments.
        if marker == 0xda || marker == 0xd9 {
            break;
        }
        let end = position + 2 + usize::from(u16::from_be_bytes([high, low]));
        jpeg.get(position + 4..end)?;
        segments.push(Segment {
            marker,
            contents: position + 4..end,
        });
        position = end;
    }
    Some(segments)
}

// The EXIF data of the segments that have it.
fn exif_ranges(jpeg: &[u8], segments: &[Segment]) -> Vec<Range<usize>> {
    segments
        .iter()
        .filter(|segment| {
            segment.marker == 0xe1 && jpeg[segment.contents.clone()].starts_with(b"Exif\0\0")
        })
        .map(|segment| segment.contents.start + 6..segment.contents.end)
        .collect()
}

// Where a new EXIF segment goes: after the JFIF segment, which has to come first.
fn exif_position(jpeg: &[u8], segments: &[Segment]) -> usize {
    segments
        .iter()
        .find(|segment| {
            segment.marker == 0xe0 && jpeg[segment.contents.clone()].starts_with(b"JFIF\0")
        })
        .map_or(2, |segment| segment.contents.end)
}

/// EXIF data in its byte order.
pub(crate) struct ExifData<'a> {
    data: &'a [u8],
    little: bool,
}

impl<'a> ExifData<'a> {
    pub(crate) fn new(data: &'a [u8]) -> Option<Self> {
        let little = match data.get(..2)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let exif = Self { data, little };
        (exif.read(2, 2)? == 42).then_some(exif)
    }

    pub(crate) fn read(&self, at: usize, length: usize) -> Option<u32> {
        let bytes = self.data.get(at..at.checked_add(length)?)?;
        Some(bytes.iter().enumerate().fold(0, |value, (index, byte)| {
            let shift = if self.little {
                index
            } else {
                length - 1 - index
            } * 8;
            value | u32::from(*byte) << shift
        }))
    }

    /// The first image directory.
    pub(crate) fn first_directory(&self) -> Option<usize> {
        self.read(4, 4).map(|offset| offset as usize)
    }

    /// The value position of a single-value entry of a type in a directory.
    pub(crate) fn entry(&self, directory: usize, tag: u16, kind: u16) -> Option<usize> {
        (0..self.read(directory, 2)? as usize).find_map(|index| {
            let entry = directory.checked_add(2 + index * 12)?;
            (self.read(entry, 2)? == u32::from(tag)
                && self.read(entry + 2, 2)? == u32::from(kind)
                && (kind == ASCII || self.read(entry + 4, 4)? == 1))
                .then_some(entry + 8)
        })
    }

    // The value position and length of a number entry, a short or long.
    fn number(&self, directory: usize, tag: u16) -> Option<(usize, usize)> {
        self.entry(directory, tag, SHORT)
            .map(|at| (at, 2))
            .or_else(|| self.entry(directory, tag, LONG).map(|at| (at, 4)))
    }

    /// The value of a number entry, a short or long.
    pub(crate) fn get(&self, directory: usize, tag: u16) -> Option<u32> {
        let (at, length) = self.number(directory, tag)?;
        self.read(at, length)
    }
}

// Writes a number in a byte order.
fn write(data: &mut [u8], at: usize, length: usize, value: u32, little: bool) {
    if let Some(bytes) = data.get_mut(at..at + length) {
        for (index, byte) in bytes.iter_mut().enumerate() {
            let shift = if little { index } else { length - 1 - index } * 8;
            *byte = (value >> shift) as u8;
        }
    }
}

// Changes a number entry of a directory that EXIF data has.
fn set(data: &mut [u8], directory: usize, tag: u16, value: u32) {
    let Some(exif) = ExifData::new(data) else {
        return;
    };
    let little = exif.little;
    if let Some((at, length)) = exif.number(directory, tag) {
        write(data, at, length, value, little);
    }
}

// A big endian EXIF segment with only an orientation.
fn orientation_segment(orientation: Orientation) -> Vec<u8> {
    let mut exif = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
    exif.extend_from_slice(&ORIENTATION.to_be_bytes());
    exif.extend_from_slice(&[0, 3, 0, 0, 0, 1]);
    exif.extend_from_slice(&orientation.exif().to_be_bytes());
    exif.extend_from_slice(&[0; 6]);
    let mut segment = alloc::vec![0xff, 0xe1];
    segment.extend_from_slice(&((2 + 6 + exif.len()) as u16).to_be_bytes());
    segment.extend_from_slice(b"Exif\0\0");
    segment.extend(exif);
    segment
}

/// Returns a JPEG file shown with an extra orientation, by changing its EXIF orientation
/// instead of encoding its pixels again, which keeps their quality. Returns `None` for files
/// that are not JPEG, or have EXIF without an orientation, which cannot be added in place.
pub fn reorient_jpeg(jpeg: &[u8], change: Orientation) -> Option<Vec<u8>> {
    let segments = segments(jpeg)?;
    let ranges = exif_ranges(jpeg, &segments);
    // The orientations, of which the decoder shows the last one.
    let tagged: Vec<(Range<usize>, usize, u32)> = ranges
        .iter()
        .filter_map(|range| {
            let exif = ExifData::new(&jpeg[range.clone()])?;
            let directory = exif.first_directory()?;
            Some((range.clone(), directory, exif.get(directory, ORIENTATION)?))
        })
        .collect();
    let mut output = jpeg.to_vec();
    let Some((_, _, current)) = tagged.last() else {
        if !ranges.is_empty() {
            return None;
        }
        let at = exif_position(jpeg, &segments);
        output.splice(at..at, orientation_segment(change));
        return Some(output);
    };
    let current = Orientation::from_exif(*current as u16).unwrap_or_default();
    let value = u32::from(current.then(change).exif());
    for (range, directory, _) in tagged {
        set(&mut output[range], directory, ORIENTATION, value);
    }
    Some(output)
}

/// Adds the EXIF metadata of a JPEG file, like its capture date, camera and location, to a
/// JPEG of its pixels encoded again with a size. The decoder turns pixels and converts them to
/// sRGB, so the orientation and color space are reset, the size follows the new pixels, and
/// the thumbnail of the original is left out.
pub fn copy_jpeg_exif(source: &[u8], jpeg: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    let (Some(source_segments), Some(segments)) = (segments(source), segments(&jpeg)) else {
        return jpeg;
    };
    let Some(range) = exif_ranges(source, &source_segments).into_iter().next() else {
        return jpeg;
    };
    // The whole segment with its marker, length and EXIF signature.
    let mut segment = source[range.start - 10..range.end].to_vec();
    let data = &mut segment[10..];
    let Some((directory, little, exif_directory)) = ExifData::new(data).and_then(|exif| {
        let directory = exif.first_directory()?;
        Some((directory, exif.little, exif.get(directory, EXIF_DIRECTORY)))
    }) else {
        return jpeg;
    };
    set(data, directory, ORIENTATION, 1);
    // Unlinking the next directory drops the thumbnail, which shows the original.
    if let Some(count) = ExifData::new(data).and_then(|exif| exif.read(directory, 2)) {
        write(data, directory + 2 + count as usize * 12, 4, 0, little);
    }
    if let Some(exif_directory) = exif_directory {
        let exif_directory = exif_directory as usize;
        set(data, exif_directory, PIXEL_WIDTH, width);
        set(data, exif_directory, PIXEL_HEIGHT, height);
        set(data, exif_directory, COLOR_SPACE, SRGB);
    }
    let at = exif_position(&jpeg, &segments);
    let mut output = jpeg;
    output.splice(at..at, segment);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bitmap, Format, decode, encode};

    // The compressed pixels of a JPEG, from its start of scan.
    fn scan(bytes: &[u8]) -> &[u8] {
        let start = bytes
            .windows(2)
            .position(|marker| marker == [0xff, 0xda])
            .expect("start of scan");
        &bytes[start..]
    }

    // A JPEG of distinct quadrants that survive compression.
    fn jpeg() -> Vec<u8> {
        let pixels = (0..16 * 8)
            .flat_map(|index| {
                let (x, y) = (index % 16, index / 16);
                match (x < 8, y < 4) {
                    (true, true) => [250, 0, 0, 255],
                    (false, true) => [0, 250, 0, 255],
                    (true, false) => [0, 0, 250, 255],
                    (false, false) => [250, 250, 250, 255],
                }
            })
            .collect();
        encode(&Bitmap::new(16, 8, pixels).expect("bitmap"), Format::Jpeg).expect("encode")
    }

    fn decoded(bytes: &[u8]) -> (u32, u32, [u8; 3]) {
        let image = decode(bytes).expect("decode");
        let first = image.pixels();
        (
            image.width(),
            image.height(),
            [first[0], first[1], first[2]],
        )
    }

    // A little endian EXIF segment like a camera writes: a turned orientation and an EXIF
    // directory with a date, size and color space, followed by a thumbnail directory.
    fn camera_exif() -> Vec<u8> {
        let mut exif = b"II\x2a\0\x08\0\0\0".to_vec();
        let entry = |tag: u16, kind: u16, value: u32| {
            let mut entry = tag.to_le_bytes().to_vec();
            entry.extend_from_slice(&kind.to_le_bytes());
            entry.extend_from_slice(&1u32.to_le_bytes());
            entry.extend_from_slice(&value.to_le_bytes());
            entry
        };
        // The first directory at 8 with 2 entries ends at 38, the EXIF one follows.
        exif.extend_from_slice(&2u16.to_le_bytes());
        exif.extend(entry(ORIENTATION, SHORT, 6));
        exif.extend(entry(EXIF_DIRECTORY, LONG, 38));
        exif.extend_from_slice(&80u32.to_le_bytes());
        exif.extend_from_slice(&4u16.to_le_bytes());
        exif.extend(entry(0x9003, LONG, 1234));
        exif.extend(entry(COLOR_SPACE, SHORT, 0xffff));
        exif.extend(entry(PIXEL_WIDTH, LONG, 4000));
        exif.extend(entry(PIXEL_HEIGHT, SHORT, 3000));
        exif.extend_from_slice(&[0; 4]);
        exif.resize(96, 0);
        let mut segment = alloc::vec![0xff, 0xe1];
        segment.extend_from_slice(&((2 + 6 + exif.len()) as u16).to_be_bytes());
        segment.extend_from_slice(b"Exif\0\0");
        segment.extend(exif);
        segment
    }

    fn with_segment(jpeg: &[u8], segment: &[u8]) -> Vec<u8> {
        let mut file = jpeg[..2].to_vec();
        file.extend_from_slice(segment);
        file.extend_from_slice(&jpeg[2..]);
        file
    }

    #[test]
    fn orientations_combine() {
        use Orientation::*;
        assert_eq!(Rotate90.then(Rotate90), Rotate180);
        assert_eq!(Rotate90.then(Rotate270), Normal);
        assert_eq!(FlipHorizontal.then(FlipVertical), Rotate180);
        assert_eq!(Rotate270.then(FlipVertical), Transpose);
        assert_eq!(Rotate90.then(FlipVertical), Transverse);
        for orientation in ORIENTATIONS {
            assert_eq!(
                Orientation::from_exif(orientation.exif()),
                Some(orientation)
            );
            assert_eq!(orientation.then(Normal), orientation);
        }
        assert_eq!(Orientation::from_exif(9), None);
    }

    #[test]
    fn jpegs_turn_without_encoding_again() {
        let bytes = jpeg();
        let turned = reorient_jpeg(&bytes, Orientation::Rotate90).expect("reorient");
        assert_eq!(scan(&turned), scan(&bytes));
        let (width, height, first) = decoded(&turned);
        assert_eq!((width, height), (8, 16));
        // The bottom left blue quadrant is now at the top left.
        assert!(first[2] > 200 && first[0] < 50, "{first:?}");

        // A file with an orientation changes it in place.
        let back = reorient_jpeg(&turned, Orientation::Rotate270).expect("reorient");
        assert_eq!(back.len(), turned.len());
        let (width, height, first) = decoded(&back);
        assert_eq!((width, height), (16, 8));
        assert!(first[0] > 200 && first[2] < 50, "{first:?}");

        // Camera files keep their other metadata.
        let camera = with_segment(&bytes, &camera_exif());
        let upright = reorient_jpeg(&camera, Orientation::Rotate270).expect("reorient");
        assert_eq!(upright.len(), camera.len());
        assert_eq!(decoded(&upright).0, 16);
    }

    #[test]
    fn exif_without_orientation_is_kept() {
        let mut segment = alloc::vec![0xff, 0xe1, 0, 16];
        segment.extend_from_slice(b"Exif\0\0II\x2a\0\x08\0\0\0");
        let without = with_segment(&jpeg(), &segment);
        assert_eq!(reorient_jpeg(&without, Orientation::Rotate270), None);
        assert_eq!(reorient_jpeg(b"not a jpeg", Orientation::Rotate270), None);
    }

    #[test]
    fn encoded_photos_keep_their_metadata() {
        let camera = with_segment(&jpeg(), &camera_exif());
        let edited = copy_jpeg_exif(&camera, jpeg(), 16, 8);
        let segments = segments(&edited).expect("jpeg");
        let range = exif_ranges(&edited, &segments).remove(0);
        let exif = ExifData::new(&edited[range]).expect("exif");
        let directory = exif.first_directory().expect("directory");
        assert_eq!(exif.get(directory, ORIENTATION), Some(1));
        assert_eq!(exif.read(directory + 2 + 2 * 12, 4), Some(0));
        let exif_directory = exif.get(directory, EXIF_DIRECTORY).expect("exif directory") as usize;
        assert_eq!(exif.get(exif_directory, 0x9003), Some(1234));
        assert_eq!(exif.get(exif_directory, PIXEL_WIDTH), Some(16));
        assert_eq!(exif.get(exif_directory, PIXEL_HEIGHT), Some(8));
        assert_eq!(exif.get(exif_directory, COLOR_SPACE), Some(SRGB));
        // The pixels show upright and unchanged.
        assert_eq!(decoded(&edited).0, 16);
        assert_eq!(scan(&edited), scan(&jpeg()));
        // Files without EXIF stay as they are.
        assert_eq!(copy_jpeg_exif(&jpeg(), jpeg(), 16, 8), jpeg());
    }
}
