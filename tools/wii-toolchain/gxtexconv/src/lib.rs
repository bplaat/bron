/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for devkitPro's gxtexconv, converts the images of a `.scf` script to
//! GX textures in the RGBA8 or CMPR format and writes them to a `.tpl` texture file.

use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

mod dxt1;

const TPL_VERSION: u32 = 0x0020_af30;
const TPL_HEADER_SIZE: usize = 12;
const TPL_TEXTURE_DESC_SIZE: usize = 8;
const TPL_IMAGE_DESC_SIZE: usize = 36;

const WRAP_CLAMP: u32 = 0;
const WRAP_REPEAT: u32 = 1;
const FILTER_LINEAR: u32 = 1;

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

/// A GX texture format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Format {
    /// 32-bit color with alpha.
    Rgba8 = 6,
    /// DXT1 compressed color.
    Cmpr = 14,
}

impl Format {
    const fn tile_size(self) -> u32 {
        match self {
            Self::Rgba8 => 4,
            Self::Cmpr => 8,
        }
    }
}

// MARK: Script
/// An image entry of a script.
#[derive(Debug)]
pub struct Entry {
    /// The image path, resolved against the script's directory.
    pub path: PathBuf,
    /// The texture format.
    pub format: Format,
}

/// Reads the `<filepath="..." id="..." colfmt="..." />` entries of a script, one per line.
pub fn read_script(path: &Path) -> Result<Vec<Entry>> {
    let text = fs::read_to_string(path)
        .map_err(|err| Error(format!("failed to read {}: {err}", path.display())))?;
    let directory = path.parent().unwrap_or(Path::new(""));
    let mut entries = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let invalid = || Error(format!("invalid script line: {line}"));
        let mut rest = line
            .strip_prefix('<')
            .and_then(|line| line.strip_suffix("/>"))
            .ok_or_else(invalid)?
            .trim();
        let mut filepath = None;
        let mut format = Format::Rgba8;
        while !rest.is_empty() {
            let (key, value) = rest.split_once('=').ok_or_else(invalid)?;
            let (value, next) = match value.strip_prefix('"') {
                Some(value) => value.split_once('"').ok_or_else(invalid)?,
                None => value.split_once(char::is_whitespace).unwrap_or((value, "")),
            };
            match key.trim() {
                "filepath" => filepath = Some(directory.join(value)),
                "id" => {}
                "colfmt" => {
                    format = match value {
                        "6" => Format::Rgba8,
                        "14" => Format::Cmpr,
                        _ => return Err(Error(format!("unsupported colfmt: {value}"))),
                    }
                }
                key => return Err(Error(format!("unsupported script option: {key}"))),
            }
            rest = next.trim_start();
        }
        let path = filepath.ok_or_else(|| Error(format!("missing filepath: {line}")))?;
        entries.push(Entry { path, format });
    }
    Ok(entries)
}

// MARK: Texture
/// A converted texture.
#[derive(Debug)]
pub struct Texture {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The texture format.
    pub format: Format,
    /// The texture data in the GX layout of its format.
    pub data: Vec<u8>,
}

impl Texture {
    /// Loads any image the image crate decodes and converts it to the format.
    pub fn load(path: &Path, format: Format) -> Result<Self> {
        let bytes = fs::read(path)
            .map_err(|err| Error(format!("failed to read {}: {err}", path.display())))?;
        let image = image::decode(&bytes)
            .map_err(|err| Error(format!("failed to decode {}: {err}", path.display())))?;
        let (width, height) = (image.width(), image.height());
        let tile_size = format.tile_size();
        if width % tile_size != 0 || height % tile_size != 0 || width > 1024 || height > 1024 {
            return Err(Error(format!(
                "{} is {width}x{height}, {format:?} needs a multiple of {tile_size} up to 1024",
                path.display()
            )));
        }
        let encode = match format {
            Format::Rgba8 => encode_rgba8,
            Format::Cmpr => encode_cmpr,
        };
        Ok(Self {
            width,
            height,
            format,
            data: encode(image.pixels(), width as usize, height as usize),
        })
    }
}

/// RGBA8 stores 4x4 tiles as 16 alpha red pairs and then 16 green blue pairs.
fn encode_rgba8(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * 4);
    for tile_y in (0..height).step_by(4) {
        for tile_x in (0..width).step_by(4) {
            for channels in [[3, 0], [1, 2]] {
                for y in tile_y..tile_y + 4 {
                    for x in tile_x..tile_x + 4 {
                        let pixel = &pixels[(y * width + x) * 4..];
                        data.extend(channels.map(|channel| pixel[channel]));
                    }
                }
            }
        }
    }
    data
}

/// CMPR stores 8x8 tiles of four DXT1 blocks.
fn encode_cmpr(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height / 2);
    for tile_y in (0..height).step_by(8) {
        for tile_x in (0..width).step_by(8) {
            for block in 0..4 {
                let block_x = tile_x + (block & 1) * 4;
                let block_y = tile_y + (block >> 1) * 4;
                let colors: [dxt1::Rgb; 16] = std::array::from_fn(|i| {
                    let pixel = &pixels[((block_y + i / 4) * width + block_x + i % 4) * 4..];
                    [pixel[0] as i32, pixel[1] as i32, pixel[2] as i32]
                });
                data.extend(dxt1::compress_block(&colors));
            }
        }
    }
    data
}

// MARK: TPL
/// Writes textures to a TPL file, power of two textures repeat and all use linear filtering.
pub fn write_tpl(textures: &[Texture]) -> Vec<u8> {
    let images_offset = TPL_HEADER_SIZE + textures.len() * TPL_TEXTURE_DESC_SIZE;
    let mut data_offset =
        (images_offset + textures.len() * TPL_IMAGE_DESC_SIZE).next_multiple_of(32);
    let mut tpl = Vec::new();
    for value in [TPL_VERSION, textures.len() as u32, TPL_HEADER_SIZE as u32] {
        tpl.extend(value.to_be_bytes());
    }
    for index in 0..textures.len() {
        let image_offset = images_offset + index * TPL_IMAGE_DESC_SIZE;
        tpl.extend((image_offset as u32).to_be_bytes());
        tpl.extend(0u32.to_be_bytes()); // No palette
    }
    for texture in textures {
        let wrap = if texture.width.is_power_of_two() && texture.height.is_power_of_two() {
            WRAP_REPEAT
        } else {
            WRAP_CLAMP
        };
        tpl.extend((texture.height as u16).to_be_bytes());
        tpl.extend((texture.width as u16).to_be_bytes());
        for value in [
            texture.format as u32,
            data_offset as u32,
            wrap,
            wrap,
            FILTER_LINEAR,
            FILTER_LINEAR,
        ] {
            tpl.extend(value.to_be_bytes());
        }
        // LOD bias, edge LOD, min LOD, max LOD and padding
        tpl.extend([0; 8]);
        data_offset += texture.data.len();
    }
    tpl.resize(tpl.len().next_multiple_of(32), 0);
    for texture in textures {
        tpl.extend(&texture.data);
    }
    tpl
}

/// Converts the images of a script to a TPL file.
pub fn convert_script(path: &Path) -> Result<Vec<u8>> {
    let textures = read_script(path)?
        .into_iter()
        .map(|entry| Texture::load(&entry.path, entry.format))
        .collect::<Result<Vec<_>>>()?;
    if textures.is_empty() {
        return Err(Error(format!("{} has no images", path.display())));
    }
    Ok(write_tpl(&textures))
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    // The header and RGBA8 texture of textures.tpl are the same as devkitPro's gxtexconv writes,
    // the CMPR texture uses our better compressor
    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    #[test]
    fn converts_script() {
        let script = Path::new(FIXTURES).join("textures.scf");
        let expected = fs::read(Path::new(FIXTURES).join("textures.tpl")).expect("read expected");
        assert!(convert_script(&script).expect("convert script") == expected);
    }

    #[test]
    fn reads_script_entries() {
        let entries = read_script(&Path::new(FIXTURES).join("textures.scf")).expect("read script");
        let formats: Vec<_> = entries.iter().map(|entry| entry.format).collect();
        assert_eq!(formats, [Format::Rgba8, Format::Cmpr]);
        assert!(
            entries
                .iter()
                .all(|entry| entry.path == Path::new(FIXTURES).join("texture.png"))
        );
    }

    #[test]
    fn rejects_unaligned_cmpr_size() {
        let directory = std::env::temp_dir().join(format!("gxtexconv-test-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("create test directory");
        let image = directory.join("small.qoi");
        // A 4x4 QOI image of one color, CMPR needs a multiple of 8
        let mut qoi = b"qoif\0\0\0\x04\0\0\0\x04\x04\0".to_vec();
        qoi.extend([0xfe, 10, 20, 30, 0xc0 | 14]);
        qoi.extend([0, 0, 0, 0, 0, 0, 0, 1]);
        fs::write(&image, qoi).expect("write image");
        assert!(Texture::load(&image, Format::Rgba8).is_ok());
        assert!(Texture::load(&image, Format::Cmpr).is_err());
        fs::remove_dir_all(&directory).expect("remove test directory");
    }
}
