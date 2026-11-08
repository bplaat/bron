/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use image::{Bitmap, Compression, EncodeOptions, Format};

use crate::romfs::Node;

mod romfs;

/// The size of the Wii U Menu icon.
pub const ICON_SIZE: (u32, u32) = (128, 128);
/// The size of the splash screen on the TV.
pub const TV_IMAGE_SIZE: (u32, u32) = (1280, 720);
/// The size of the splash screen on the GamePad.
pub const DRC_IMAGE_SIZE: (u32, u32) = (854, 480);

/// A bundle error.
#[derive(Debug)]
pub struct Error(String);

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// What goes in a bundle, the options of devkitPro's wuhbtool.
#[derive(Debug, Default)]
pub struct Options {
    /// The program, stored as `code/root.rpx`.
    pub rpx: PathBuf,
    /// The long name on the Wii U Menu, the short name or the program's file name by default.
    pub name: Option<String>,
    /// The short name on the Wii U Menu, the long name or the program's file name by default.
    pub short_name: Option<String>,
    /// The author, `Built with wut` by default.
    pub author: Option<String>,
    /// The 128x128 icon on the Wii U Menu.
    pub icon: Option<PathBuf>,
    /// The 1280x720 splash screen shown on the TV while the program starts.
    pub tv_image: Option<PathBuf>,
    /// The 854x480 splash screen shown on the GamePad while the program starts.
    pub drc_image: Option<PathBuf>,
    /// A directory with the files of `content/`, which the program reads at runtime.
    pub content: Option<PathBuf>,
}

/// A built bundle with the warnings about its images.
#[derive(Debug)]
pub struct Bundle {
    /// The `.wuhb` file.
    pub data: Vec<u8>,
    /// The images that had the wrong size and were resized.
    pub warnings: Vec<String>,
}

fn read(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|err| Error(format!("failed to read {}: {err}", path.display())))
}

/// The `[menu]` names of `meta/meta.ini`, they default to each other and then to the file name
/// like in wuhbtool. Empty values count as missing, so `bundle.sh` can pass keys that `meta.ini`
/// leaves out.
fn meta_ini(options: &Options) -> String {
    fn given(value: &Option<String>) -> Option<&str> {
        value.as_deref().filter(|value| !value.is_empty())
    }
    let stem = options.rpx.file_stem().map(|stem| stem.to_string_lossy());
    let fallback = || stem.as_deref().unwrap_or_default();
    let (name, short_name) = (given(&options.name), given(&options.short_name));
    format!(
        "[menu]\nlongname={}\nshortname={}\nauthor={}\n",
        name.or(short_name).unwrap_or_else(fallback),
        short_name.or(name).unwrap_or_else(fallback),
        given(&options.author).unwrap_or("Built with wut")
    )
}

/// The weights of the source pixels for each destination pixel, a triangle filter as wide as a
/// destination pixel when shrinking.
fn filter_weights(source: usize, destination: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = source as f32 / destination as f32;
    let support = scale.max(1.0);
    (0..destination)
        .map(|index| {
            let center = (index as f32 + 0.5) * scale - 0.5;
            let first = (center - support).floor().max(0.0) as usize;
            let last = ((center + support).ceil() as usize).min(source - 1);
            let mut weights: Vec<_> = (first..=last)
                .map(|source| {
                    (
                        source,
                        (1.0 - (source as f32 - center).abs() / support).max(0.0),
                    )
                })
                .filter(|(_, weight)| *weight > 0.0)
                .collect();
            if weights.is_empty() {
                weights.push((center.round().clamp(0.0, (source - 1) as f32) as usize, 1.0));
            }
            let total: f32 = weights.iter().map(|(_, weight)| weight).sum();
            weights.iter_mut().for_each(|(_, weight)| *weight /= total);
            weights
        })
        .collect()
}

/// Resizes RGBA pixels with a bilinear filter, weighting the colors by alpha so transparent
/// pixels don't bleed into the edges.
fn resize(pixels: &[u8], width: u32, height: u32, size: (u32, u32)) -> Vec<u8> {
    let (width, height) = (width as usize, height as usize);
    let (new_width, new_height) = (size.0 as usize, size.1 as usize);
    let premultiplied: Vec<[f32; 4]> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&[r, g, b, a]| {
            let alpha = f32::from(a) / 255.0;
            [
                f32::from(r) * alpha,
                f32::from(g) * alpha,
                f32::from(b) * alpha,
                f32::from(a),
            ]
        })
        .collect();
    let sample = |source: &[[f32; 4]], weights: &[(usize, f32)], index: &dyn Fn(usize) -> usize| {
        weights
            .iter()
            .fold([0.0; 4], |mut sum, &(position, weight)| {
                let pixel = source[index(position)];
                for (channel, value) in sum.iter_mut().zip(pixel) {
                    *channel += value * weight;
                }
                sum
            })
    };
    let columns = filter_weights(width, new_width);
    let rows = filter_weights(height, new_height);
    let mut horizontal = Vec::with_capacity(new_width * height);
    for y in 0..height {
        for weights in &columns {
            horizontal.push(sample(&premultiplied, weights, &|x| y * width + x));
        }
    }
    let mut output = Vec::with_capacity(new_width * new_height * 4);
    for weights in &rows {
        for x in 0..new_width {
            let [r, g, b, a] = sample(&horizontal, weights, &|y| y * new_width + x);
            let alpha = a / 255.0;
            let color = |value: f32| {
                if alpha > 0.0 {
                    (value / alpha).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            output.extend([
                color(r),
                color(g),
                color(b),
                a.round().clamp(0.0, 255.0) as u8,
            ]);
        }
    }
    output
}

/// Converts an image to the gzipped TGA the Wii U Menu reads, resized to its size. Images
/// without alpha are drawn over black.
fn tga_gz(
    path: &Path,
    size: (u32, u32),
    alpha: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<u8>> {
    let image = image::decode(&read(path)?)
        .map_err(|err| Error(format!("failed to decode {}: {err}", path.display())))?;
    let mut pixels = if (image.width(), image.height()) == size {
        image.pixels().to_vec()
    } else {
        warnings.push(format!(
            "Image {} has incorrect size (expected {}x{}), resizing...",
            path.display(),
            size.0,
            size.1
        ));
        resize(image.pixels(), image.width(), image.height(), size)
    };
    if !alpha {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let opacity = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * opacity + 127) / 255) as u8;
            }
            pixel[3] = 255;
        }
    }
    let bitmap = Bitmap::new(size.0, size.1, pixels)
        .map_err(|err| Error(format!("failed to convert {}: {err}", path.display())))?;
    // The Wii U Menu only reads uncompressed true-color TGA
    let options = EncodeOptions {
        compression: Compression::None,
        tga_alpha: alpha,
        ..EncodeOptions::default()
    };
    let tga = image::encode_with_options(&bitmap, Format::Tga, options)
        .map_err(|err| Error(format!("failed to convert {}: {err}", path.display())))?;
    Ok(gzip::compress(&tga))
}

/// Reads a directory into nodes, sorted by name so bundles are reproducible.
fn read_directory(path: &Path) -> Result<Vec<Node>> {
    let error = |err: std::io::Error| Error(format!("failed to read {}: {err}", path.display()));
    let mut entries = fs::read_dir(path)
        .map_err(error)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(error)?;
    entries.sort();
    entries
        .iter()
        .map(|entry| {
            let name = entry
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| Error(format!("{} has no UTF-8 name", entry.display())))?
                .to_owned();
            Ok(if entry.is_dir() {
                Node::Directory {
                    name,
                    children: read_directory(entry)?,
                }
            } else {
                Node::File {
                    name,
                    data: read(entry)?,
                }
            })
        })
        .collect()
}

/// Builds a `.wuhb` bundle like devkitPro's wuhbtool: the program in `code/`, the names and
/// images for the Wii U Menu in `meta/` and the files of `content/`.
pub fn build(options: &Options) -> Result<Bundle> {
    let mut warnings = Vec::new();
    let mut meta = vec![Node::File {
        name: "meta.ini".to_owned(),
        data: meta_ini(options).into_bytes(),
    }];
    for (path, name, size, alpha) in [
        (&options.icon, "iconTex.tga.gz", ICON_SIZE, true),
        (&options.tv_image, "bootTvTex.tga.gz", TV_IMAGE_SIZE, false),
        (
            &options.drc_image,
            "bootDrcTex.tga.gz",
            DRC_IMAGE_SIZE,
            false,
        ),
    ] {
        if let Some(path) = path {
            meta.push(Node::File {
                name: name.to_owned(),
                data: tga_gz(path, size, alpha, &mut warnings)?,
            });
        }
    }
    let mut root = vec![
        Node::Directory {
            name: "code".to_owned(),
            children: vec![Node::File {
                name: "root.rpx".to_owned(),
                data: read(&options.rpx)?,
            }],
        },
        Node::Directory {
            name: "meta".to_owned(),
            children: meta,
        },
    ];
    if let Some(content) = &options.content {
        let children = read_directory(content)?;
        if !children.is_empty() {
            root.push(Node::Directory {
                name: "content".to_owned(),
                children,
            });
        }
    }
    Ok(Bundle {
        data: romfs::write(&root),
        warnings,
    })
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    fn be32(data: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
    }

    fn be64(data: &[u8], offset: usize) -> usize {
        u64::from_be_bytes(data[offset..offset + 8].try_into().unwrap()) as usize
    }

    /// Looks up a file through the hash tables, like the RomFS readers of the Wii U.
    fn lookup<'a>(archive: &'a [u8], path: &str) -> Option<&'a [u8]> {
        let [
            dir_hashes,
            dir_hashes_size,
            dirs,
            _,
            file_hashes,
            file_hashes_size,
            files,
            _,
            data,
        ] = std::array::from_fn(|index| be64(archive, 8 + index * 8));
        let name_of = |table: usize, entry: usize, base: usize| {
            let size = be32(archive, table + entry + base - 4) as usize;
            &archive[table + entry + base..table + entry + base + size]
        };
        let (directories, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut parent = 0;
        for component in directories.split('/').filter(|part| !part.is_empty()) {
            let count = dir_hashes_size / 4;
            let bucket = romfs::path_hash(parent, component) as usize % count;
            let mut entry = be32(archive, dir_hashes + bucket * 4);
            while name_of(dirs, entry as usize, 0x18) != component.as_bytes()
                || be32(archive, dirs + entry as usize) != parent
            {
                entry = be32(archive, dirs + entry as usize + 16);
                if entry == u32::MAX {
                    return None;
                }
            }
            parent = entry;
        }
        let bucket = romfs::path_hash(parent, name) as usize % (file_hashes_size / 4);
        let mut entry = be32(archive, file_hashes + bucket * 4) as usize;
        loop {
            if entry == u32::MAX as usize {
                return None;
            }
            if name_of(files, entry, 0x20) == name.as_bytes()
                && be32(archive, files + entry) == parent
            {
                let offset = data + be64(archive, files + entry + 8);
                return Some(&archive[offset..offset + be64(archive, files + entry + 16)]);
            }
            entry = be32(archive, files + entry + 24) as usize;
        }
    }

    fn tga(archive: &[u8], path: &str) -> Vec<u8> {
        gzip::decompress(lookup(archive, path).expect("image in bundle")).expect("gzipped image")
    }

    #[test]
    fn bundles_program_metadata_and_content() {
        let fixtures = Path::new(FIXTURES);
        let bundle = build(&Options {
            rpx: fixtures.join("program.rpx"),
            name: Some("Test Program".to_owned()),
            author: Some("Tester".to_owned()),
            icon: Some(fixtures.join("icon.png")),
            tv_image: Some(fixtures.join("icon.png")),
            drc_image: Some(fixtures.join("icon.png")),
            content: Some(fixtures.join("content")),
            ..Options::default()
        })
        .expect("build bundle");
        let archive = &bundle.data;
        assert_eq!(&archive[..8], b"WUHB\0\0\0\x50");
        assert_eq!(
            lookup(archive, "code/root.rpx"),
            Some(&read(&fixtures.join("program.rpx")).unwrap()[..])
        );
        assert_eq!(
            lookup(archive, "meta/meta.ini"),
            Some(&b"[menu]\nlongname=Test Program\nshortname=Test Program\nauthor=Tester\n"[..])
        );
        assert_eq!(
            lookup(archive, "content/data/level.txt"),
            Some(&b"level\n"[..])
        );
        assert_eq!(
            lookup(archive, "content/README.txt"),
            Some(&b"readme\n"[..])
        );
        assert_eq!(lookup(archive, "content/missing.txt"), None);

        // Uncompressed true color, the icon keeps its alpha, the splash screens are opaque and
        // resized with a warning
        for (path, size, depth) in [
            ("meta/iconTex.tga.gz", ICON_SIZE, 32),
            ("meta/bootTvTex.tga.gz", TV_IMAGE_SIZE, 24),
            ("meta/bootDrcTex.tga.gz", DRC_IMAGE_SIZE, 24),
        ] {
            let tga = tga(archive, path);
            assert_eq!((tga[2], tga[16]), (2, depth), "{path}");
            let image = image::decode(&tga).expect("decode TGA");
            assert_eq!((image.width(), image.height()), size, "{path}");
        }
        assert_eq!(bundle.warnings.len(), 2);
    }

    #[test]
    fn names_default_like_wuhbtool() {
        let options = |name: Option<&str>, short_name: Option<&str>| Options {
            rpx: PathBuf::from("dir/cube.rpx"),
            name: name.map(str::to_owned),
            short_name: short_name.map(str::to_owned),
            ..Options::default()
        };
        assert_eq!(
            meta_ini(&options(None, None)),
            "[menu]\nlongname=cube\nshortname=cube\nauthor=Built with wut\n"
        );
        assert!(meta_ini(&options(Some("Long"), None)).contains("longname=Long\nshortname=Long\n"));
        assert!(
            meta_ini(&options(None, Some("Short"))).contains("longname=Short\nshortname=Short\n")
        );
        // Empty values are missing ones, like the keys bundle.sh reads from a partial meta.ini
        let partial = Options {
            author: Some(String::new()),
            ..options(Some("Long"), Some(""))
        };
        assert_eq!(
            meta_ini(&partial),
            "[menu]\nlongname=Long\nshortname=Long\nauthor=Built with wut\n"
        );
    }

    #[test]
    fn resizes_without_dark_edges() {
        // A white pixel next to a transparent black one keeps its color when shrunk
        let pixels = [[255, 255, 255, 255], [0, 0, 0, 0]].concat();
        assert_eq!(resize(&pixels, 2, 1, (1, 1)), [255, 255, 255, 128]);
        // Growing a solid image keeps it solid
        let pixels = [10, 20, 30, 255].repeat(4);
        assert_eq!(resize(&pixels, 2, 2, (5, 3)), [10, 20, 30, 255].repeat(15));
    }
}
