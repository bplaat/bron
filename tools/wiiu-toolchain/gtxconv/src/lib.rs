/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::addrlib::SurfaceLayout;

mod addrlib;
mod bc1;
mod mipmap;

const GFD_MAGIC: u32 = 0x4766_7832;
const GFD_VERSION_MAJOR: u32 = 7;
const GFD_VERSION_MINOR: u32 = 1;
const GFD_GPU_VERSION: u32 = 2;
const GFD_BLOCK_MAGIC: u32 = 0x424c_4b7b;
const GFD_BLOCK_VERSION_MAJOR: u32 = 1;
const GFD_HEADER_SIZE: usize = 0x20;
const GFD_BLOCK_END_OF_FILE: u32 = 1;
const GFD_BLOCK_PADDING: u32 = 2;
const GFD_BLOCK_TEXTURE_HEADER: u32 = 11;
const GFD_BLOCK_TEXTURE_IMAGE: u32 = 12;
const GFD_BLOCK_TEXTURE_MIPMAP: u32 = 13;

const GX2_SURFACE_DIM_TEXTURE_2D: u32 = 1;
const GX2_SURFACE_USE_TEXTURE: u32 = 1;
/// Selects the red, green, blue and alpha channels, like `GX2_COMP_MAP`.
const GX2_COMP_MAP_RGBA: u32 = 0x0001_0203;

/// A conversion error.
#[derive(Debug)]
pub struct Error(String);

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;

/// A GX2 texture format.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// 32-bit color with alpha, `GX2_SURFACE_FORMAT_UNORM_R8_G8_B8_A8`.
    #[default]
    Rgba8 = 0x1a,
    /// DXT1 compressed color, 4 bits per pixel, `GX2_SURFACE_FORMAT_UNORM_BC1`.
    Bc1 = 0x31,
}

impl Format {
    /// The bits per element, a pixel or a 4x4 block of compressed formats.
    const fn bpp(self) -> u32 {
        match self {
            Self::Rgba8 => 32,
            Self::Bc1 => 64,
        }
    }

    const fn compressed(self) -> bool {
        matches!(self, Self::Bc1)
    }
}

// MARK: Config
/// A texture of a config file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The image path, resolved against the config file's directory.
    pub path: PathBuf,
    /// The texture format, `rgba8` by default.
    #[serde(default)]
    pub format: Format,
    /// Whether to add mipmaps, down to one pixel on the shortest side.
    #[serde(default)]
    pub mipmaps: bool,
}

impl Entry {
    /// Loads and converts the entry's image.
    pub fn load(&self) -> Result<Texture> {
        Texture::load(&self.path, self.format, self.mipmaps)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    textures: Vec<Entry>,
}

/// Reads the `[[textures]]` of a TOML config file, in the order of the textures in the `.gtx`.
pub fn read_config(path: &Path) -> Result<Vec<Entry>> {
    let text = fs::read_to_string(path)
        .map_err(|err| Error(format!("failed to read {}: {err}", path.display())))?;
    let config: Config = basic_toml::from_str(&text)
        .map_err(|err| Error(format!("invalid config {}: {err}", path.display())))?;
    let directory = path.parent().unwrap_or(Path::new(""));
    Ok(config
        .textures
        .into_iter()
        .map(|entry| Entry {
            path: directory.join(entry.path),
            ..entry
        })
        .collect())
}

// MARK: Texture
/// A converted texture in the tiled layout of the GPU, the levels with the alignment and offsets
/// `GX2CalcSurfaceSizeAndAlignment` computes.
#[derive(Debug)]
pub struct Texture {
    /// Width of the first level in pixels.
    pub width: u32,
    /// Height of the first level in pixels.
    pub height: u32,
    /// The texture format.
    pub format: Format,
    /// The number of mipmap levels, 1 without mipmaps.
    pub levels: u32,
    /// The first level, the GPU reads it from `surface.image`.
    pub image: Vec<u8>,
    /// The other levels, the GPU reads them from `surface.mipmaps`.
    pub mipmaps: Vec<u8>,
    layout: SurfaceLayout,
}

impl Texture {
    /// Loads any image the image crate decodes and converts it to the format, with mipmaps down
    /// to one pixel on the shortest side when `mipmaps` is set.
    pub fn load(path: &Path, format: Format, mipmaps: bool) -> Result<Self> {
        let bytes = fs::read(path)
            .map_err(|err| Error(format!("failed to read {}: {err}", path.display())))?;
        let image = image::decode(&bytes)
            .map_err(|err| Error(format!("failed to decode {}: {err}", path.display())))?;
        let (width, height) = (image.width(), image.height());
        if width > 8192 || height > 8192 {
            return Err(Error(format!(
                "{} is {width}x{height}, textures can be up to 8192x8192",
                path.display()
            )));
        }
        if mipmaps && (!width.is_power_of_two() || !height.is_power_of_two()) {
            return Err(Error(format!(
                "{} is {width}x{height}, mipmaps need power of two sizes",
                path.display()
            )));
        }
        let levels = if mipmaps {
            width.min(height).ilog2() + 1
        } else {
            1
        };
        Ok(Self::convert(image.pixels(), width, height, format, levels))
    }

    fn convert(pixels: &[u8], width: u32, height: u32, format: Format, levels: u32) -> Self {
        let layout =
            addrlib::surface_layout(format.bpp(), width, height, levels, format.compressed());
        let mut image = vec![0; layout.image_size as usize];
        let mut mipmaps = vec![0; layout.mipmap_size as usize];
        let coverage = mipmap::coverage(pixels);
        let mut pixels = Cow::Borrowed(pixels);
        for (level, level_layout) in layout.levels.iter().enumerate() {
            let (level_width, level_height) = (width >> level, height >> level);
            if level > 0 {
                let mut downsampled = mipmap::downsample(
                    &pixels,
                    level_width as usize * 2,
                    level_height as usize * 2,
                );
                mipmap::preserve_coverage(&mut downsampled, coverage);
                pixels = Cow::Owned(downsampled);
            }
            let data = match level {
                0 => &mut image[..],
                1 => &mut mipmaps[..],
                _ => &mut mipmaps[layout.mip_level_offset[level - 1] as usize..],
            };
            let element_size = format.bpp() as usize / 8;
            for (x, y, element) in elements(format, &pixels, level_width, level_height) {
                let offset = addrlib::element_offset(level_layout, format.bpp(), x, y);
                data[offset..offset + element_size].copy_from_slice(&element[..element_size]);
            }
        }
        Self {
            width,
            height,
            format,
            levels,
            image,
            mipmaps,
            layout,
        }
    }

    /// The alignment of the image and the mipmaps in memory.
    pub const fn alignment(&self) -> u32 {
        self.layout.alignment
    }

    /// The `GX2Texture` header with big endian fields, the image and mipmap pointers are zero.
    pub fn header(&self) -> Vec<u8> {
        let layout = &self.layout;
        let tile_mode = layout.tile_mode as u32;
        let pitch_pixels = (layout.pitch * if self.format.compressed() { 4 } else { 1 }).max(8);
        // The SQ_TEX_RESOURCE words GX2InitTextureRegs computes, with the last level of the view
        let regs = [
            GX2_SURFACE_DIM_TEXTURE_2D
                | tile_mode << 3
                | (pitch_pixels / 8 - 1) << 8
                | (self.width - 1) << 19,
            (self.height - 1) | (self.format as u32) << 26,
            2 << 14 | 1 << 19 | 2 << 22 | 3 << 25,
            self.levels - 1,
            4 << 2 | 7 << 5 | 2 << 30,
        ];
        let surface = [
            GX2_SURFACE_DIM_TEXTURE_2D,
            self.width,
            self.height,
            1,
            self.levels,
            self.format as u32,
            0,
            GX2_SURFACE_USE_TEXTURE,
            layout.image_size,
            0,
            layout.mipmap_size,
            0,
            tile_mode,
            layout.swizzle,
            layout.alignment,
            layout.pitch,
        ];
        let view = [0, self.levels, 0, 1, GX2_COMP_MAP_RGBA];
        surface
            .iter()
            .chain(&layout.mip_level_offset)
            .chain(&view)
            .chain(&regs)
            .flat_map(|value| value.to_be_bytes())
            .collect()
    }
}

/// The elements of a level row by row with their coordinates: RGBA pixels in the first bytes, or
/// 4x4 blocks with the edge pixels repeated past the level.
fn elements(
    format: Format,
    pixels: &[u8],
    width: u32,
    height: u32,
) -> impl Iterator<Item = (u32, u32, [u8; 8])> {
    let block_size = if format.compressed() { 4 } else { 1 };
    let (columns, rows) = (width.div_ceil(block_size), height.div_ceil(block_size));
    let pixel = move |x: u32, y: u32| {
        let index = (y.min(height - 1) * width + x.min(width - 1)) as usize * 4;
        &pixels[index..index + 4]
    };
    (0..rows).flat_map(move |y| {
        (0..columns).map(move |x| {
            let element = match format {
                Format::Rgba8 => {
                    let mut element = [0; 8];
                    element[..4].copy_from_slice(pixel(x, y));
                    element
                }
                Format::Bc1 => {
                    let colors: [bc1::Rgb; 16] = std::array::from_fn(|i| {
                        let i = i as u32;
                        let pixel = pixel(x * 4 + i % 4, y * 4 + i / 4);
                        [pixel[0], pixel[1], pixel[2]].map(i32::from)
                    });
                    bc1::compress_block(&colors)
                }
            };
            (x, y, element)
        })
    })
}

// MARK: GFD
fn push_block(file: &mut Vec<u8>, block_type: u32, index: usize, data: &[u8]) {
    for value in [
        GFD_BLOCK_MAGIC,
        GFD_HEADER_SIZE as u32,
        GFD_BLOCK_VERSION_MAJOR,
        0,
        block_type,
        data.len() as u32,
        0,
        index as u32,
    ] {
        file.extend(value.to_be_bytes());
    }
    file.extend(data);
}

/// Adds a block with its data aligned in the file, after a padding block when needed.
fn push_aligned_block(
    file: &mut Vec<u8>,
    block_type: u32,
    index: usize,
    data: &[u8],
    alignment: usize,
) {
    if !(file.len() + GFD_HEADER_SIZE).is_multiple_of(alignment) {
        let padding = (alignment - (file.len() + 2 * GFD_HEADER_SIZE) % alignment) % alignment;
        push_block(file, GFD_BLOCK_PADDING, index, &vec![0; padding]);
    }
    push_block(file, block_type, index, data);
}

/// Writes textures to a GFD `.gtx` file that wut's libgfd reads. The image and mipmaps of each
/// texture are aligned in the file, so the GPU can read them in place from a file loaded at the
/// [`Texture::alignment`] of all textures.
pub fn write_gtx(textures: &[Texture]) -> Vec<u8> {
    let mut file = Vec::new();
    for value in [
        GFD_MAGIC,
        GFD_HEADER_SIZE as u32,
        GFD_VERSION_MAJOR,
        GFD_VERSION_MINOR,
        GFD_GPU_VERSION,
        1,
        0,
        0,
    ] {
        file.extend(value.to_be_bytes());
    }
    for (index, texture) in textures.iter().enumerate() {
        let alignment = texture.alignment() as usize;
        push_block(
            &mut file,
            GFD_BLOCK_TEXTURE_HEADER,
            index,
            &texture.header(),
        );
        push_aligned_block(
            &mut file,
            GFD_BLOCK_TEXTURE_IMAGE,
            index,
            &texture.image,
            alignment,
        );
        if !texture.mipmaps.is_empty() {
            push_aligned_block(
                &mut file,
                GFD_BLOCK_TEXTURE_MIPMAP,
                index,
                &texture.mipmaps,
                alignment,
            );
        }
    }
    push_block(&mut file, GFD_BLOCK_END_OF_FILE, 0, &[]);
    file
}

/// Converts the textures of a config file to a `.gtx` file.
pub fn convert_config(path: &Path) -> Result<Vec<u8>> {
    let textures = read_config(path)?
        .iter()
        .map(Entry::load)
        .collect::<Result<Vec<_>>>()?;
    if textures.is_empty() {
        return Err(Error(format!("{} has no textures", path.display())));
    }
    Ok(write_gtx(&textures))
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    fn be(data: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
    }

    /// The blocks of a GFD file: their type, index and the offset and size of their data.
    fn blocks(file: &[u8]) -> Vec<(u32, u32, usize, usize)> {
        let mut blocks = Vec::new();
        let mut offset = be(file, 4) as usize;
        loop {
            assert_eq!(be(file, offset), GFD_BLOCK_MAGIC);
            let data = offset + be(file, offset + 4) as usize;
            let size = be(file, offset + 20) as usize;
            blocks.push((be(file, offset + 16), be(file, offset + 28), data, size));
            if be(file, offset + 16) == GFD_BLOCK_END_OF_FILE {
                return blocks;
            }
            offset = data + size;
        }
    }

    #[test]
    fn reads_config_entries() {
        let entries = read_config(&Path::new(FIXTURES).join("textures.toml")).expect("read config");
        let options: Vec<_> = entries
            .iter()
            .map(|entry| (entry.format, entry.mipmaps))
            .collect();
        assert_eq!(options, [(Format::Bc1, true), (Format::Rgba8, false)]);
        assert!(
            entries
                .iter()
                .all(|entry| entry.path == Path::new(FIXTURES).join("texture.png"))
        );
        let directory = std::env::temp_dir().join(format!("gtxconv-config-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("create test directory");
        let config = directory.join("textures.toml");
        for invalid in [
            "[[textures]]\npath = \"a.png\"\nformat = \"bc3\"\n",
            "[[textures]]\nfile = \"a.png\"\n",
        ] {
            fs::write(&config, invalid).expect("write config");
            assert!(read_config(&config).is_err(), "{invalid}");
        }
        fs::remove_dir_all(&directory).expect("remove test directory");
    }

    #[test]
    fn writes_aligned_texture_blocks() {
        let image = Path::new(FIXTURES).join("texture.png");
        let textures = [
            Texture::load(&image, Format::Bc1, true).expect("load texture"),
            Texture::load(&image, Format::Rgba8, false).expect("load texture"),
        ];
        let file = write_gtx(&textures);
        assert_eq!(&file[..4], b"Gfx2");
        let types: Vec<_> = blocks(&file)
            .iter()
            .filter(|block| block.0 != GFD_BLOCK_PADDING)
            .map(|block| (block.0, block.1))
            .collect();
        assert_eq!(types, [(11, 0), (12, 0), (13, 0), (11, 1), (12, 1), (1, 0)]);
        for (block_type, index, data, size) in blocks(&file) {
            let texture = &textures[index as usize];
            match block_type {
                GFD_BLOCK_TEXTURE_HEADER => assert_eq!(&file[data..data + size], texture.header()),
                GFD_BLOCK_TEXTURE_IMAGE | GFD_BLOCK_TEXTURE_MIPMAP => {
                    assert!(data.is_multiple_of(texture.alignment() as usize));
                }
                _ => {}
            }
        }
    }

    #[test]
    fn header_matches_gx2() {
        let image = Path::new(FIXTURES).join("texture.png");
        let texture = Texture::load(&image, Format::Bc1, true).expect("load texture");
        let header = texture.header();
        assert_eq!(header.len(), 0x9c);
        // The width, height, levels and format, then the regs GX2InitTextureRegs would write
        assert_eq!(
            [4, 8, 16, 20].map(|offset| be(&header, offset)),
            [32, 32, 6, 0x31]
        );
        assert_eq!(be(&header, 0x88 + 4), 31 | 0x31 << 26);
        assert_eq!(be(&header, 0x88 + 12), 5);
    }

    #[test]
    fn compresses_blocks_in_tiles() {
        // One color compresses to the same block everywhere, the tiling only moves blocks
        let pixels = [10, 200, 30, 255].repeat(64 * 64);
        let texture = Texture::convert(&pixels, 64, 64, Format::Bc1, 1);
        // The 16x16 blocks fill half of a macro tiled level padded to 32 blocks wide
        let blocks = texture.image.as_chunks::<8>().0;
        assert_eq!(blocks.len(), 32 * 16);
        let filled = blocks.iter().filter(|block| **block != [0; 8]).count();
        assert_eq!(filled, 16 * 16);
        assert!(
            blocks
                .iter()
                .all(|block| *block == [0; 8] || *block == blocks[0])
        );
    }

    #[test]
    fn rejects_mipmaps_of_odd_sizes() {
        let directory = std::env::temp_dir().join(format!("gtxconv-test-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("create test directory");
        let image = directory.join("odd.qoi");
        // A 3x2 QOI image of one color
        let mut qoi = b"qoif\0\0\0\x03\0\0\0\x02\x04\0".to_vec();
        qoi.extend([0xfe, 10, 20, 30, 0xc0 | 4]);
        qoi.extend([0, 0, 0, 0, 0, 0, 0, 1]);
        fs::write(&image, qoi).expect("write image");
        assert!(Texture::load(&image, Format::Bc1, false).is_ok());
        assert!(Texture::load(&image, Format::Bc1, true).is_err());
        fs::remove_dir_all(&directory).expect("remove test directory");
    }
}
