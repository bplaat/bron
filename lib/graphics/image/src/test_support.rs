/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
use std::collections::HashSet;
#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
use crate::{Format, decode};

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
#[derive(Clone, Copy)]
enum PixelTolerance {
    Exact,
    #[cfg(feature = "jpeg")]
    Lossy {
        maximum: u8,
        mean: f64,
    },
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
#[derive(Clone, Copy)]
struct RasterCase {
    directory: &'static str,
    extension: &'static str,
    format: Format,
    inputs: usize,
    references: usize,
    tolerance: PixelTolerance,
}

#[path = "fixture_corpus.rs"]
mod fixture_corpus;
use fixture_corpus::FixtureCorpus;

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
fn raster_cases() -> Vec<RasterCase> {
    vec![
        #[cfg(feature = "qoi")]
        RasterCase {
            directory: "qoi",
            extension: "qoi",
            format: Format::Qoi,
            inputs: 5,
            references: 5,
            tolerance: PixelTolerance::Exact,
        },
        #[cfg(feature = "jpeg")]
        RasterCase {
            directory: "jpeg",
            extension: "jpg",
            format: Format::Jpeg,
            inputs: 4,
            references: 4,
            tolerance: PixelTolerance::Lossy {
                maximum: 4,
                mean: 1.0,
            },
        },
        #[cfg(feature = "png")]
        RasterCase {
            directory: "png",
            extension: "png",
            format: Format::Png,
            inputs: 10,
            references: 18,
            tolerance: PixelTolerance::Exact,
        },
        #[cfg(feature = "gif")]
        RasterCase {
            directory: "gif",
            extension: "gif",
            format: Format::Gif,
            inputs: 4,
            references: 9,
            tolerance: PixelTolerance::Exact,
        },
        #[cfg(feature = "bmp")]
        RasterCase {
            directory: "bmp",
            extension: "bmp",
            format: Format::Bmp,
            inputs: 7,
            references: 7,
            tolerance: PixelTolerance::Exact,
        },
        #[cfg(feature = "ico")]
        RasterCase {
            directory: "ico",
            extension: "ico",
            format: Format::Ico,
            inputs: 4,
            references: 4,
            tolerance: PixelTolerance::Exact,
        },
    ]
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
fn animation_references(corpus: &FixtureCorpus, image: &Path) -> Vec<PathBuf> {
    let relative = corpus.relative_name(image);
    let parent = corpus
        .references
        .join(relative.parent().unwrap_or_else(|| Path::new("")));
    let mut prefix = relative.file_name().expect("fixture file name").to_owned();
    prefix.push(".anim_");
    let mut references = fs::read_dir(parent)
        .expect("read animation reference directory")
        .map(|entry| entry.expect("read animation reference entry").path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.as_encoded_bytes()
                    .starts_with(prefix.as_encoded_bytes())
                    && path
                        .extension()
                        .is_some_and(|extension| extension == "rgba")
            })
        })
        .collect::<Vec<_>>();
    references.sort();
    references
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
fn compare_pixels(name: &Path, actual: &[u8], expected: &[u8], tolerance: PixelTolerance) {
    assert_eq!(actual.len(), expected.len(), "{}", name.display());
    match tolerance {
        PixelTolerance::Exact => {
            let mismatch = actual
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.as_chunks::<4>().0)
                .position(|(actual, expected)| actual != expected);
            assert_eq!(
                mismatch,
                None,
                "{}: first mismatching pixel",
                name.display()
            );
        }
        #[cfg(feature = "jpeg")]
        PixelTolerance::Lossy { maximum, mean } => {
            let mut maximum_error = 0;
            let mut total_error = 0usize;
            for (&actual, &expected) in actual.iter().zip(expected) {
                let error = actual.abs_diff(expected);
                maximum_error = maximum_error.max(error);
                total_error += usize::from(error);
            }
            let mean_error = total_error as f64 / actual.len() as f64;
            assert!(
                maximum_error <= maximum && mean_error < mean,
                "{}: max {maximum_error}, mean {mean_error}",
                name.display()
            );
        }
    }
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
fn run_raster_case(case: RasterCase) {
    let corpus = FixtureCorpus::new(case.directory);
    let images = corpus.image_files(case.extension);
    assert_eq!(images.len(), case.inputs, "{} input count", case.directory);
    let all_references = corpus.reference_files("rgba");
    assert_eq!(
        all_references.len(),
        case.references,
        "{} reference count",
        case.directory
    );
    let mut used_references = HashSet::new();

    for path in images {
        let name = corpus.relative_name(&path);
        let data = fs::read(&path).expect("read raster fixture");
        let image = decode(&data).unwrap_or_else(|error| panic!("{}: {error}", name.display()));
        assert_eq!(image.format(), case.format, "{}", name.display());

        let still = corpus.reference_for(&path, ".rgba");
        let references = if still.is_file() {
            vec![still]
        } else {
            let animation = animation_references(&corpus, &path);
            assert!(
                !animation.is_empty(),
                "{}: missing reference",
                name.display()
            );
            animation
        };
        assert_eq!(image.frames().len(), references.len(), "{}", name.display());
        let expected_len = usize::try_from(image.width())
            .expect("image width")
            .checked_mul(usize::try_from(image.height()).expect("image height"))
            .and_then(|pixels| pixels.checked_mul(4))
            .expect("reference dimensions");

        for (index, (frame, reference)) in image.frames().iter().zip(references).enumerate() {
            assert_eq!(
                frame.bitmap().width(),
                image.width(),
                "{} frame {index}",
                name.display()
            );
            assert_eq!(
                frame.bitmap().height(),
                image.height(),
                "{} frame {index}",
                name.display()
            );
            assert_eq!(
                frame.bitmap().data(),
                frame.pixels(),
                "{} frame {index}",
                name.display()
            );
            let expected = fs::read(&reference).expect("read raster reference");
            assert_eq!(
                expected.len(),
                expected_len,
                "{} frame {index}",
                name.display()
            );
            compare_pixels(name, frame.pixels(), &expected, case.tolerance);
            used_references.insert(reference);
        }
    }

    assert_eq!(used_references, all_references.into_iter().collect());
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
pub(crate) fn run_raster_format(format: &str) {
    let case = raster_cases()
        .into_iter()
        .find(|case| case.directory == format)
        .unwrap_or_else(|| panic!("{format} raster case is enabled"));
    run_raster_case(case);
}

#[cfg(any(
    feature = "qoi",
    feature = "jpeg",
    feature = "png",
    feature = "gif",
    feature = "bmp",
    feature = "ico"
))]
pub(crate) fn raster_inputs() -> Vec<(OsString, Vec<u8>)> {
    raster_cases()
        .into_iter()
        .flat_map(|case| {
            let corpus = FixtureCorpus::new(case.directory);
            corpus
                .image_files(case.extension)
                .into_iter()
                .map(|path| {
                    let name = path.as_os_str().to_owned();
                    let data = fs::read(path).expect("read raster fixture");
                    (name, data)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
