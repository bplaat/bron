/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]

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
const FILTER_LINEAR_MIP_LINEAR: u32 = 5;
const MAX_LOD: u32 = 10;

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
    /// 16-bit color, RGB555 for opaque pixels and RGB444 with 3-bit alpha for the others.
    Rgb5a3 = 5,
    /// 32-bit color with alpha.
    Rgba8 = 6,
    /// DXT1 compressed color.
    Cmpr = 14,
}

impl Format {
    const fn tile_size(self) -> u32 {
        match self {
            Self::Rgb5a3 | Self::Rgba8 => 4,
            Self::Cmpr => 8,
        }
    }
}

// MARK: Script
/// The mipmap levels of a script entry, like devkitPro's `minlod`, `maxlod` and `remaplod`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lod {
    /// The first level, the image halved this many times.
    pub min: u32,
    /// The last level.
    pub max: u32,
    /// The LOD number of the first level in the TPL file.
    pub remap: u32,
}

/// An image entry of a script.
#[derive(Debug)]
pub struct Entry {
    /// The image path, resolved against the script's directory.
    pub path: PathBuf,
    /// The texture format.
    pub format: Format,
    /// The mipmap levels, `None` without `mipmap="yes"`.
    pub lod: Option<Lod>,
}

impl Entry {
    /// Loads and converts the entry's image.
    pub fn load(&self) -> Result<Texture> {
        let image = decode(&self.path)?;
        Texture::convert(&self.path, &image, self.format, self.lod)
    }
}

/// Reads the `<filepath="..." id="..." colfmt="..." />` entries of a script, one per line.
/// `mipmap="yes"` with `minlod`, `maxlod` and `remaplod` adds mipmaps like devkitPro's
/// gxtexconv, without `maxlod` that's only the full image.
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
        let mut mipmaps = false;
        let mut lod = Lod::default();
        while !rest.is_empty() {
            let (key, value) = rest.split_once('=').ok_or_else(invalid)?;
            let (value, next) = match value.strip_prefix('"') {
                Some(value) => value.split_once('"').ok_or_else(invalid)?,
                None => value.split_once(char::is_whitespace).unwrap_or((value, "")),
            };
            let level = || {
                value
                    .parse::<u32>()
                    .ok()
                    .filter(|&level| level <= MAX_LOD)
                    .ok_or_else(|| Error(format!("{} must be 0 to {MAX_LOD}: {line}", key.trim())))
            };
            match key.trim() {
                "filepath" => filepath = Some(directory.join(value)),
                "id" => {}
                "colfmt" => {
                    format = match value {
                        "5" => Format::Rgb5a3,
                        "6" => Format::Rgba8,
                        "14" => Format::Cmpr,
                        _ => return Err(Error(format!("unsupported colfmt: {value}"))),
                    }
                }
                // devkitPro treats every value but yes as no
                "mipmap" => mipmaps = value.eq_ignore_ascii_case("yes"),
                "minlod" => lod.min = level()?,
                "maxlod" => lod.max = level()?,
                "remaplod" => lod.remap = level()?,
                key => return Err(Error(format!("unsupported script option: {key}"))),
            }
            rest = next.trim_start();
        }
        let path = filepath.ok_or_else(|| Error(format!("missing filepath: {line}")))?;
        if mipmaps && (lod.min > lod.max || lod.remap + lod.max - lod.min > MAX_LOD) {
            return Err(Error(format!(
                "minlod must be at most maxlod and remaplod + maxlod - minlod at most {MAX_LOD}: {line}"
            )));
        }
        entries.push(Entry {
            path,
            format,
            lod: mipmaps.then_some(lod),
        });
    }
    Ok(entries)
}

// MARK: Texture
/// A converted texture.
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
    /// The LOD number of the first level, written to the TPL file.
    pub first_lod: u32,
    /// The texture data in the GX layout of its format, the levels one after another.
    pub data: Vec<u8>,
}

fn decode(path: &Path) -> Result<image::Image> {
    let bytes =
        fs::read(path).map_err(|err| Error(format!("failed to read {}: {err}", path.display())))?;
    image::decode(&bytes)
        .map_err(|err| Error(format!("failed to decode {}: {err}", path.display())))
}

impl Texture {
    /// Loads any image the image crate decodes and converts it to the format.
    pub fn load(path: &Path, format: Format) -> Result<Self> {
        Self::convert(path, &decode(path)?, format, None)
    }

    /// Loads an image like [`Texture::load`] with the mipmap levels of `lod`.
    pub fn load_lod(path: &Path, format: Format, lod: Lod) -> Result<Self> {
        Self::convert(path, &decode(path)?, format, Some(lod))
    }

    /// Loads an image like [`Texture::load`] and adds mipmaps down to one tile of the format.
    pub fn load_mipmapped(path: &Path, format: Format) -> Result<Self> {
        let image = decode(path)?;
        let tile_size = format.tile_size();
        let mut max = 0;
        while image.width().min(image.height()) >> (max + 1) >= tile_size {
            max += 1;
        }
        let lod = Lod {
            max,
            ..Lod::default()
        };
        Self::convert(path, &image, format, Some(lod))
    }

    fn convert(
        path: &Path,
        image: &image::Image,
        format: Format,
        lod: Option<Lod>,
    ) -> Result<Self> {
        let (width, height) = (image.width(), image.height());
        let tile_size = format.tile_size();
        let Some(lod) = lod else {
            if width % tile_size != 0 || height % tile_size != 0 || width > 1024 || height > 1024 {
                return Err(Error(format!(
                    "{} is {width}x{height}, {format:?} needs a multiple of {tile_size} up to 1024",
                    path.display()
                )));
            }
            return Ok(Self {
                width,
                height,
                format,
                levels: 1,
                first_lod: 0,
                data: encode(format, image.pixels(), width as usize, height as usize),
            });
        };
        // The same size checks as devkitPro's gxtexconv
        if !width.is_power_of_two() || !height.is_power_of_two() || width > 1024 || height > 1024 {
            return Err(Error(format!(
                "{} is {width}x{height}, mipmaps need power of two sizes up to 1024",
                path.display()
            )));
        }
        if width >> lod.max == 0 || height >> lod.max == 0 {
            return Err(Error(format!(
                "{} is {width}x{height}, too small for maxlod {}",
                path.display(),
                lod.max
            )));
        }
        let mut pixels = image.pixels().to_vec();
        let coverage = coverage(&pixels);
        let (mut level_width, mut level_height) = (width as usize, height as usize);
        let mut data = Vec::new();
        for level in 0..=lod.max {
            if level > 0 {
                pixels = downsample(&pixels, level_width, level_height);
                preserve_coverage(&mut pixels, coverage);
                level_width /= 2;
                level_height /= 2;
            }
            if level >= lod.min {
                data.extend(encode(format, &pixels, level_width, level_height));
            }
        }
        Ok(Self {
            width: width >> lod.min,
            height: height >> lod.min,
            format,
            levels: lod.max - lod.min + 1,
            first_lod: lod.remap,
            data,
        })
    }
}

/// Encodes a level in the GX layout of a format. Levels smaller than a tile are padded with
/// their edge pixels, GX always reads whole tiles.
fn encode(format: Format, pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let tile_size = format.tile_size() as usize;
    let (padded_width, padded_height) = (
        width.next_multiple_of(tile_size),
        height.next_multiple_of(tile_size),
    );
    let padded;
    let pixels = if (padded_width, padded_height) == (width, height) {
        pixels
    } else {
        padded = (0..padded_height)
            .flat_map(|y| (0..padded_width).map(move |x| (y.min(height - 1), x.min(width - 1))))
            .flat_map(|(y, x)| pixels[(y * width + x) * 4..][..4].to_vec())
            .collect::<Vec<u8>>();
        &padded
    };
    match format {
        Format::Rgb5a3 => encode_rgb5a3(pixels, padded_width, padded_height),
        Format::Rgba8 => encode_rgba8(pixels, padded_width, padded_height),
        Format::Cmpr => encode_cmpr(pixels, padded_width, padded_height),
    }
}

/// The fraction of pixels that pass an alpha test at half opacity.
fn coverage(pixels: &[u8]) -> f32 {
    let pixels = pixels.as_chunks::<4>().0;
    pixels.iter().filter(|pixel| pixel[3] >= 128).count() as f32 / pixels.len() as f32
}

/// Scales the alpha of a mipmap so as many pixels pass the alpha test as in the full image,
/// otherwise cutouts like leaves fade away in the distance.
fn preserve_coverage(pixels: &mut [u8], coverage: f32) {
    if coverage == 0.0 || coverage == 1.0 {
        return;
    }
    let mut alphas: Vec<_> = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| pixel[3])
        .collect();
    alphas.sort_unstable_by(|a, b| b.cmp(a));
    let target = ((coverage * alphas.len() as f32).round() as usize).clamp(1, alphas.len());
    let threshold = f32::from(alphas[target - 1]);
    if threshold == 0.0 {
        return;
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = (f32::from(pixel[3]) * 128.0 / threshold).round().min(255.0) as u8;
    }
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let value = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (value * 255.0).round() as u8
}

/// Halves an RGBA image by averaging 2x2 pixels in linear light, like GPUs filter sRGB
/// textures. Colors are weighted by alpha, so transparent pixels don't darken the edges of
/// cutouts.
fn downsample(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(width * height);
    for y in (0..height).step_by(2) {
        for x in (0..width).step_by(2) {
            let mut color = [0.0f32; 3];
            let mut alpha = 0.0;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let pixel = &pixels[((y + dy) * width + x + dx) * 4..];
                let weight = f32::from(pixel[3]) / 255.0;
                for channel in 0..3 {
                    color[channel] += srgb_to_linear(pixel[channel]) * weight;
                }
                alpha += weight;
            }
            for channel in color {
                output.push(if alpha > 0.0 {
                    linear_to_srgb(channel / alpha)
                } else {
                    0
                });
            }
            output.push((alpha / 4.0 * 255.0).round() as u8);
        }
    }
    output
}

/// RGB5A3 stores 4x4 tiles of big endian 16-bit pixels, the top bit selects the opaque mode.
fn encode_rgb5a3(pixels: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(width * height * 2);
    for tile_y in (0..height).step_by(4) {
        for tile_x in (0..width).step_by(4) {
            for y in tile_y..tile_y + 4 {
                for x in tile_x..tile_x + 4 {
                    let [r, g, b, a] =
                        [0, 1, 2, 3].map(|c| u16::from(pixels[(y * width + x) * 4 + c]));
                    let value = if a >> 5 == 7 {
                        0x8000 | (r >> 3) << 10 | (g >> 3) << 5 | b >> 3
                    } else {
                        (a >> 5) << 12 | (r >> 4) << 8 | (g >> 4) << 4 | b >> 4
                    };
                    data.extend(value.to_be_bytes());
                }
            }
        }
    }
    data
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
/// Writes textures to a TPL file, power of two textures repeat and all use linear filtering,
/// between mipmaps too.
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
            if texture.levels > 1 {
                FILTER_LINEAR_MIP_LINEAR
            } else {
                FILTER_LINEAR
            },
            FILTER_LINEAR,
        ] {
            tpl.extend(value.to_be_bytes());
        }
        // LOD bias, edge LOD, min LOD, max LOD and padding
        tpl.extend([
            0,
            0,
            0,
            0,
            0,
            texture.first_lod as u8,
            (texture.first_lod + texture.levels - 1) as u8,
            0,
        ]);
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
        .iter()
        .map(Entry::load)
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
        assert!(entries.iter().all(|entry| entry.lod.is_none()));
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

    #[test]
    fn encodes_rgb5a3_opaque_and_translucent_pixels() {
        let mut pixels = [0; 16 * 4];
        pixels[..4].copy_from_slice(&[255, 128, 8, 255]);
        pixels[4..8].copy_from_slice(&[255, 128, 16, 100]);
        let data = encode_rgb5a3(&pixels, 4, 4);
        assert_eq!(data.len(), 32);
        assert_eq!(&data[..4], &[0xfe, 0x01, 0x3f, 0x81]);
        assert_eq!(&data[4..6], &[0, 0]);
    }

    #[test]
    fn mipmaps_go_down_to_one_tile() {
        let image = Path::new(FIXTURES).join("texture.png");
        let base = Texture::load(&image, Format::Cmpr).expect("load texture");
        let mipmapped = Texture::load_mipmapped(&image, Format::Cmpr).expect("load mipmaps");
        assert_eq!(base.levels, 1);
        let mut size = base.data.len();
        let mut expected = 0;
        for _ in 0..mipmapped.levels {
            expected += size;
            size /= 4;
        }
        assert!(mipmapped.levels > 1);
        assert_eq!(mipmapped.width >> (mipmapped.levels - 1), 8);
        assert_eq!(mipmapped.data.len(), expected);
        assert_eq!(&mipmapped.data[..base.data.len()], &base.data[..]);
        // Transparent pixels don't darken their neighbors.
        let pixels = [[200, 100, 50, 255], [0; 4], [0; 4], [0; 4]].concat();
        assert_eq!(downsample(&pixels, 2, 2), [200, 100, 50, 64]);
        // Black and white average in linear light, like the GPU filters sRGB textures
        let pixels = [[0, 0, 0, 255], [255; 4], [0, 0, 0, 255], [255; 4]].concat();
        assert_eq!(downsample(&pixels, 2, 2), [188, 188, 188, 255]);
        // Stripes average to just under half opacity, which would fail the alpha test.
        let mut pixels = downsample(&[[0, 0, 0, 250], [0; 4]].repeat(8).concat(), 4, 4);
        assert_eq!(coverage(&pixels), 0.0);
        preserve_coverage(&mut pixels, 0.5);
        assert_eq!(coverage(&pixels), 1.0);
    }

    #[test]
    fn script_lods_match_devkitpro() {
        let directory = std::env::temp_dir().join(format!("gxtexconv-lod-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("create test directory");
        fs::copy(
            Path::new(FIXTURES).join("texture.png"),
            directory.join("texture.png"),
        )
        .expect("copy texture");
        let script = directory.join("textures.scf");
        let convert = |options: &str| {
            fs::write(
                &script,
                format!("<filepath=\"texture.png\" id=\"texture\" colfmt=\"14\" {options} />\n"),
            )
            .expect("write script");
            read_script(&script).and_then(|entries| entries[0].load())
        };

        // Like devkitPro, mipmap="yes" without maxlod is only the full image
        let texture = convert("mipmap=\"yes\"").expect("convert texture");
        assert_eq!((texture.levels, texture.data.len()), (1, 512));
        let texture = convert("mipmap=\"yes\" maxlod=\"2\"").expect("convert texture");
        assert_eq!(
            (texture.width, texture.levels, texture.first_lod),
            (32, 3, 0)
        );
        assert_eq!(texture.data.len(), 512 + 128 + 32);
        // minlod skips the largest levels and remaplod moves the LOD numbers in the TPL file
        let texture = convert("mipmap=\"yes\" minlod=\"1\" maxlod=\"3\" remaplod=\"2\"")
            .expect("convert texture");
        assert_eq!(
            (texture.width, texture.levels, texture.first_lod),
            (16, 3, 2)
        );
        let tpl = write_tpl(&[texture]);
        let image = u32::from_be_bytes(tpl[12..16].try_into().unwrap()) as usize;
        assert_eq!(
            &tpl[image + 20..image + 24],
            &FILTER_LINEAR_MIP_LINEAR.to_be_bytes()
        );
        assert_eq!(&tpl[image + 33..image + 35], &[2, 4]);
        // Levels smaller than a CMPR tile are padded to one, down to 1x1
        let texture = convert("mipmap=\"yes\" maxlod=\"5\"").expect("convert texture");
        assert_eq!(texture.data.len(), 512 + 128 + 32 * 4);
        // Out of range levels, a 1x1 level of a 32x32 image and minlod above maxlod are errors
        for options in [
            "mipmap=\"yes\" maxlod=\"6\"",
            "mipmap=\"yes\" maxlod=\"11\"",
            "mipmap=\"yes\" minlod=\"3\" maxlod=\"2\"",
            "mipmap=\"yes\" maxlod=\"4\" remaplod=\"7\"",
        ] {
            assert!(convert(options).is_err(), "{options}");
        }
        fs::remove_dir_all(&directory).expect("remove test directory");
    }
}
