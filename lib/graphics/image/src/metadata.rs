/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Metadata shared by image decoders: TIFF directories, EXIF orientation of pixels, and the
//! readable metadata of EXIF and TIFF tags, XMP packets and ICC profiles.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
#[cfg(feature = "jpeg")]
use core::ops::Range;

use super::math::pow;
#[cfg(any(feature = "jpeg", feature = "webp"))]
use super::{Budget, Result};
use super::{Metadata, MetadataGroup};

#[cfg(any(feature = "jpeg", feature = "webp"))]
pub(crate) const ORIENTATION: u16 = 0x112;
pub(crate) const EXIF_DIRECTORY: u16 = 0x8769;
#[cfg(feature = "jpeg")]
pub(crate) const COLOR_SPACE: u16 = 0xa001;
#[cfg(feature = "jpeg")]
pub(crate) const INTEROPERABILITY_DIRECTORY: u16 = 0xa005;
#[cfg(feature = "jpeg")]
const PIXEL_WIDTH: u16 = 0xa002;
#[cfg(feature = "jpeg")]
const PIXEL_HEIGHT: u16 = 0xa003;
#[cfg(feature = "jpeg")]
const SRGB: u32 = 1;

/// How stored pixels are turned to show them, as numbered by the EXIF orientation tag.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[cfg(any(feature = "jpeg", feature = "webp"))]
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

#[cfg(any(feature = "jpeg", feature = "webp"))]
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

#[cfg(any(feature = "jpeg", feature = "webp"))]
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
#[cfg(feature = "jpeg")]
struct Segment {
    marker: u8,
    contents: Range<usize>,
}

// The segments before the pixel data, or `None` for a file that is not a JPEG.
#[cfg(feature = "jpeg")]
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
#[cfg(feature = "jpeg")]
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
#[cfg(feature = "jpeg")]
fn exif_position(jpeg: &[u8], segments: &[Segment]) -> usize {
    segments
        .iter()
        .find(|segment| {
            segment.marker == 0xe0 && jpeg[segment.contents.clone()].starts_with(b"JFIF\0")
        })
        .map_or(2, |segment| segment.contents.end)
}

// Writes a number in a byte order.
#[cfg(feature = "jpeg")]
fn write(data: &mut [u8], at: usize, length: usize, value: u32, little: bool) {
    if let Some(bytes) = data.get_mut(at..at + length) {
        for (index, byte) in bytes.iter_mut().enumerate() {
            let shift = if little { index } else { length - 1 - index } * 8;
            *byte = (value >> shift) as u8;
        }
    }
}

// Changes a number entry of a directory that EXIF data has.
#[cfg(feature = "jpeg")]
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
#[cfg(feature = "jpeg")]
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
#[cfg(feature = "jpeg")]
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
#[cfg(feature = "jpeg")]
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

// MARK: TIFF directories

pub(crate) const ASCII: u16 = 2;
#[cfg(any(feature = "jpeg", feature = "webp"))]
pub(crate) const SHORT: u16 = 3;
#[cfg(feature = "jpeg")]
pub(crate) const LONG: u16 = 4;

/// TIFF structured data, like EXIF metadata or a TIFF file.
pub(crate) struct ExifData<'a> {
    data: &'a [u8],
    pub(crate) little: bool,
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
}

// MARK: EXIF and TIFF tags

const GPS_DIRECTORY: u16 = 0x8825;
const XMP: u16 = 0x2bc;
const ICC_PROFILE: u16 = 0x8773;

// The size of a value of each TIFF type, by type number.
const TYPE_SIZES: [usize; 13] = [0, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 4, 8];
// Values of more numbers than this are only counted.
const MAX_NUMBERS: usize = 16;

// A tag entry of a directory.
struct Entry<'a> {
    tag: u16,
    kind: u16,
    count: usize,
    bytes: &'a [u8],
}

impl ExifData<'_> {
    // The entries of a directory with their values.
    fn entries(&self, directory: usize) -> Vec<Entry<'_>> {
        let count = self.read(directory, 2).unwrap_or(0) as usize;
        (0..count)
            .filter_map(|index| {
                let at = directory.checked_add(2 + index * 12)?;
                let tag = self.read(at, 2)? as u16;
                let kind = self.read(at + 2, 2)? as u16;
                let count = self.read(at + 4, 4)? as usize;
                let length = TYPE_SIZES.get(usize::from(kind))?.checked_mul(count)?;
                let start = if length <= 4 {
                    at + 8
                } else {
                    self.read(at + 8, 4)? as usize
                };
                let bytes = self.data.get(start..start.checked_add(length)?)?;
                Some(Entry {
                    tag,
                    kind,
                    count,
                    bytes,
                })
            })
            .collect()
    }

    // The value of a directory pointer entry.
    fn pointer(&self, entry: &Entry<'_>) -> Option<usize> {
        self.integer(entry, 0).map(|offset| offset as usize)
    }

    // The integer at an index of a value.
    fn integer(&self, entry: &Entry<'_>, index: usize) -> Option<i64> {
        let size = TYPE_SIZES[usize::from(entry.kind)];
        let bytes = entry.bytes.get(index * size..(index + 1) * size)?;
        let value = ExifData {
            data: bytes,
            little: self.little,
        }
        .read(0, size.min(4))?;
        Some(match entry.kind {
            1 | 3 | 4 | 7 => i64::from(value),
            6 => i64::from(value as u8 as i8),
            8 => i64::from(value as u16 as i16),
            9 => i64::from(value as i32),
            _ => return None,
        })
    }

    // The rational or float at an index of a value.
    fn real(&self, entry: &Entry<'_>, index: usize) -> Option<f64> {
        let read = |at: usize, length: usize| {
            ExifData {
                data: entry.bytes,
                little: self.little,
            }
            .read(at, length)
        };
        Some(match entry.kind {
            5 | 10 => {
                let (numerator, denominator) = (read(index * 8, 4)?, read(index * 8 + 4, 4)?);
                if denominator == 0 {
                    return None;
                }
                if entry.kind == 5 {
                    f64::from(numerator) / f64::from(denominator)
                } else {
                    f64::from(numerator as i32) / f64::from(denominator as i32)
                }
            }
            11 => f64::from(f32::from_bits(read(index * 4, 4)?)),
            12 => {
                let (first, second) = (read(index * 8, 4)?, read(index * 8 + 4, 4)?);
                let bits = if self.little {
                    u64::from(second) << 32 | u64::from(first)
                } else {
                    u64::from(first) << 32 | u64::from(second)
                };
                f64::from_bits(bits)
            }
            _ => self.integer(entry, index)? as f64,
        })
    }

    // All numbers of a value, or `None` when it has too many to list.
    fn numbers(&self, entry: &Entry<'_>) -> Option<Vec<f64>> {
        (entry.count <= MAX_NUMBERS)
            .then(|| {
                (0..entry.count)
                    .map(|index| self.real(entry, index))
                    .collect()
            })
            .flatten()
    }
}

/// Lists the tags of the first image directory of TIFF structured data, like EXIF metadata,
/// with those of its EXIF and GPS directories. The XMP packet and ICC profile of a TIFF file
/// are described too.
pub(crate) fn list_exif(data: &[u8], metadata: &mut Vec<Metadata>) {
    let Some(exif) = ExifData::new(data) else {
        return;
    };
    let Some(directory) = exif.first_directory() else {
        return;
    };
    list_directory(&exif, directory, MetadataGroup::Exif, true, metadata);
}

// Lists the tags of a directory, and those of the EXIF and GPS directories it points to when it
// is the first one, so directories pointing at each other are not followed around.
fn list_directory(
    exif: &ExifData<'_>,
    directory: usize,
    group: MetadataGroup,
    first: bool,
    metadata: &mut Vec<Metadata>,
) {
    let entries = exif.entries(directory);
    for entry in &entries {
        let value = match (group, entry.tag) {
            (MetadataGroup::Exif, EXIF_DIRECTORY | GPS_DIRECTORY) => {
                if let Some(pointer) = exif
                    .pointer(entry)
                    .filter(|pointer| first && *pointer != directory)
                {
                    let group = if entry.tag == EXIF_DIRECTORY {
                        MetadataGroup::Exif
                    } else {
                        MetadataGroup::Gps
                    };
                    list_directory(exif, pointer, group, false, metadata);
                }
                continue;
            }
            (MetadataGroup::Exif, XMP) => {
                list_xmp(entry.bytes, metadata);
                continue;
            }
            (MetadataGroup::Exif, ICC_PROFILE) => {
                list_icc(entry.bytes, metadata);
                continue;
            }
            (MetadataGroup::Exif, tag) if SKIPPED_TAGS.contains(&tag) => continue,
            (MetadataGroup::Gps, 1 | 3 | 5) => continue,
            (MetadataGroup::Gps, _) => gps_value(exif, &entries, entry),
            _ => exif_value(exif, entry),
        };
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            continue;
        };
        let names = if group == MetadataGroup::Gps {
            GPS_NAMES
        } else {
            EXIF_NAMES
        };
        let name = names.iter().find(|(tag, _)| *tag == entry.tag).map_or_else(
            || format!("Tag 0x{:04X}", entry.tag),
            |(_, name)| (*name).to_string(),
        );
        metadata.push(Metadata::new(group, name, value));
    }
}

// Structure of the pixel data and binary blobs that are not shown.
const SKIPPED_TAGS: [u16; 14] = [
    0x0111, 0x0117, 0x0144, 0x0145, 0x015b, 0x0201, 0x0202, 0x0140, 0x0153, 0x0152, 0x83bb, 0x927c,
    0x9101, 0xa005,
];

const EXIF_NAMES: &[(u16, &str)] = &[
    (0x00fe, "Subfile Type"),
    (0x0100, "Image Width"),
    (0x0101, "Image Height"),
    (0x0102, "Bits per Sample"),
    (0x0103, "Compression"),
    (0x0106, "Photometric Interpretation"),
    (0x010a, "Fill Order"),
    (0x010d, "Document Name"),
    (0x010e, "Description"),
    (0x010f, "Camera Make"),
    (0x0110, "Camera Model"),
    (0x0112, "Orientation"),
    (0x0115, "Samples per Pixel"),
    (0x0116, "Rows per Strip"),
    (0x011a, "Horizontal Resolution"),
    (0x011b, "Vertical Resolution"),
    (0x011c, "Planar Configuration"),
    (0x0128, "Resolution Unit"),
    (0x0131, "Software"),
    (0x0132, "Date Modified"),
    (0x013b, "Artist"),
    (0x013c, "Host Computer"),
    (0x013d, "Predictor"),
    (0x0142, "Tile Width"),
    (0x0143, "Tile Length"),
    (0x0213, "YCbCr Positioning"),
    (0x8298, "Copyright"),
    (0x829a, "Exposure Time"),
    (0x829d, "F-Number"),
    (0x8822, "Exposure Program"),
    (0x8827, "ISO"),
    (0x8830, "Sensitivity Type"),
    (0x9000, "EXIF Version"),
    (0x9003, "Date Taken"),
    (0x9004, "Date Digitized"),
    (0x9010, "Time Zone"),
    (0x9011, "Time Zone Taken"),
    (0x9012, "Time Zone Digitized"),
    (0x9102, "Compressed Bits per Pixel"),
    (0x9201, "Shutter Speed"),
    (0x9202, "Aperture"),
    (0x9203, "Brightness"),
    (0x9204, "Exposure Bias"),
    (0x9205, "Max Aperture"),
    (0x9206, "Subject Distance"),
    (0x9207, "Metering Mode"),
    (0x9208, "Light Source"),
    (0x9209, "Flash"),
    (0x920a, "Focal Length"),
    (0x9214, "Subject Area"),
    (0x9286, "User Comment"),
    (0x9290, "Subsecond Time"),
    (0x9291, "Subsecond Time Taken"),
    (0x9292, "Subsecond Time Digitized"),
    (0x9c9b, "Title"),
    (0x9c9c, "Comment"),
    (0x9c9d, "Author"),
    (0x9c9e, "Keywords"),
    (0x9c9f, "Subject"),
    (0xa000, "FlashPix Version"),
    (0xa001, "Color Space"),
    (0xa002, "Pixel Width"),
    (0xa003, "Pixel Height"),
    (0xa20e, "Focal Plane Horizontal Resolution"),
    (0xa20f, "Focal Plane Vertical Resolution"),
    (0xa210, "Focal Plane Resolution Unit"),
    (0xa217, "Sensing Method"),
    (0xa300, "File Source"),
    (0xa301, "Scene Type"),
    (0xa401, "Custom Rendered"),
    (0xa402, "Exposure Mode"),
    (0xa403, "White Balance"),
    (0xa404, "Digital Zoom Ratio"),
    (0xa405, "Focal Length in 35mm Film"),
    (0xa406, "Scene Capture Type"),
    (0xa407, "Gain Control"),
    (0xa408, "Contrast"),
    (0xa409, "Saturation"),
    (0xa40a, "Sharpness"),
    (0xa40c, "Subject Distance Range"),
    (0xa420, "Image Unique ID"),
    (0xa430, "Camera Owner"),
    (0xa431, "Camera Serial Number"),
    (0xa432, "Lens Specification"),
    (0xa433, "Lens Make"),
    (0xa434, "Lens Model"),
    (0xa435, "Lens Serial Number"),
    (0xa460, "Composite Image"),
];

const GPS_NAMES: &[(u16, &str)] = &[
    (0x00, "GPS Version"),
    (0x02, "Latitude"),
    (0x04, "Longitude"),
    (0x06, "Altitude"),
    (0x07, "Time"),
    (0x08, "Satellites"),
    (0x09, "Receiver Status"),
    (0x0a, "Measure Mode"),
    (0x0b, "Precision"),
    (0x0c, "Speed Unit"),
    (0x0d, "Speed"),
    (0x0e, "Track Reference"),
    (0x0f, "Track"),
    (0x10, "Direction Reference"),
    (0x11, "Direction"),
    (0x12, "Map Datum"),
    (0x17, "Destination Bearing Reference"),
    (0x18, "Destination Bearing"),
    (0x1b, "Processing Method"),
    (0x1d, "Date"),
    (0x1e, "Differential"),
    (0x1f, "Horizontal Error"),
];

// The names of the values of tags with enumerated values.
const fn enumerated(tag: u16, value: i64) -> Option<&'static str> {
    Some(match (tag, value) {
        (0x0103, 1) => "None",
        (0x0103, 2) => "CCITT Huffman",
        (0x0103, 3) => "CCITT Group 3",
        (0x0103, 4) => "CCITT Group 4",
        (0x0103, 5) => "LZW",
        (0x0103, 6 | 7) => "JPEG",
        (0x0103, 8 | 32946) => "Deflate",
        (0x0103, 32773) => "PackBits",
        (0x0106, 0) => "White Is Zero",
        (0x0106, 1) => "Black Is Zero",
        (0x0106, 2) => "RGB",
        (0x0106, 3) => "Palette",
        (0x0106, 4) => "Transparency Mask",
        (0x0106, 5) => "CMYK",
        (0x0106, 6) => "YCbCr",
        (0x0106, 8) => "CIELab",
        (0x0112, 1) => "Normal",
        (0x0112, 2) => "Mirrored Horizontally",
        (0x0112, 3) => "Rotated 180\u{b0}",
        (0x0112, 4) => "Mirrored Vertically",
        (0x0112, 5) => "Mirrored Horizontally, Rotated 270\u{b0}",
        (0x0112, 6) => "Rotated 90\u{b0}",
        (0x0112, 7) => "Mirrored Horizontally, Rotated 90\u{b0}",
        (0x0112, 8) => "Rotated 270\u{b0}",
        (0x011c, 1) => "Chunky",
        (0x011c, 2) => "Planar",
        (0x0128 | 0xa210, 1) => "None",
        (0x0128 | 0xa210, 2) => "Inches",
        (0x0128 | 0xa210, 3) => "Centimeters",
        (0x0213, 1) => "Centered",
        (0x0213, 2) => "Co-sited",
        (0x8822, 0) => "Not Defined",
        (0x8822, 1) => "Manual",
        (0x8822, 2) => "Normal Program",
        (0x8822, 3) => "Aperture Priority",
        (0x8822, 4) => "Shutter Priority",
        (0x8822, 5) => "Creative Program",
        (0x8822, 6) => "Action Program",
        (0x8822, 7) => "Portrait Mode",
        (0x8822, 8) => "Landscape Mode",
        (0x9207, 0) => "Unknown",
        (0x9207, 1) => "Average",
        (0x9207, 2) => "Center Weighted Average",
        (0x9207, 3) => "Spot",
        (0x9207, 4) => "Multi-spot",
        (0x9207, 5) => "Pattern",
        (0x9207, 6) => "Partial",
        (0x9208, 0) => "Unknown",
        (0x9208, 1) => "Daylight",
        (0x9208, 2) => "Fluorescent",
        (0x9208, 3) => "Tungsten",
        (0x9208, 4) => "Flash",
        (0x9208, 9) => "Fine Weather",
        (0x9208, 10) => "Cloudy",
        (0x9208, 11) => "Shade",
        (0x9208, 17..=19) => "Standard Light",
        (0x9208, 20..=23) => "Daylight",
        (0x9208, 24) => "Studio Tungsten",
        (0xa001, 1) => "sRGB",
        (0xa001, 2) => "Adobe RGB",
        (0xa001, 0xffff) => "Uncalibrated",
        (0xa217, 1) => "Not Defined",
        (0xa217, 2) => "One-chip Color Area",
        (0xa217, 3) => "Two-chip Color Area",
        (0xa217, 4) => "Three-chip Color Area",
        (0xa217, 5) => "Color Sequential Area",
        (0xa217, 7) => "Trilinear",
        (0xa217, 8) => "Color Sequential Linear",
        (0x010a, 1) => "Most Significant Bit First",
        (0x010a, 2) => "Least Significant Bit First",
        (0xa300, 1) => "Film Scanner",
        (0xa300, 2) => "Reflection Print Scanner",
        (0xa300, 3) => "Digital Camera",
        (0xa301, 1) => "Directly Photographed",
        (0xa401, 0) => "Normal",
        (0xa401, 1) => "Custom",
        (0xa402, 0) => "Auto",
        (0xa402, 1) => "Manual",
        (0xa402, 2) => "Auto Bracket",
        (0xa403, 0) => "Auto",
        (0xa403, 1) => "Manual",
        (0xa406, 0) => "Standard",
        (0xa406, 1) => "Landscape",
        (0xa406, 2) => "Portrait",
        (0xa406, 3) => "Night",
        (0xa407, 0) => "None",
        (0xa407, 1) => "Low Gain Up",
        (0xa407, 2) => "High Gain Up",
        (0xa407, 3) => "Low Gain Down",
        (0xa407, 4) => "High Gain Down",
        (0xa408 | 0xa40a, 0) => "Normal",
        (0xa408, 1) => "Low",
        (0xa408, 2) => "High",
        (0xa409, 0) => "Normal",
        (0xa409, 1) => "Low",
        (0xa409, 2) => "High",
        (0xa40a, 1) => "Soft",
        (0xa40a, 2) => "Hard",
        (0xa40c, 0) => "Unknown",
        (0xa40c, 1) => "Macro",
        (0xa40c, 2) => "Close",
        (0xa40c, 3) => "Distant",
        _ => return None,
    })
}

// The readable value of an EXIF or TIFF tag.
fn exif_value(exif: &ExifData<'_>, entry: &Entry<'_>) -> Option<String> {
    let first = || exif.real(entry, 0);
    match entry.tag {
        // Windows stores these as UTF-16 bytes.
        0x9c9b..=0x9c9f => return Some(utf16(entry.bytes, true)),
        0x9000 | 0xa000 => return version(entry.bytes),
        0x9286 => return Some(user_comment(entry.bytes)),
        0x829a => return first().map(exposure_time),
        0x829d => return first().map(|value| format!("f/{}", decimal(value, 1))),
        0x9201 => return first().map(|value| exposure_time(pow(2.0, -value))),
        0x9202 | 0x9205 => {
            return first().map(|value| format!("f/{}", decimal(pow(2.0, value / 2.0), 1)));
        }
        0x9204 => {
            return first().map(|value| match decimal(value, 1).as_str() {
                "0" | "-0" => String::from("0 EV"),
                bias if value > 0.0 => format!("+{bias} EV"),
                bias => format!("{bias} EV"),
            });
        }
        // EXIF dates separate their parts with colons, like 2003:05:16 06:51:43.
        0x0132 | 0x9003 | 0x9004 => return Some(date(&text(entry.bytes))),
        0x920a => return first().map(|value| format!("{} mm", decimal(value, 1))),
        0xa405 => return first().map(|value| format!("{} mm", decimal(value, 0))),
        0x9206 => return first().map(|value| format!("{} m", decimal(value, 2))),
        0x9209 => {
            let flash = exif.integer(entry, 0)?;
            return Some(String::from(if flash & 1 != 0 {
                "Fired"
            } else {
                "Did Not Fire"
            }));
        }
        0xa432 => {
            let [short, long, short_f, long_f] =
                core::array::from_fn(|index| exif.real(entry, index));
            let focal = match (short?, long?) {
                (short, long) if short == long => format!("{} mm", decimal(short, 0)),
                (short, long) => format!("{}\u{2013}{} mm", decimal(short, 0), decimal(long, 0)),
            };
            return Some(match (short_f, long_f) {
                (Some(short), Some(long)) if short == long => {
                    format!("{focal} f/{}", decimal(short, 1))
                }
                (Some(short), Some(long)) => {
                    format!(
                        "{focal} f/{}\u{2013}{}",
                        decimal(short, 1),
                        decimal(long, 1)
                    )
                }
                _ => focal,
            });
        }
        _ => {}
    }
    if entry.count == 1
        && let Some(name) = exif
            .integer(entry, 0)
            .and_then(|value| enumerated(entry.tag, value))
    {
        return Some(String::from(name));
    }
    generic_value(exif, entry)
}

// The readable value of a GPS tag, with its reference for a coordinate or altitude.
fn gps_value(exif: &ExifData<'_>, entries: &[Entry<'_>], entry: &Entry<'_>) -> Option<String> {
    let reference = |tag: u16| {
        entries
            .iter()
            .find(|entry| entry.tag == tag)
            .map(|entry| text(entry.bytes))
    };
    let real = |index| exif.real(entry, index);
    match entry.tag {
        0x00 => Some(
            entry
                .bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join("."),
        ),
        0x02 | 0x04 => {
            let degrees =
                real(0)? + real(1).unwrap_or(0.0) / 60.0 + real(2).unwrap_or(0.0) / 3600.0;
            let reference = reference(entry.tag - 1).unwrap_or_default();
            Some(
                format!("{}\u{b0} {reference}", decimal(degrees, 6))
                    .trim_end()
                    .into(),
            )
        }
        0x06 => {
            let below = entries
                .iter()
                .find(|entry| entry.tag == 0x05)
                .and_then(|reference| exif.integer(reference, 0))
                == Some(1);
            Some(format!(
                "{}{} m",
                if below { "-" } else { "" },
                decimal(real(0)?, 1)
            ))
        }
        0x1d => Some(date(&text(entry.bytes))),
        0x07 => {
            let [hours, minutes, seconds] = [real(0)?, real(1)?, real(2)?];
            Some(format!(
                "{:02}:{:02}:{:02} UTC",
                hours as u32, minutes as u32, seconds as u32
            ))
        }
        _ => generic_value(exif, entry),
    }
}

// The value of a tag as text or numbers.
fn generic_value(exif: &ExifData<'_>, entry: &Entry<'_>) -> Option<String> {
    match entry.kind {
        ASCII => Some(text(entry.bytes)),
        1 | 7 if is_text(entry.bytes) => Some(text(entry.bytes)),
        1 | 7 if entry.count > MAX_NUMBERS => Some(bytes(entry.count)),
        _ => match exif.numbers(entry) {
            Some(numbers) => Some(
                numbers
                    .iter()
                    .map(|number| decimal(*number, 4))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            None => Some(format!("{} values", entry.count)),
        },
    }
}

// An exposure time as a fraction of a second, or in seconds.
fn exposure_time(seconds: f64) -> String {
    if seconds > 0.0 && seconds < 1.0 {
        format!("1/{} s", decimal(1.0 / seconds, 0))
    } else {
        format!("{} s", decimal(seconds, 1))
    }
}

// An EXIF date with dashes between the parts of its date.
fn date(date: &str) -> String {
    match date.split_at_checked(10) {
        Some((day, time)) if day.as_bytes().get(4) == Some(&b':') => {
            format!("{}{time}", day.replace(':', "-"))
        }
        _ => date.into(),
    }
}

// An EXIF version of four digits, like 0232 for 2.32.
fn version(bytes: &[u8]) -> Option<String> {
    let [major @ .., minor_1, minor_2] = bytes else {
        return None;
    };
    let major = core::str::from_utf8(major).ok()?.trim_start_matches('0');
    Some(format!(
        "{}.{}{}",
        if major.is_empty() { "0" } else { major },
        char::from(*minor_1),
        char::from(*minor_2)
    ))
}

// A user comment, after its character code.
fn user_comment(bytes: &[u8]) -> String {
    match bytes.split_at_checked(8) {
        Some((b"UNICODE\0", rest)) => utf16(rest, false),
        Some((_, rest)) => text(rest),
        None => text(bytes),
    }
}

/// The number of bytes of binary data.
fn bytes(count: usize) -> String {
    format!("{count} bytes")
}

// Whether bytes are printable text, up to its terminator.
fn is_text(bytes: &[u8]) -> bool {
    let text = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
    !text.is_empty()
        && text
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ')
}

// Text up to its terminator, trimmed, as UTF-8 or else Latin-1.
pub(crate) fn text(bytes: &[u8]) -> String {
    let bytes = bytes.split(|byte| *byte == 0).next().unwrap_or_default();
    match core::str::from_utf8(bytes) {
        Ok(text) => text.trim().into(),
        Err(_) => bytes
            .iter()
            .map(|byte| char::from(*byte))
            .collect::<String>()
            .trim()
            .into(),
    }
}

// UTF-16 text up to its terminator.
fn utf16(bytes: &[u8], little: bool) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|unit| {
        if little {
            u16::from_le_bytes(*unit)
        } else {
            u16::from_be_bytes(*unit)
        }
    });
    char::decode_utf16(units.take_while(|unit| *unit != 0))
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect::<String>()
        .trim()
        .into()
}

/// A number with at most some decimals, without trailing zeros.
pub(crate) fn decimal(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').into()
    } else {
        text
    }
}

// MARK: XMP

/// Lists the properties of an XMP packet: the attributes and text of its description
/// elements, with the values of lists joined.
pub(crate) fn list_xmp(packet: &[u8], metadata: &mut Vec<Metadata>) {
    let Ok(xml) = core::str::from_utf8(packet) else {
        return;
    };
    let mut elements: Vec<&str> = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find('<') {
        let content = rest[..start].trim();
        if !content.is_empty()
            && let Some(name) = elements.iter().rev().find(|name| !name.starts_with("rdf:"))
        {
            add_xmp(metadata, name, &unescape(content));
        }
        rest = &rest[start + 1..];
        let Some(end) = rest.find('>') else {
            break;
        };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        if tag.starts_with(['?', '!']) {
            continue;
        }
        if let Some(name) = tag.strip_prefix('/') {
            if let Some(index) = elements.iter().rposition(|element| *element == name.trim()) {
                elements.truncate(index);
            }
            continue;
        }
        let closed = tag.ends_with('/');
        let tag = tag.trim_end_matches('/');
        let (name, attributes) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        for (attribute, value) in attributes_of(attributes) {
            if !attribute.starts_with("xmlns")
                && !attribute.starts_with("rdf:")
                && !attribute.starts_with("xml:")
                && !attribute.starts_with("x:")
            {
                add_xmp(metadata, attribute, &unescape(value));
            }
        }
        if !closed {
            elements.push(name);
        }
    }
}

// The name and value pairs of the attributes of a tag.
fn attributes_of(mut attributes: &str) -> impl Iterator<Item = (&str, &str)> {
    core::iter::from_fn(move || {
        let (name, rest) = attributes.split_once('=')?;
        let rest = rest.trim_start();
        let quote = rest.chars().next()?;
        let rest = rest.get(1..)?;
        let (value, rest) = rest.split_once(quote)?;
        attributes = rest;
        Some((name.trim(), value))
    })
}

// Adds an XMP property, or a value to the property before it with the same name.
fn add_xmp(metadata: &mut Vec<Metadata>, name: &str, value: &str) {
    let name = readable_name(name);
    match metadata.last_mut() {
        Some(last) if last.group == MetadataGroup::Xmp && last.name == name => {
            last.value.push_str(", ");
            last.value.push_str(value);
        }
        _ => metadata.push(Metadata::new(MetadataGroup::Xmp, name, value.into())),
    }
}

// A property name without its namespace prefix, with words split, like Creator Tool for
// xmp:CreatorTool.
fn readable_name(name: &str) -> String {
    let name = name.rsplit(':').next().unwrap_or(name);
    let mut readable = String::new();
    let mut previous: Option<char> = None;
    for character in name.chars() {
        if character.is_ascii_uppercase()
            && previous.is_some_and(|previous| previous.is_ascii_lowercase())
        {
            readable.push(' ');
        }
        readable.push(if previous.is_none() {
            character.to_ascii_uppercase()
        } else {
            character
        });
        previous = Some(character);
    }
    readable
}

// Text with its XML character references resolved.
fn unescape(text: &str) -> String {
    let mut unescaped = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        unescaped.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let reference = &rest[1..end];
        let character = match reference {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => reference
                .strip_prefix("#x")
                .map_or_else(
                    || reference.strip_prefix('#').and_then(|n| n.parse().ok()),
                    |hex| u32::from_str_radix(hex, 16).ok(),
                )
                .and_then(char::from_u32),
        };
        match character {
            Some(character) => {
                unescaped.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                unescaped.push('&');
                rest = &rest[1..];
            }
        }
    }
    unescaped.push_str(rest);
    unescaped
}

// MARK: ICC profiles

/// Lists the description of an ICC profile as its color profile.
pub(crate) fn list_icc(profile: &[u8], metadata: &mut Vec<Metadata>) {
    if let Some(description) = icc_description(profile) {
        metadata.push(Metadata::new(
            MetadataGroup::Image,
            "Color Profile".into(),
            description,
        ));
    }
}

// The description of an ICC profile, from its v2 text description or v4 localized description.
fn icc_description(profile: &[u8]) -> Option<String> {
    let be32 = |at: usize| {
        profile
            .get(at..at.checked_add(4)?)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize)
    };
    let count = be32(128)?;
    let tag = (0..count.min(profile.len() / 12)).find_map(|index| {
        let entry = 132 + index * 12;
        (profile.get(entry..entry + 4)? == b"desc").then_some(())?;
        let offset = be32(entry + 4)?;
        profile.get(offset..offset.checked_add(be32(entry + 8)?)?)
    })?;
    let description = match tag.get(..4)? {
        b"desc" => {
            let length = u32::from_be_bytes(tag.get(8..12)?.try_into().ok()?) as usize;
            text(tag.get(12..12 + length)?)
        }
        b"mluc" => {
            let length = u32::from_be_bytes(tag.get(20..24)?.try_into().ok()?) as usize;
            let offset = u32::from_be_bytes(tag.get(24..28)?.try_into().ok()?) as usize;
            utf16(tag.get(offset..offset.checked_add(length)?)?, false)
        }
        _ => return None,
    };
    (!description.is_empty()).then_some(description)
}

// MARK: Orientation and number entries

#[cfg(any(feature = "jpeg", feature = "webp"))]
impl ExifData<'_> {
    pub(crate) fn orientation(&self) -> Option<u16> {
        let directory = self.first_directory()?;
        let value = self.read(self.entry(directory, ORIENTATION, SHORT)?, 2)? as u16;
        Orientation::from_exif(value).map(Orientation::exif)
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
    #[cfg(feature = "jpeg")]
    fn number(&self, directory: usize, tag: u16) -> Option<(usize, usize)> {
        self.entry(directory, tag, SHORT)
            .map(|at| (at, 2))
            .or_else(|| self.entry(directory, tag, LONG).map(|at| (at, 4)))
    }

    /// The value of a number entry, a short or long.
    #[cfg(feature = "jpeg")]
    pub(crate) fn get(&self, directory: usize, tag: u16) -> Option<u32> {
        let (at, length) = self.number(directory, tag)?;
        self.read(at, length)
    }
}

// MARK: Pixel orientation

#[cfg(any(feature = "jpeg", feature = "webp"))]
pub(super) fn orient(
    width: u32,
    height: u32,
    mut pixels: Vec<u8>,
    orientation: u16,
    budget: &mut Budget,
) -> Result<(u32, u32, Vec<u8>)> {
    if orientation == 1 {
        return Ok((width, height, pixels));
    }
    if (2..=4).contains(&orientation) {
        let pixel_chunks = pixels.as_chunks_mut::<4>().0;
        let row_width = width as usize;
        let row_height = height as usize;
        match orientation {
            2 => {
                for row in pixel_chunks.chunks_exact_mut(row_width) {
                    row.reverse();
                }
            }
            3 => pixel_chunks.reverse(),
            4 => {
                for y in 0..row_height / 2 {
                    for x in 0..row_width {
                        pixel_chunks.swap(y * row_width + x, (row_height - 1 - y) * row_width + x);
                    }
                }
            }
            _ => unreachable!(),
        }
        return Ok((width, height, pixels));
    }
    let (w, h) = (height, width);
    let mut output = budget.zeroed(pixels.len())?;
    for y in 0..height {
        for x in 0..width {
            let (dx, dy) = match orientation {
                5 => (y, x),
                6 => (height - 1 - y, x),
                7 => (height - 1 - y, width - 1 - x),
                8 => (y, width - 1 - x),
                _ => unreachable!("orientation was checked"),
            };
            let src = (y as usize * width as usize + x as usize) * 4;
            let dst = (dy as usize * w as usize + dx as usize) * 4;
            output[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    Ok((w, h, output))
}

// MARK: Tests

#[cfg(all(test, feature = "jpeg"))]
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

    fn entry(tag: u16, kind: u16, count: u32, value: [u8; 4]) -> Vec<u8> {
        let mut entry = tag.to_be_bytes().to_vec();
        entry.extend_from_slice(&kind.to_be_bytes());
        entry.extend_from_slice(&count.to_be_bytes());
        entry.extend_from_slice(&value);
        entry
    }

    fn find<'a>(metadata: &'a [Metadata], name: &str) -> Option<&'a str> {
        metadata
            .iter()
            .find(|entry| entry.name() == name)
            .map(Metadata::value)
    }

    #[test]
    fn exif_tags_are_named_and_formatted() {
        // A big endian directory at 8, with values after its 4 entries at 62.
        let mut data = b"MM\0\x2a\0\0\0\x08\0\x04".to_vec();
        data.extend(entry(0x010f, ASCII, 4, *b"Acme"));
        data.extend(entry(0x0112, 3, 1, [0, 6, 0, 0]));
        data.extend(entry(0x829a, 5, 1, [0, 0, 0, 62]));
        data.extend(entry(0xbeef, 3, 1, [0, 7, 0, 0]));
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 250]);
        let mut metadata = Vec::new();
        list_exif(&data, &mut metadata);
        assert_eq!(find(&metadata, "Camera Make"), Some("Acme"));
        assert_eq!(find(&metadata, "Orientation"), Some("Rotated 90\u{b0}"));
        assert_eq!(find(&metadata, "Exposure Time"), Some("1/250 s"));
        assert_eq!(find(&metadata, "Tag 0xBEEF"), Some("7"));
        assert!(
            metadata
                .iter()
                .all(|entry| entry.group() == MetadataGroup::Exif)
        );
    }

    #[test]
    fn gps_coordinates_and_altitude_have_their_references() {
        // An EXIF directory at 8 points to a GPS directory at 26, with values from 80.
        let mut data = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
        data.extend(entry(0x8825, 4, 1, [0, 0, 0, 26]));
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0x04]);
        data.extend(entry(0x01, ASCII, 2, *b"S\0\0\0"));
        data.extend(entry(0x02, 5, 3, [0, 0, 0, 80]));
        data.extend(entry(0x05, 1, 1, [1, 0, 0, 0]));
        data.extend(entry(0x06, 5, 1, [0, 0, 0, 104]));
        data.extend_from_slice(&[0; 4]);
        for (numerator, denominator) in [(52, 1), (30, 1), (36, 1), (25, 2)] {
            data.extend_from_slice(&u32::to_be_bytes(numerator));
            data.extend_from_slice(&u32::to_be_bytes(denominator));
        }
        let mut metadata = Vec::new();
        list_exif(&data, &mut metadata);
        assert_eq!(find(&metadata, "Latitude"), Some("52.51\u{b0} S"));
        assert_eq!(find(&metadata, "Altitude"), Some("-12.5 m"));
        assert_eq!(metadata.len(), 2);
        assert!(
            metadata
                .iter()
                .all(|entry| entry.group() == MetadataGroup::Gps)
        );
    }

    #[test]
    fn directories_pointing_at_each_other_are_listed_once() {
        // The first directory at 8 and the EXIF directory at 26 point at each other.
        let mut data = b"MM\0\x2a\0\0\0\x08\0\x01".to_vec();
        data.extend(entry(EXIF_DIRECTORY, 4, 1, [0, 0, 0, 26]));
        data.extend_from_slice(&[0, 0, 0, 0, 0, 0x02]);
        data.extend(entry(EXIF_DIRECTORY, 4, 1, [0, 0, 0, 8]));
        data.extend(entry(0x8827, 3, 1, [0, 100, 0, 0]));
        data.extend_from_slice(&[0; 4]);
        let mut metadata = Vec::new();
        list_exif(&data, &mut metadata);
        assert_eq!(metadata.len(), 1);
        assert_eq!(find(&metadata, "ISO"), Some("100"));
    }

    #[test]
    fn xmp_properties_are_listed_with_lists_joined() {
        let packet = br#"<?xpacket begin=""?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF>
            <rdf:Description rdf:about="" xmp:CreatorTool="Tool &amp; Co">
            <dc:subject><rdf:Bag><rdf:li>sea</rdf:li><rdf:li>sun</rdf:li></rdf:Bag></dc:subject>
            </rdf:Description></rdf:RDF></x:xmpmeta>"#;
        let mut metadata = Vec::new();
        list_xmp(packet, &mut metadata);
        assert_eq!(find(&metadata, "Creator Tool"), Some("Tool & Co"));
        assert_eq!(find(&metadata, "Subject"), Some("sea, sun"));
        assert_eq!(metadata.len(), 2);
    }

    #[test]
    fn numbers_keep_only_needed_decimals() {
        assert_eq!(decimal(2.80, 1), "2.8");
        assert_eq!(decimal(50.0, 1), "50");
        assert_eq!(exposure_time(0.004), "1/250 s");
        assert_eq!(exposure_time(2.0), "2 s");
        assert_eq!(version(b"0232").as_deref(), Some("2.32"));
        assert_eq!(date("2003:05:16 06:51:43"), "2003-05-16 06:51:43");
    }
}
