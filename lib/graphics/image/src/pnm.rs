/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use super::{Budget, DecodeError, Format, Image, Result};

struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn skip(&mut self) {
        loop {
            self.data = self.data.trim_ascii_start();
            if self.data.first() != Some(&b'#') {
                break;
            }
            self.data = &self.data[self
                .data
                .iter()
                .position(|b| matches!(b, b'\r' | b'\n'))
                .unwrap_or(self.data.len())..];
        }
    }

    fn number(&mut self) -> Result<u32> {
        self.skip();
        let end = self
            .data
            .iter()
            .position(|b| !b.is_ascii_digit())
            .unwrap_or(self.data.len());
        let value = decimal(&self.data[..end])?;
        self.data = &self.data[end..];
        if self
            .data
            .first()
            .is_some_and(|b| !b.is_ascii_whitespace() && *b != b'#')
        {
            return Err(DecodeError::InvalidData);
        }
        Ok(value)
    }

    fn separator(&mut self) -> Result<()> {
        // Consume exactly one delimiter: raster bytes may themselves be whitespace or '#'.
        if self.data.first() == Some(&b'#') {
            let end = self
                .data
                .iter()
                .position(|b| matches!(b, b'\r' | b'\n'))
                .ok_or(DecodeError::InvalidData)?;
            self.data = &self.data[end..];
        }
        if !self.data.first().is_some_and(u8::is_ascii_whitespace) {
            return Err(DecodeError::InvalidData);
        }
        self.data = &self.data[1..];
        Ok(())
    }

    fn line(&mut self) -> Result<&'a [u8]> {
        let end = self
            .data
            .iter()
            .position(|b| *b == b'\n')
            .ok_or(DecodeError::InvalidHeader)?;
        let line = self.data[..end].trim_ascii();
        self.data = &self.data[end + 1..];
        Ok(line)
    }

    fn sample(&mut self, plain: bool, max: u32) -> Result<u8> {
        let value = if plain {
            self.number()?
        } else {
            let size = if max < 256 { 1 } else { 2 };
            let bytes = self.data.get(..size).ok_or(DecodeError::InvalidData)?;
            let value = if size == 1 {
                u32::from(bytes[0])
            } else {
                u32::from(u16::from_be_bytes([bytes[0], bytes[1]]))
            };
            self.data = &self.data[size..];
            value
        };
        if value > max {
            return Err(DecodeError::InvalidData);
        }
        Ok(((value * 255 + max / 2) / max) as u8)
    }
}

fn decimal(bytes: &[u8]) -> Result<u32> {
    if bytes.is_empty() {
        return Err(DecodeError::InvalidData);
    }
    bytes.iter().try_fold(0u32, |value, b| {
        if !b.is_ascii_digit() {
            return Err(DecodeError::InvalidData);
        }
        value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u32::from(b - b'0')))
            .ok_or(DecodeError::InvalidData)
    })
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    if Format::from_signature(data) != Some(Format::Pnm) {
        return Err(DecodeError::InvalidMagic);
    }
    let kind = data[1];
    let mut reader = Reader { data: &data[2..] };
    let (width, height, depth, max) = if kind == b'7' {
        if !reader.line()?.is_empty() {
            return Err(DecodeError::InvalidHeader);
        }
        let mut fields = [None; 4];
        let mut tuple: Option<&[u8]> = None;
        loop {
            let line = reader.line()?;
            if line.is_empty() || line.starts_with(b"#") {
                continue;
            }
            if line == b"ENDHDR" {
                break;
            }
            let split = line
                .iter()
                .position(u8::is_ascii_whitespace)
                .ok_or(DecodeError::InvalidHeader)?;
            let (key, value) = (&line[..split], line[split..].trim_ascii());
            let field = match key {
                b"WIDTH" => 0,
                b"HEIGHT" => 1,
                b"DEPTH" => 2,
                b"MAXVAL" => 3,
                b"TUPLTYPE" => {
                    if tuple.replace(value).is_some() {
                        return Err(DecodeError::UnsupportedFeature);
                    }
                    continue;
                }
                _ => return Err(DecodeError::InvalidHeader),
            };
            if fields[field].replace(decimal(value)?).is_some() {
                return Err(DecodeError::InvalidHeader);
            }
        }
        let [Some(width), Some(height), Some(depth), Some(max)] = fields else {
            return Err(DecodeError::InvalidHeader);
        };
        if let Some(tuple) = tuple {
            let expected = match tuple {
                b"BLACKANDWHITE" | b"BLACKANDWHITE_ALPHA" => {
                    if max != 1 {
                        return Err(DecodeError::InvalidHeader);
                    }
                    if tuple == b"BLACKANDWHITE" { 1 } else { 2 }
                }
                b"GRAYSCALE" => 1,
                b"GRAYSCALE_ALPHA" => 2,
                b"RGB" => 3,
                b"RGB_ALPHA" => 4,
                _ => return Err(DecodeError::UnsupportedFeature),
            };
            if depth != expected {
                return Err(DecodeError::InvalidHeader);
            }
        }
        (width, height, depth, max)
    } else {
        let width = reader.number()?;
        let height = reader.number()?;
        let max = if matches!(kind, b'1' | b'4') {
            1
        } else {
            reader.number()?
        };
        if kind >= b'4' {
            reader.separator()?;
        }
        (
            width,
            height,
            if matches!(kind, b'3' | b'6') { 3 } else { 1 },
            max,
        )
    };
    if width == 0 || height == 0 || !(1..=65535).contains(&max) || depth == 0 {
        return Err(DecodeError::InvalidHeader);
    }
    if depth > 4 {
        return Err(DecodeError::UnsupportedFeature);
    }
    let count = usize::try_from(u64::from(width) * u64::from(height))
        .map_err(|_| DecodeError::ImageTooLarge)?;
    let len = count.checked_mul(4).ok_or(DecodeError::ImageTooLarge)?;
    let mut pixels = Budget::default().zeroed::<u8>(len)?;
    let stride = (width as usize).div_ceil(8);
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if kind == b'4' {
            let byte = reader
                .data
                .get(index / width as usize * stride + index % width as usize / 8)
                .ok_or(DecodeError::InvalidData)?;
            let gray = if byte & (0x80 >> (index % width as usize % 8)) == 0 {
                255
            } else {
                0
            };
            *pixel = [gray, gray, gray, 255];
        } else if kind == b'1' {
            reader.skip();
            let gray = match reader.data.first() {
                Some(b'0') => 255,
                Some(b'1') => 0,
                _ => return Err(DecodeError::InvalidData),
            };
            reader.data = &reader.data[1..];
            *pixel = [gray, gray, gray, 255];
        } else {
            let plain = kind <= b'3';
            let first = reader.sample(plain, max)?;
            *pixel = if depth <= 2 {
                [
                    first,
                    first,
                    first,
                    if depth == 2 {
                        reader.sample(plain, max)?
                    } else {
                        255
                    },
                ]
            } else {
                [
                    first,
                    reader.sample(plain, max)?,
                    reader.sample(plain, max)?,
                    if depth == 4 {
                        reader.sample(plain, max)?
                    } else {
                        255
                    },
                ]
            };
        }
    }
    Ok(Image::still(Format::Pnm, width, height, pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants() {
        for data in [
            &b"P1\n2 1\n01"[..],
            &b"P2\n# comment\n2 1\n15\n15 0"[..],
            &b"P3\n2 1\n255\n255 255 255 0 0 0"[..],
            &b"P4\n2 1\n\x40"[..],
            &b"P5\n2 1\n65535\n\xff\xff\0\0"[..],
            &b"P6\n2 1\n255\n\xff\xff\xff\0\0\0"[..],
            &b"P7\nHEIGHT 1\nWIDTH 2\nMAXVAL 1\nDEPTH 1\nTUPLTYPE BLACKANDWHITE\nENDHDR\n\x01\0"[..],
        ] {
            let image = super::super::decode(data).expect("decode Netpbm");
            assert_eq!(image.format(), Format::Pnm);
            assert_eq!(
                image.frames()[0].pixels(),
                [255, 255, 255, 255, 0, 0, 0, 255]
            );
        }
        for extension in ["pbm", "pgm", "ppm", "pnm", "PAM"] {
            assert_eq!(Format::from_extension(extension), Some(Format::Pnm));
        }
        assert!(!Format::Pnm.can_encode());
    }

    #[test]
    fn binary_boundaries_and_row_padding() {
        let image = decode(b"P5\n3 1\n255\n\n #").expect("binary whitespace");
        assert_eq!(
            image.frames()[0].pixels(),
            [10, 10, 10, 255, 32, 32, 32, 255, 35, 35, 35, 255]
        );
        let image = decode(b"P4\n1 2\n\x7f\x80").expect("row padding");
        assert_eq!(
            image.frames()[0].pixels(),
            [255, 255, 255, 255, 0, 0, 0, 255]
        );
    }

    #[test]
    fn pam_alpha() {
        for (depth, tuple, samples, expected) in [
            (2, "GRAYSCALE_ALPHA", &b"\x80\0"[..], [128, 128, 128, 0]),
            (4, "RGB_ALPHA", &b"\x12\x34\x56\x78"[..], [18, 52, 86, 120]),
        ] {
            let mut data = alloc::format!("P7\n# comment\n\nWIDTH 1\nHEIGHT 1\nDEPTH {depth}\nMAXVAL 255\nTUPLTYPE {tuple}\nENDHDR\n").into_bytes();
            data.extend_from_slice(samples);
            assert_eq!(
                decode(&data).expect("PAM alpha").frames()[0].pixels(),
                expected
            );
        }
    }

    #[test]
    fn invalid_data() {
        for data in [
            &b"P2\n0 1\n255\n0"[..],
            &b"P2\n1 1\n0\n0"[..],
            &b"P2\n1 1\n65536\n0"[..],
            &b"P2\n1 1\n15\n16"[..],
            &b"P5\n1 1\n255\n"[..],
            &b"P6\n1 1\n65535\n\0\0"[..],
            &b"P4\n9 1\n\0"[..],
            &b"P7\nWIDTH 1\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\n"[..],
            &b"P7\nWIDTH 1\nWIDTH 1\nHEIGHT 1\nDEPTH 1\nMAXVAL 255\nENDHDR\n\0"[..],
        ] {
            assert!(decode(data).is_err(), "accepted {data:?}");
        }
        assert_eq!(
            decode(b"P2\n4294967295 4294967295\n255\n0").err(),
            Some(DecodeError::ImageTooLarge)
        );
    }
}
