/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The information an inspector shows about an image: its file, how it is shown and the
//! metadata of the file, in titled sections.

use std::path::Path;
use std::time::Duration;

use image::{LoopCount, Metadata, MetadataGroup};

use crate::{Frame, Media};

/// The format and metadata of an image file.
#[derive(Clone, Default)]
pub struct Info {
    /// The common name of the format, like PNG.
    pub format: &'static str,
    /// The readable metadata in file order.
    pub metadata: Vec<Metadata>,
}

/// A titled section of information, with name and value rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InfoSection {
    /// The title, like General.
    pub title: &'static str,
    /// The names and values, in order.
    pub rows: Vec<(String, String)>,
}

// The titles of the metadata groups, in the order they are shown.
const GROUPS: [(MetadataGroup, &str); 5] = [
    (MetadataGroup::Image, "Details"),
    (MetadataGroup::Exif, "EXIF"),
    (MetadataGroup::Gps, "Location"),
    (MetadataGroup::Text, "Text"),
    (MetadataGroup::Xmp, "XMP"),
];

impl Info {
    /// The sections about the file at a path showing media, the page at an index of a number
    /// of pages: the file and media first, then a section for each group of metadata.
    pub fn sections(
        &self,
        path: &Path,
        media: &Media,
        page: usize,
        pages: usize,
    ) -> Vec<InfoSection> {
        let mut general = vec![(
            "Name".into(),
            path.file_name().map_or_else(
                || path.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            ),
        )];
        if !self.format.is_empty() {
            general.push(("Kind".into(), format!("{} Image", self.format)));
        }
        if let Ok(metadata) = std::fs::metadata(path) {
            general.push(("Size".into(), file_size(metadata.len())));
        }
        let (width, height) = media.size();
        general.push((
            "Dimensions".into(),
            format!(
                "{} \u{d7} {} pixels",
                width.round() as i64,
                height.round() as i64
            ),
        ));
        if pages > 1 {
            general.push(("Page".into(), format!("{} of {pages}", page + 1)));
        }
        if let Media::Raster(raster) = media
            && raster.frames.len() > 1
        {
            let duration: Duration = raster.frames.iter().map(Frame::shown_for).sum();
            general.push(("Frames".into(), raster.frames.len().to_string()));
            general.push((
                "Duration".into(),
                format!("{:.2} s", duration.as_secs_f64()),
            ));
            general.push((
                "Loops".into(),
                match raster.loop_count {
                    LoopCount::Infinite => "Forever".into(),
                    LoopCount::Finite(plays) => plays.to_string(),
                },
            ));
        }
        let mut sections = vec![InfoSection {
            title: "General",
            rows: general,
        }];
        sections.extend(GROUPS.iter().filter_map(|(group, title)| {
            let rows: Vec<_> = self
                .metadata
                .iter()
                .filter(|entry| entry.group() == *group)
                .map(|entry| (entry.name().to_owned(), entry.value().to_owned()))
                .collect();
            (!rows.is_empty()).then_some(InfoSection { title, rows })
        }));
        sections
    }
}

// A file size in decimal units like Finder, with the exact number of bytes.
fn file_size(bytes: u64) -> String {
    let exact = grouped(bytes);
    let (value, unit) = [("KB", 1e3), ("MB", 1e6), ("GB", 1e9)]
        .into_iter()
        .rev()
        .find(|(_, size)| bytes as f64 >= *size)
        .map_or((None, "bytes"), |(unit, size)| {
            (Some(bytes as f64 / size), unit)
        });
    match value {
        Some(value) => format!("{value:.1} {unit} ({exact} bytes)"),
        None => format!("{exact} bytes"),
    }
}

// A number with commas between groups of thousands.
fn grouped(number: u64) -> String {
    let digits = number.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

// MARK: Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode_pages;

    #[test]
    fn sizes_are_readable_and_exact() {
        assert_eq!(file_size(999), "999 bytes");
        assert_eq!(file_size(83_916), "83.9 KB (83,916 bytes)");
        assert_eq!(file_size(1_234_567), "1.2 MB (1,234,567 bytes)");
    }

    #[test]
    fn sections_list_the_file_then_its_metadata() {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../imageview/examples/dice.png"
        ));
        let pages = decode_pages(&std::fs::read(path).expect("read PNG")).expect("PNG");
        let sections = pages.info.sections(path, &pages.pages[0], 0, 1);
        let titles: Vec<_> = sections.iter().map(|section| section.title).collect();
        assert_eq!(titles, ["General", "Details"]);
        let general = &sections[0].rows;
        assert_eq!(general[0], ("Name".into(), "dice.png".into()));
        assert_eq!(general[1], ("Kind".into(), "PNG Image".into()));
        assert!(general.contains(&("Dimensions".into(), "800 \u{d7} 600 pixels".into())));
        assert!(
            sections[1]
                .rows
                .contains(&("Color Type".into(), "RGB with Alpha".into()))
        );
    }
}
