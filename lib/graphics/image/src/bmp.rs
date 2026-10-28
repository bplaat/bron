/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;

use super::{Bitmap, Budget, DecodeError, EncodeError, Format, Image, Reader, Result, pixel_len};

// MARK: Decoder
#[derive(Clone, Copy, Eq, PartialEq)]
enum Compression {
    Rgb,
    Rle8,
    Rle4,
    Rle24,
    Bitfields,
    AlphaBitfields,
}

impl Compression {
    const fn is_rle(self) -> bool {
        matches!(self, Self::Rle8 | Self::Rle4 | Self::Rle24)
    }
}

impl TryFrom<u32> for Compression {
    type Error = DecodeError;

    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::Rgb),
            1 => Ok(Self::Rle8),
            2 => Ok(Self::Rle4),
            3 => Ok(Self::Bitfields),
            6 => Ok(Self::AlphaBitfields),
            _ => Err(DecodeError::UnsupportedFeature),
        }
    }
}

#[derive(Clone, Copy)]
struct ChannelMasks([u32; 4]);

impl ChannelMasks {
    const fn defaults(depth: u16) -> Self {
        if depth == 16 {
            Self([0x7c00, 0x03e0, 0x001f, 0])
        } else {
            Self([0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0])
        }
    }

    fn read(
        depth: u16,
        compression: Compression,
        dib_len: usize,
        header: &mut Reader<'_>,
        file: &mut Reader<'_>,
    ) -> Result<Self> {
        let mut masks = Self::defaults(depth);
        if compression == Compression::Rgb && depth == 32 && dib_len >= 108 {
            header.take(12)?;
            let alpha = header.le32()?;
            if alpha != 0 {
                if alpha != 0xff00_0000 {
                    return Err(DecodeError::InvalidHeader);
                }
                masks.0[3] = alpha;
            }
            return Ok(masks);
        }
        if !matches!(
            compression,
            Compression::Bitfields | Compression::AlphaBitfields
        ) {
            return Ok(masks);
        }
        let has_alpha = dib_len >= 56 || compression == Compression::AlphaBitfields;
        if dib_len >= 52 {
            masks.read_values(header, has_alpha)?;
        } else {
            masks.read_values(file, has_alpha)?;
        }
        masks.validate(depth, compression)?;
        Ok(masks)
    }

    fn read_values(&mut self, source: &mut Reader<'_>, has_alpha: bool) -> Result<()> {
        for mask in &mut self.0[..3] {
            *mask = source.le32()?;
        }
        if has_alpha {
            self.0[3] = source.le32()?;
        }
        Ok(())
    }

    fn validate(self, depth: u16, compression: Compression) -> Result<()> {
        if self.0[..3].contains(&0)
            || (compression == Compression::AlphaBitfields && self.0[3] == 0)
        {
            return Err(DecodeError::InvalidHeader);
        }
        let mut used = 0;
        for mask in self.0 {
            if mask == 0 {
                continue;
            }
            let value = mask >> mask.trailing_zeros();
            if value & value.wrapping_add(1) != 0
                || used & mask != 0
                || (depth < 32 && mask >> depth != 0)
            {
                return Err(DecodeError::InvalidHeader);
            }
            used |= mask;
        }
        Ok(())
    }

    fn component(self, value: u32, channel: usize) -> u8 {
        let mask = self.0[channel];
        let shift = mask.trailing_zeros();
        let max = mask >> shift;
        ((u64::from((value & mask) >> shift) * 255 + u64::from(max / 2)) / u64::from(max)) as u8
    }
}

struct BmpHeader {
    file_size: usize,
    pixel_offset: usize,
    dib_len: usize,
    width: u32,
    height: u32,
    top_down: bool,
    depth: u16,
    compression: Compression,
    #[cfg(feature = "ico")]
    image_size: usize,
    colors: usize,
    masks: ChannelMasks,
    pixel_len: usize,
}

impl BmpHeader {
    fn parse<'a>(data: &'a [u8]) -> Result<(Self, Reader<'a>)> {
        let mut r = Reader::new(data);
        r.take(2)?;
        let file_size = r.le32()? as usize;
        r.take(4)?;
        let pixel_offset = r.le32()? as usize;
        let mut header = Self::parse_dib(&mut r, false, 0)?;
        header.file_size = file_size;
        header.pixel_offset = pixel_offset;
        Ok((header, r))
    }

    fn parse_dib(r: &mut Reader<'_>, icon: bool, icon_colors: usize) -> Result<Self> {
        // CORE and INFO-family headers arrange palettes and masks differently.
        let dib_len = r.le32()? as usize;
        if !matches!(dib_len, 12 | 40 | 52 | 56 | 64 | 108 | 124) {
            return Err(DecodeError::UnsupportedFeature);
        }
        let mut h = Reader::new(r.take(dib_len - 4)?);
        let (width, mut height, top_down) = if dib_len == 12 {
            (u32::from(h.le16()?), u32::from(h.le16()?), false)
        } else {
            let w = h.le32()? as i32;
            let signed_h = h.le32()? as i32;
            if w <= 0 || signed_h == i32::MIN {
                return Err(DecodeError::InvalidHeader);
            }
            (w as u32, signed_h.unsigned_abs(), signed_h < 0)
        };
        if icon {
            if height % 2 != 0 {
                return Err(DecodeError::InvalidHeader);
            }
            height /= 2;
        }
        let pixel_len = pixel_len(width, height)?;
        if h.le16()? != 1 {
            return Err(DecodeError::InvalidHeader);
        }
        let depth = h.le16()?;
        if !matches!(depth, 1 | 4 | 8 | 16 | 24 | 32) {
            return Err(DecodeError::UnsupportedFeature);
        }
        let mut compression = Compression::Rgb;
        let mut _image_size = 0;
        let mut colors = 0;
        if dib_len != 12 {
            let value = h.le32()?;
            compression = if dib_len == 64 && value == 4 {
                Compression::Rle24
            } else {
                Compression::try_from(value)?
            };
            _image_size = h.le32()? as usize;
            h.take(8)?; // Pixels per meter.
            colors = h.le32()? as usize;
            h.le32()?;
        }
        if icon && colors == 0 && icon_colors != 0 {
            colors = icon_colors;
        }
        if (compression == Compression::Rle8 && depth != 8)
            || (compression == Compression::Rle4 && depth != 4)
            || (compression == Compression::Rle24 && depth != 24)
            || (matches!(
                compression,
                Compression::Bitfields | Compression::AlphaBitfields
            ) && !matches!(depth, 16 | 32))
            || (top_down && compression.is_rle())
        {
            return Err(DecodeError::InvalidHeader);
        }
        let mut masks = ChannelMasks::read(depth, compression, dib_len, &mut h, r)?;
        if icon && depth == 32 && compression == Compression::Rgb {
            masks.0[3] = 0xff00_0000;
        }
        Ok(Self {
            file_size: 0,
            pixel_offset: 0,
            dib_len,
            width,
            height,
            top_down,
            depth,
            compression,
            #[cfg(feature = "ico")]
            image_size: _image_size,
            colors,
            masks,
            pixel_len,
        })
    }
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    let (header, mut r) = BmpHeader::parse(data)?;
    let (palette, palette_len) = header.read_palette(&mut r)?;
    if header.pixel_offset < r.pos
        || header.pixel_offset > data.len()
        || (header.file_size != 0
            && (header.file_size > data.len() || header.file_size < header.pixel_offset))
    {
        return Err(DecodeError::InvalidData);
    }
    let end = if header.file_size == 0
        || (header.dib_len == 64 && header.file_size == header.pixel_offset)
    {
        data.len()
    } else {
        header.file_size
    };
    let mut budget = Budget::default();
    let (pixels, _) = header.decode_pixels(
        &data[header.pixel_offset..end],
        &palette[..palette_len],
        &mut budget,
    )?;
    Ok(Image::still(
        Format::Bmp,
        header.width,
        header.height,
        pixels,
    ))
}

pub(super) fn decode_dib(data: &[u8]) -> Result<Image> {
    let mut r = Reader::new(data);
    let header = BmpHeader::parse_dib(&mut r, false, 0)?;
    let (palette, palette_len) = header.read_palette(&mut r)?;
    let mut budget = Budget::default();
    let (pixels, _) = header.decode_pixels(&data[r.pos..], &palette[..palette_len], &mut budget)?;
    Ok(Image::still(
        Format::Bmp,
        header.width,
        header.height,
        pixels,
    ))
}

#[cfg(feature = "ico")]
pub(super) fn decode_icon(
    data: &[u8],
    directory_width: u32,
    directory_height: u32,
    directory_colors: usize,
) -> Result<Image> {
    let mut r = Reader::new(data);
    let header = BmpHeader::parse_dib(&mut r, true, directory_colors)?;
    if header.width != directory_width || header.height != directory_height {
        return Err(DecodeError::InvalidHeader);
    }
    let (palette, palette_len) = header.read_palette(&mut r)?;
    let pixel_start = r.pos;
    let mut budget = Budget::default();
    let (mut pixels, consumed) =
        header.decode_pixels(&data[pixel_start..], &palette[..palette_len], &mut budget)?;
    let xor_len = if header.compression.is_rle() && header.image_size != 0 {
        if header.image_size < consumed {
            return Err(DecodeError::InvalidData);
        }
        header.image_size
    } else {
        consumed
    };
    let mask = data
        .get(
            pixel_start
                .checked_add(xor_len)
                .ok_or(DecodeError::InvalidData)?..,
        )
        .ok_or(DecodeError::InvalidData)?;
    header.apply_icon_mask(mask, &mut pixels)?;
    Ok(Image::still(
        Format::Ico,
        header.width,
        header.height,
        pixels,
    ))
}

impl BmpHeader {
    fn read_palette(&self, r: &mut Reader<'_>) -> Result<([[u8; 4]; 256], usize)> {
        let mut palette = [[0u8, 0, 0, 255]; 256];
        let palette_len = if self.depth <= 8 {
            let max = 1usize << self.depth;
            if self.colors > max {
                return Err(DecodeError::InvalidHeader);
            }
            if self.colors == 0 { max } else { self.colors }
        } else {
            0
        };
        for color in &mut palette[..palette_len] {
            let bgr = r.take(if self.dib_len == 12 { 3 } else { 4 })?;
            color[..3].copy_from_slice(&[bgr[2], bgr[1], bgr[0]]);
        }
        Ok((palette, palette_len))
    }

    fn decode_pixels(
        &self,
        data: &[u8],
        palette: &[[u8; 4]],
        budget: &mut Budget,
    ) -> Result<(Vec<u8>, usize)> {
        let mut r = Reader::new(data);
        let mut pixels = budget.zeroed(self.pixel_len)?;
        if self.compression.is_rle() {
            pixels.as_chunks_mut::<4>().0.fill([0, 0, 0, 255]);
            rle(
                &mut r,
                &mut pixels,
                self.width as usize,
                self.height as usize,
                self.compression,
                palette,
            )?;
        } else {
            let stride =
                usize::try_from((u64::from(self.width) * u64::from(self.depth)).div_ceil(32) * 4)
                    .map_err(|_| DecodeError::ImageTooLarge)?;
            if r.remaining() < stride * self.height as usize {
                return Err(DecodeError::InvalidData);
            }
            for y in 0..self.height as usize {
                let row = r.take(stride)?;
                let dest_y = if self.top_down {
                    y
                } else {
                    self.height as usize - 1 - y
                };
                let dest = &mut pixels
                    [dest_y * self.width as usize * 4..(dest_y + 1) * self.width as usize * 4];
                let dest = dest.as_chunks_mut::<4>().0;
                match self.depth {
                    1 | 4 | 8 => {
                        for (x, pixel) in dest.iter_mut().enumerate() {
                            let bit = x * self.depth as usize;
                            let index = (row[bit / 8] >> (8 - self.depth as usize - bit % 8))
                                & ((1u16 << self.depth) - 1) as u8;
                            pixel.copy_from_slice(
                                palette
                                    .get(index as usize)
                                    .ok_or(DecodeError::InvalidData)?,
                            );
                        }
                    }
                    24 => {
                        for (pixel, s) in dest.iter_mut().zip(row.as_chunks::<3>().0) {
                            pixel.copy_from_slice(&[s[2], s[1], s[0], 255]);
                        }
                    }
                    _ => {
                        for (x, pixel) in dest.iter_mut().enumerate() {
                            let v = if self.depth == 16 {
                                u32::from(u16::from_le_bytes(
                                    row[x * 2..x * 2 + 2].try_into().expect("two bytes"),
                                ))
                            } else {
                                u32::from_le_bytes(
                                    row[x * 4..x * 4 + 4].try_into().expect("four bytes"),
                                )
                            };
                            for (channel, value) in pixel[..3].iter_mut().enumerate() {
                                *value = self.masks.component(v, channel);
                            }
                            pixel[3] = if self.masks.0[3] == 0 {
                                255
                            } else {
                                self.masks.component(v, 3)
                            };
                        }
                    }
                }
            }
        }
        Ok((pixels, r.pos))
    }

    #[cfg(feature = "ico")]
    fn apply_icon_mask(&self, mask: &[u8], pixels: &mut [u8]) -> Result<()> {
        let has_alpha = self.depth == 32
            && self.masks.0[3] != 0
            && pixels.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
        if self.depth == 32 && !has_alpha {
            for pixel in pixels.as_chunks_mut::<4>().0 {
                pixel[3] = 255;
            }
        }
        if has_alpha {
            return Ok(());
        }
        let stride = usize::try_from(u64::from(self.width).div_ceil(32) * 4)
            .map_err(|_| DecodeError::ImageTooLarge)?;
        let len = stride
            .checked_mul(self.height as usize)
            .ok_or(DecodeError::ImageTooLarge)?;
        if mask.len() < len {
            return Err(DecodeError::InvalidData);
        }
        for source_y in 0..self.height as usize {
            let row = &mask[source_y * stride..(source_y + 1) * stride];
            let y = if self.top_down {
                source_y
            } else {
                self.height as usize - 1 - source_y
            };
            for x in 0..self.width as usize {
                if row[x / 8] & (0x80 >> (x % 8)) != 0 {
                    pixels[(y * self.width as usize + x) * 4..][..4].fill(0);
                }
            }
        }
        Ok(())
    }
}

fn rle(
    r: &mut Reader<'_>,
    pixels: &mut [u8],
    width: usize,
    height: usize,
    compression: Compression,
    palette: &[[u8; 4]],
) -> Result<()> {
    // RLE streams alternate encoded runs with escape commands for literals, row ends, and deltas.
    let mut x = 0;
    let mut y = 0;
    loop {
        let count = r.byte()? as usize;
        let value = r.byte()?;
        if count == 0 {
            match value {
                0 => {
                    x = 0;
                    y += 1;
                    if y > height {
                        return Err(DecodeError::InvalidData);
                    }
                }
                1 => return Ok(()),
                2 => {
                    x += r.byte()? as usize;
                    y += r.byte()? as usize;
                    if x > width || y >= height {
                        return Err(DecodeError::InvalidData);
                    }
                }
                _ => {
                    let count = value as usize;
                    if y >= height || count > width - x {
                        return Err(DecodeError::InvalidData);
                    }
                    if compression == Compression::Rle24 {
                        let size = count.checked_mul(3).ok_or(DecodeError::InvalidData)?;
                        let bytes = r.take(size)?;
                        for (i, bgr) in bytes.as_chunks::<3>().0.iter().enumerate() {
                            let offset = ((height - 1 - y) * width + x + i) * 4;
                            pixels[offset..offset + 4]
                                .copy_from_slice(&[bgr[2], bgr[1], bgr[0], 255]);
                        }
                        x += count;
                        if size % 2 != 0 {
                            r.byte()?;
                        }
                        continue;
                    }
                    let size = if compression == Compression::Rle8 {
                        count
                    } else {
                        count.div_ceil(2)
                    };
                    let bytes = r.take(size)?;
                    for i in 0..count {
                        let index = if compression == Compression::Rle8 {
                            bytes[i]
                        } else {
                            (bytes[i / 2] >> (4 - i % 2 * 4)) & 15
                        };
                        put(
                            pixels,
                            ((height - 1 - y) * width + x + i) * 4,
                            index,
                            palette,
                        )?;
                    }
                    x += count;
                    if size % 2 != 0 {
                        r.byte()?;
                    }
                }
            }
        } else {
            if y >= height || count > width - x {
                return Err(DecodeError::InvalidData);
            }
            if compression == Compression::Rle24 {
                let green = r.byte()?;
                let red = r.byte()?;
                for i in 0..count {
                    let offset = ((height - 1 - y) * width + x + i) * 4;
                    pixels[offset..offset + 4].copy_from_slice(&[red, green, value, 255]);
                }
                x += count;
                continue;
            }
            for i in 0..count {
                let index = if compression == Compression::Rle8 {
                    value
                } else {
                    (value >> (4 - i % 2 * 4)) & 15
                };
                put(
                    pixels,
                    ((height - 1 - y) * width + x + i) * 4,
                    index,
                    palette,
                )?;
            }
            x += count;
        }
    }
}

fn put(pixels: &mut [u8], offset: usize, index: u8, palette: &[[u8; 4]]) -> Result<()> {
    pixels[offset..offset + 4].copy_from_slice(
        palette
            .get(index as usize)
            .ok_or(DecodeError::InvalidData)?,
    );
    Ok(())
}

// MARK: Encoder
fn bmp_palette_slot(lookup: &[u64; 512], key: u32) -> usize {
    let mut slot = (key.wrapping_mul(0x9e37_79b1) as usize) & (lookup.len() - 1);
    while lookup[slot] != 0 && lookup[slot] >> 9 != u64::from(key) {
        slot = (slot + 1) & (lookup.len() - 1);
    }
    slot
}

pub(super) fn encode(bitmap: &Bitmap) -> core::result::Result<Vec<u8>, EncodeError> {
    let width = i32::try_from(bitmap.width).map_err(|_| EncodeError::InvalidDimensions)?;
    let height = i32::try_from(bitmap.height).map_err(|_| EncodeError::InvalidDimensions)?;
    let opaque = bitmap.data.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let mut indexed = None;
    if opaque {
        let mut colors = Vec::<[u8; 3]>::with_capacity(256);
        let mut lookup = [0u64; 512];
        let mut indices = Vec::new();
        indices
            .try_reserve_exact(bitmap.data.len() / 4)
            .map_err(|_| EncodeError::ImageTooLarge)?;
        for pixel in bitmap.data.as_chunks::<4>().0 {
            let rgb = [pixel[0], pixel[1], pixel[2]];
            let key = u32::from_be_bytes([0, rgb[0], rgb[1], rgb[2]]);
            let slot = bmp_palette_slot(&lookup, key);
            if lookup[slot] == 0 {
                if colors.len() == 256 {
                    colors.clear();
                    break;
                }
                let index = colors.len();
                colors.push(rgb);
                lookup[slot] = (u64::from(key) << 9) | (index as u64 + 1);
            }
            indices.push((lookup[slot] - 1) as u8);
        }
        if !colors.is_empty() {
            indexed = Some((colors, indices));
        }
    }
    let rgb_stride = u64::from(bitmap.width)
        .checked_mul(3)
        .ok_or(EncodeError::ImageTooLarge)?
        .div_ceil(4)
        * 4;
    let indexed_depth = match indexed.as_ref().map(|(colors, _)| colors.len()) {
        Some(1..=2) => 1u16,
        Some(3..=16) => 4,
        _ => 8,
    };
    let indexed_row_len = (u64::from(bitmap.width) * u64::from(indexed_depth)).div_ceil(8);
    let indexed_stride = indexed_row_len.div_ceil(4) * 4;
    let rle = indexed
        .as_ref()
        .map(|(_, indices)| bmp_rle8(indices, bitmap.width as usize));
    let rgb_size = 54 + rgb_stride * u64::from(bitmap.height);
    let indexed_size = indexed.as_ref().map(|(colors, _)| {
        54 + colors.len() as u64 * 4 + indexed_stride * u64::from(bitmap.height)
    });
    let rle_size = indexed
        .as_ref()
        .zip(rle.as_ref())
        .map(|((colors, _), bytes)| 54 + colors.len() as u64 * 4 + bytes.len() as u64);
    let use_rle =
        rle_size.is_some_and(|size| size < rgb_size && indexed_size.is_none_or(|raw| size < raw));
    let use_indexed = use_rle || indexed_size.is_some_and(|size| size < rgb_size);
    let depth: u16 = if use_rle {
        8
    } else if use_indexed {
        indexed_depth
    } else if opaque {
        24
    } else {
        32
    };
    let stride = match depth {
        1 | 4 | 8 => indexed_stride,
        24 => rgb_stride,
        _ => u64::from(bitmap.width) * 4,
    };
    let palette_len = if use_indexed {
        indexed.as_ref().expect("indexed data").0.len()
    } else {
        0
    };
    let offset = if depth == 32 {
        70u64
    } else {
        54 + palette_len as u64 * 4
    };
    let image_size = if use_rle {
        rle.as_ref().expect("RLE data").len() as u64
    } else {
        stride
            .checked_mul(u64::from(bitmap.height))
            .ok_or(EncodeError::ImageTooLarge)?
    };
    let size = offset
        .checked_add(image_size)
        .ok_or(EncodeError::ImageTooLarge)?;
    let size = u32::try_from(size).map_err(|_| EncodeError::ImageTooLarge)?;
    let mut out = Vec::new();
    out.try_reserve_exact(size as usize)
        .map_err(|_| EncodeError::ImageTooLarge)?;
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(offset as u32).to_le_bytes());
    out.extend_from_slice(&(if depth == 32 { 56u32 } else { 40 }).to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&depth.to_le_bytes());
    out.extend_from_slice(
        &(if depth == 32 {
            3u32
        } else if use_rle {
            1
        } else {
            0
        })
        .to_le_bytes(),
    );
    out.extend_from_slice(&(image_size as u32).to_le_bytes());
    out.extend_from_slice(&[0; 8]);
    out.extend_from_slice(&(palette_len as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    if depth == 32 {
        for mask in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
            out.extend_from_slice(&mask.to_le_bytes());
        }
    }
    if let Some((colors, indices)) = indexed.as_ref().filter(|_| use_indexed) {
        for color in colors {
            out.extend_from_slice(&[color[2], color[1], color[0], 0]);
        }
        if use_rle {
            out.extend_from_slice(rle.as_ref().expect("RLE data"));
        } else {
            for row in indices.chunks_exact(bitmap.width as usize).rev() {
                if depth == 8 {
                    out.extend_from_slice(row);
                } else {
                    let per_byte = 8 / usize::from(depth);
                    for chunk in row.chunks(per_byte) {
                        let mut byte = 0u8;
                        for (i, &index) in chunk.iter().enumerate() {
                            byte |= index << (8 - usize::from(depth) * (i + 1));
                        }
                        out.push(byte);
                    }
                }
                out.resize(out.len() + (stride - indexed_row_len) as usize, 0);
            }
        }
    } else {
        for row in bitmap.data.chunks_exact(bitmap.width as usize * 4).rev() {
            for pixel in row.as_chunks::<4>().0 {
                out.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
                if depth == 32 {
                    out.push(pixel[3]);
                }
            }
            out.resize(
                out.len() + (stride as usize - bitmap.width as usize * usize::from(depth / 8)),
                0,
            );
        }
    }
    Ok(out)
}

fn bmp_run(row: &[u8], limit: usize) -> usize {
    row.iter()
        .take(limit)
        .take_while(|&&value| value == row[0])
        .count()
}

fn bmp_rle8(indices: &[u8], width: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for row in indices.chunks_exact(width).rev() {
        let mut x = 0;
        while x < width {
            // Absolute mode stores distinct indices at one byte each instead of two.
            let mut end = x;
            while end < width && end - x < 255 && bmp_run(&row[end..], 3) < 3 {
                end += 1;
            }
            if end - x >= 3 {
                out.extend_from_slice(&[0, (end - x) as u8]);
                out.extend_from_slice(&row[x..end]);
                if (end - x) % 2 == 1 {
                    out.push(0);
                }
                x = end;
                continue;
            }
            let count = bmp_run(&row[x..], if end > x { end - x } else { 255 });
            out.extend_from_slice(&[count as u8, row[x]]);
            x += count;
        }
        out.extend_from_slice(&[0, 0]);
    }
    out.extend_from_slice(&[0, 1]);
    out
}

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EncodeOptions, EncodingStyle, Format, encode, encode_with_options};

    fn smallest() -> EncodeOptions<'static> {
        EncodeOptions {
            style: EncodingStyle::MaxCompression,
            ..EncodeOptions::default()
        }
    }

    #[test]
    fn compression_values_are_validated() {
        assert!(matches!(
            Compression::try_from(6),
            Ok(Compression::AlphaBitfields)
        ));
        assert!(Compression::try_from(4).is_err());
    }

    fn bitmap(
        width: i32,
        height: i32,
        depth: u16,
        compression: u32,
        masks: &[u32],
        palette: &[[u8; 4]],
        pixels: &[u8],
    ) -> Vec<u8> {
        let offset = 54 + masks.len() * 4 + palette.len() * 4;
        let mut out = b"BM".to_vec();
        out.extend_from_slice(&((offset + pixels.len()) as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&width.to_le_bytes());
        out.extend_from_slice(&height.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&depth.to_le_bytes());
        out.extend_from_slice(&compression.to_le_bytes());
        out.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(&(palette.len() as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        for m in masks {
            out.extend_from_slice(&m.to_le_bytes());
        }
        for p in palette {
            out.extend_from_slice(p);
        }
        out.extend_from_slice(pixels);
        out
    }

    #[test]
    fn decodes_standalone_dib() {
        let bmp = bitmap(1, 1, 24, 0, &[], &[], &[3, 2, 1, 0]);
        assert_eq!(
            decode_dib(&bmp[14..]).expect("standalone DIB").pixels(),
            decode(&bmp).expect("BMP file").pixels()
        );
    }

    #[test]
    fn bmp_depths_bitfields_top_down_and_core_header() {
        let colors = [[0, 0, 255, 0], [0, 255, 0, 0]];
        for depth in [1, 4, 8] {
            let rows = match depth {
                1 => vec![0x40, 0, 0, 0],
                4 => vec![0x01, 0, 0, 0],
                _ => vec![0, 1, 0, 0],
            };
            assert_eq!(
                decode(&bitmap(2, 1, depth, 0, &[], &colors, &rows))
                    .expect("indexed BMP")
                    .pixels(),
                &[255, 0, 0, 255, 0, 255, 0, 255]
            );
        }
        assert_eq!(
            decode(&bitmap(
                1,
                -2,
                24,
                0,
                &[],
                &[],
                &[0, 0, 255, 0, 0, 255, 0, 0]
            ))
            .expect("top down")
            .pixels(),
            &[255, 0, 0, 255, 0, 255, 0, 255]
        );
        assert_eq!(
            decode(&bitmap(1, 1, 16, 0, &[], &[], &[0, 0x7c, 0, 0]))
                .expect("RGB555")
                .pixels(),
            &[255, 0, 0, 255]
        );
        assert_eq!(
            decode(&bitmap(
                1,
                1,
                16,
                3,
                &[0xf800, 0x07e0, 0x001f],
                &[],
                &[0xe0, 0x07, 0, 0]
            ))
            .expect("RGB565")
            .pixels(),
            &[0, 255, 0, 255]
        );
        assert_eq!(
            decode(&bitmap(
                1,
                1,
                32,
                6,
                &[0xff0000, 0xff00, 0xff, 0xff000000],
                &[],
                &[0, 0, 255, 128]
            ))
            .expect("alpha bitfields")
            .pixels(),
            &[255, 0, 0, 128]
        );
        assert_eq!(
            decode(&bitmap(1, 1, 32, 0, &[], &[], &[0, 0, 255, 0]))
                .expect("unused alpha")
                .pixels(),
            &[255, 0, 0, 255]
        );
        assert!(
            decode(&bitmap(
                1,
                1,
                16,
                3,
                &[0xf800, 0xf800, 0x001f],
                &[],
                &[0; 4]
            ))
            .is_err()
        );
        let mut core = b"BM".to_vec();
        core.extend_from_slice(&30u32.to_le_bytes());
        core.extend_from_slice(&[0; 4]);
        core.extend_from_slice(&26u32.to_le_bytes());
        core.extend_from_slice(&12u32.to_le_bytes());
        for v in [1u16, 1, 1, 24] {
            core.extend_from_slice(&v.to_le_bytes());
        }
        core.extend_from_slice(&[0, 0, 255, 0]);
        assert_eq!(
            decode(&core).expect("CORE bitmap").pixels(),
            &[255, 0, 0, 255]
        );
    }

    #[test]
    fn bmp_rle_runs_literals_deltas_and_padding() {
        let palette = [[0, 0, 0, 0], [0, 0, 255, 0], [0, 255, 0, 0], [255, 0, 0, 0]];
        for (depth, compression, bytes) in [
            (8, 1, vec![0, 3, 1, 2, 3, 0, 0, 0, 0, 2, 1, 0, 2, 1, 0, 1]),
            (
                4,
                2,
                vec![0, 3, 0x12, 0x30, 0, 0, 0, 2, 1, 0, 2, 0x11, 0, 1],
            ),
        ] {
            let image = decode(&bitmap(3, 2, depth, compression, &[], &palette, &bytes))
                .expect("RLE bitmap");
            assert_eq!(
                image.pixels(),
                &[
                    0, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255,
                    0, 0, 255, 255
                ]
            );
        }
        assert!(decode(&bitmap(1, 1, 8, 1, &[], &palette, &[2, 1, 0, 1])).is_err());

        let sparse_palette = [[255, 0, 128, 0], [0, 0, 255, 0]];
        assert_eq!(
            decode(&bitmap(
                2,
                1,
                8,
                1,
                &[],
                &sparse_palette,
                &[0, 2, 1, 0, 1, 1, 0, 1]
            ))
            .expect("sparse RLE bitmap")
            .pixels(),
            &[0, 0, 0, 255, 255, 0, 0, 255]
        );
    }

    #[test]
    fn bmp_v4_v5_embedded_masks() {
        for header_len in [108u32, 124] {
            let mut data = bitmap(
                1,
                1,
                32,
                3,
                &[0xff0000, 0xff00, 0xff, 0xff000000],
                &[],
                &[1, 2, 3, 128],
            );
            data.splice(70..70, core::iter::repeat_n(0, (header_len - 56) as usize));
            let len = data.len() as u32;
            data[2..6].copy_from_slice(&len.to_le_bytes());
            data[10..14].copy_from_slice(&(14 + header_len).to_le_bytes());
            data[14..18].copy_from_slice(&header_len.to_le_bytes());
            assert_eq!(
                decode(&data).expect("V4/V5 bitmap").pixels(),
                &[3, 2, 1, 128]
            );
        }

        let mut data = bitmap(1, 1, 32, 0, &[], &[], &[1, 2, 3, 128]);
        data.splice(54..54, core::iter::repeat_n(0, 84));
        let len = data.len() as u32;
        data[2..6].copy_from_slice(&len.to_le_bytes());
        data[10..14].copy_from_slice(&138u32.to_le_bytes());
        data[14..18].copy_from_slice(&124u32.to_le_bytes());
        data[66..70].copy_from_slice(&0xff00_0000u32.to_le_bytes());
        assert_eq!(
            decode(&data).expect("V5 BI_RGB alpha").pixels(),
            &[3, 2, 1, 128]
        );
    }

    #[test]
    fn bmp_os2_rle24() {
        let pixels = [2, 3, 2, 1, 0, 1];
        let offset = 78u32;
        let mut data = b"BM".to_vec();
        data.extend_from_slice(&offset.to_le_bytes());
        data.extend_from_slice(&[0; 4]);
        data.extend_from_slice(&offset.to_le_bytes());
        data.extend_from_slice(&64u32.to_le_bytes());
        data.extend_from_slice(&2i32.to_le_bytes());
        data.extend_from_slice(&1i32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&24u16.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&(pixels.len() as u32).to_le_bytes());
        data.extend_from_slice(&[0; 40]);
        data.extend_from_slice(&pixels);
        assert_eq!(
            decode(&data).expect("OS/2 RLE24 bitmap").pixels(),
            &[1, 2, 3, 255, 1, 2, 3, 255]
        );
    }

    #[cfg(feature = "bmp")]
    #[test]
    fn bmp_packs_indexed_pixels_in_both_styles() {
        let bitmap = Bitmap::new(32, 16, [255, 0, 0, 255].repeat(512)).unwrap();
        let normal = encode(&bitmap, Format::Bmp).unwrap();
        let small = encode_with_options(&bitmap, Format::Bmp, smallest()).unwrap();
        assert_eq!(small, normal);
        assert_eq!(u16::from_le_bytes([small[28], small[29]]), 1);
        assert_eq!(u32::from_le_bytes(small[30..34].try_into().unwrap()), 0);
        assert_eq!(crate::decode(&small).unwrap().pixels(), bitmap.data());

        let mut pixels = Vec::new();
        for i in 0..256u16 {
            pixels.extend_from_slice(&[(i % 4) as u8 * 60, 0, 0, 255]);
        }
        let bitmap = Bitmap::new(32, 8, pixels).unwrap();
        let small = encode_with_options(&bitmap, Format::Bmp, smallest()).unwrap();
        assert_eq!(u16::from_le_bytes([small[28], small[29]]), 4);
        assert_eq!(crate::decode(&small).unwrap().pixels(), bitmap.data());
    }

    #[test]
    fn bmp_rle8_uses_absolute_mode_for_distinct_indices() {
        let row = [1, 2, 3, 4, 5, 9, 9, 9, 9, 6, 7, 7, 8];
        assert_eq!(
            bmp_rle8(&row, row.len()),
            [0, 5, 1, 2, 3, 4, 5, 0, 4, 9, 0, 4, 6, 7, 7, 8, 0, 0, 0, 1]
        );

        let mut pixels = Vec::new();
        for i in 0..32 * 32 {
            let index = if i % 32 < 8 { i % 20 } else { 3 };
            pixels.extend_from_slice(&[index as u8 * 12, 0, 0, 255]);
        }
        let bitmap = Bitmap::new(32, 32, pixels).unwrap();
        let encoded = encode(&bitmap, Format::Bmp).unwrap();
        assert_eq!(u32::from_le_bytes(encoded[30..34].try_into().unwrap()), 1);
        assert_eq!(crate::decode(&encoded).unwrap().pixels(), bitmap.data());
    }

    #[cfg(feature = "bmp")]
    #[test]
    fn bmp_keeps_packed_indexed_when_rle_grows() {
        let mut pixels = Vec::new();
        for i in 0..1024 {
            pixels.extend_from_slice(if i % 2 == 0 {
                &[255, 0, 0, 255]
            } else {
                &[0, 0, 255, 255]
            });
        }
        let bitmap = Bitmap::new(32, 32, pixels).unwrap();
        let small = encode_with_options(&bitmap, Format::Bmp, smallest()).unwrap();
        assert_eq!(u16::from_le_bytes([small[28], small[29]]), 1);
        assert_eq!(u32::from_le_bytes(small[30..34].try_into().unwrap()), 0);
        assert_eq!(crate::decode(&small).unwrap().pixels(), bitmap.data());
    }

    #[cfg(feature = "bmp")]
    #[test]
    fn compact_bmp_uses_24_bit_for_opaque_photos() {
        let mut pixels = Vec::new();
        for i in 0..300u16 {
            pixels.extend_from_slice(&[(i & 255) as u8, (i >> 8) as u8, 17, 255]);
        }
        let bitmap = Bitmap::new(20, 15, pixels).unwrap();
        let normal = encode(&bitmap, Format::Bmp).unwrap();
        assert_eq!(
            EncodeOptions::default().style,
            EncodingStyle::NormalCompression
        );
        assert_eq!(u16::from_le_bytes([normal[28], normal[29]]), 24);
        assert_eq!(crate::decode(&normal).unwrap().pixels(), bitmap.data());
    }
}
