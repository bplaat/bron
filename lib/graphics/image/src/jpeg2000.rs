/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use alloc::vec::Vec;

use super::color::{ColorModel, Transform};
use super::{Budget, DecodeError, Format, Image, Result, pixel_len};

struct Boxes<'a> {
    data: &'a [u8],
}
impl<'a> Boxes<'a> {
    fn next(&mut self) -> Result<Option<(&'a [u8], &'a [u8])>> {
        if self.data.is_empty() {
            return Ok(None);
        }
        let header = self.data.get(..8).ok_or(DecodeError::InvalidData)?;
        let length = u32::from_be_bytes(header[..4].try_into().expect("four bytes"));
        let (length, start) = if length == 0 {
            (self.data.len(), 8)
        } else if length == 1 {
            let extended = self.data.get(8..16).ok_or(DecodeError::InvalidData)?;
            (
                usize::try_from(u64::from_be_bytes(
                    extended.try_into().expect("eight bytes"),
                ))
                .map_err(|_| DecodeError::ImageTooLarge)?,
                16,
            )
        } else {
            (length as usize, 8)
        };
        if length < start {
            return Err(DecodeError::InvalidData);
        }
        let payload = self
            .data
            .get(start..length)
            .ok_or(DecodeError::InvalidData)?;
        self.data = &self.data[length..];
        Ok(Some((&header[4..8], payload)))
    }
}
fn word(data: &[u8]) -> Result<usize> {
    Ok(usize::from(u16::from_be_bytes(
        data.get(..2)
            .ok_or(DecodeError::InvalidData)?
            .try_into()
            .expect("two bytes"),
    )))
}

pub(super) fn decode(data: &[u8]) -> Result<Image> {
    let mut budget = Budget::default();
    budget.claim(data.len())?;
    let mut stream = data;
    let mut space = 0u32;
    let mut profile = None;
    let mut alpha = None;
    let mut premultiplied = false;
    let mut colors = Vec::new();
    let mut dimensions = None;
    if data.starts_with(b"\0\0\0\x0cjP  \r\n\x87\n") {
        let mut boxes = Boxes { data };
        let mut found = false;
        while let Some((kind, payload)) = boxes.next()? {
            match kind {
                b"jp2c" => {
                    if found {
                        return Err(DecodeError::UnsupportedFeature);
                    }
                    stream = payload;
                    found = true;
                }
                b"jp2h" => {
                    let mut header = Boxes { data: payload };
                    while let Some((kind, payload)) = header.next()? {
                        match kind {
                            b"ihdr" => {
                                if payload.len() != 14 || payload[11] != 7 {
                                    return Err(DecodeError::InvalidHeader);
                                }
                                let height = u32::from_be_bytes(
                                    payload[..4].try_into().expect("four bytes"),
                                );
                                let width = u32::from_be_bytes(
                                    payload[4..8].try_into().expect("four bytes"),
                                );
                                dimensions =
                                    Some((width as usize, height as usize, word(&payload[8..])?));
                            }
                            b"colr" => {
                                let method = *payload.first().ok_or(DecodeError::InvalidData)?;
                                match method {
                                    1 => {
                                        space = u32::from_be_bytes(
                                            payload
                                                .get(3..7)
                                                .ok_or(DecodeError::InvalidData)?
                                                .try_into()
                                                .expect("four bytes"),
                                        )
                                    }
                                    2 | 3 => {
                                        profile =
                                            Some(payload.get(3..).ok_or(DecodeError::InvalidData)?)
                                    }
                                    _ => return Err(DecodeError::UnsupportedFeature),
                                }
                            }
                            b"cdef" => {
                                let count = word(payload)?;
                                for i in 0..count {
                                    let entry = payload
                                        .get(2 + i * 6..8 + i * 6)
                                        .ok_or(DecodeError::InvalidData)?;
                                    let index = word(entry)?;
                                    let kind = word(&entry[2..])?;
                                    let association = word(&entry[4..])?;
                                    if kind == 0 {
                                        colors.push((association, index));
                                    } else if kind == 1 || kind == 2 {
                                        if alpha.is_some() {
                                            return Err(DecodeError::UnsupportedFeature);
                                        }
                                        alpha = Some(index);
                                        premultiplied = kind == 2;
                                    }
                                }
                            }
                            b"pclr" | b"cmap" => return Err(DecodeError::UnsupportedFeature),
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
        if !found {
            return Err(DecodeError::InvalidData);
        }
    }
    let decoded = decode_codestream(stream, &mut budget)?;
    let channels = decoded.planes.len();
    if dimensions.is_some_and(|size| size != (decoded.width, decoded.height, channels)) {
        return Err(DecodeError::InvalidHeader);
    }
    if space == 0 && decoded.subsampled {
        space = 18;
    }
    let model = if let Some(profile) = profile {
        match profile.get(16..20) {
            Some(b"GRAY") => ColorModel::Gray,
            Some(b"CMYK") => ColorModel::Cmyk,
            _ => ColorModel::Rgb,
        }
    } else {
        match space {
            17 => ColorModel::Gray,
            12 => ColorModel::Cmyk,
            0 => match channels {
                1 | 2 => ColorModel::Gray,
                3 => ColorModel::Rgb,
                4 => ColorModel::Cmyk,
                _ => return Err(DecodeError::UnsupportedFeature),
            },
            16 | 18 => ColorModel::Rgb,
            _ => return Err(DecodeError::UnsupportedFeature),
        }
    };
    let count = match model {
        ColorModel::Gray => 1,
        ColorModel::Rgb => 3,
        ColorModel::Cmyk => 4,
    };
    if alpha.is_none() && channels == count + 1 {
        alpha = Some(count);
    }
    if channels != count + usize::from(alpha.is_some()) || alpha.is_some_and(|i| i >= channels) {
        return Err(DecodeError::InvalidData);
    }
    if colors.is_empty() {
        for i in 0..channels {
            if Some(i) != alpha {
                colors.push((colors.len() + 1, i));
            }
        }
    }
    colors.sort_unstable();
    if colors.len() != count || colors.iter().any(|(_, i)| *i >= channels) {
        return Err(DecodeError::InvalidData);
    }
    let transform = profile.and_then(|p| Transform::icc(p, model));
    let mut pixels =
        budget.zeroed::<u8>(pixel_len(decoded.width as u32, decoded.height as u32)?)?;
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let sample = |component: usize| {
            let maximum = ((1u64 << decoded.precision[component]) - 1) as f64;
            let (dx, dy) = decoded.sampling[component];
            let (cx0, cy0) = (decoded.origin.0.div_ceil(dx), decoded.origin.1.div_ceil(dy));
            let cx = (decoded.origin.0 + index % decoded.width) / dx;
            let cy = (decoded.origin.1 + index / decoded.width) / dy;
            let width = (decoded.origin.0 + decoded.width).div_ceil(dx) - cx0;
            let height = (decoded.origin.1 + decoded.height).div_ceil(dy) - cy0;
            let value = if width == 0 || height == 0 || space == 18 && (cx < cx0 || cy < cy0) {
                0.0
            } else {
                decoded.planes[component][(cy.max(cy0) - cy0) * width + cx.max(cx0) - cx0]
            };
            (value * 255.0 / maximum + 0.5).clamp(0.0, 255.0) as u8
        };
        let mut values = [0u8; 4];
        for (i, &(_, component)) in colors.iter().enumerate() {
            values[i] = sample(component);
        }
        let opacity = alpha.map_or(255, sample);
        if premultiplied {
            for value in &mut values[..count] {
                *value = if opacity == 0 {
                    0
                } else {
                    ((u32::from(*value) * 255 + u32::from(opacity) / 2) / u32::from(opacity))
                        .min(255) as u8
                };
            }
        }
        let rgb = match model {
            ColorModel::Gray => [values[0]; 3],
            ColorModel::Rgb if space == 18 => {
                let y = f64::from(values[0]);
                let cb = f64::from(values[1]) - 128.0;
                let cr = f64::from(values[2]) - 128.0;
                [
                    y + 1.402 * cr,
                    y - 0.344_136 * cb - 0.714_136 * cr,
                    y + 1.772 * cb,
                ]
                .map(|v| (v + 0.5).clamp(0.0, 255.0) as u8)
            }
            ColorModel::Rgb => [values[0], values[1], values[2]],
            ColorModel::Cmyk => match &transform {
                Some(t) => t.cmyk(values),
                None => core::array::from_fn(|i| {
                    ((u16::from(255 - values[i]) * u16::from(255 - values[3]) + 127) / 255) as u8
                }),
            },
        };
        *pixel = [rgb[0], rgb[1], rgb[2], opacity];
    }
    if model != ColorModel::Cmyk
        && let Some(t) = transform
    {
        t.apply(&mut pixels);
    }
    Ok(Image::still(
        Format::Jpeg2000,
        decoded.width as u32,
        decoded.height as u32,
        pixels,
    ))
}

// MARK: Codestream and packets
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}
impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(DecodeError::ImageTooLarge)?;
        let out = self
            .bytes
            .get(self.position..end)
            .ok_or(DecodeError::InvalidData)?;
        self.position = end;
        Ok(out)
    }
    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn word(&mut self) -> Result<usize> {
        Ok(usize::from(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        )))
    }
    fn long(&mut self) -> Result<usize> {
        usize::try_from(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
        .map_err(|_| DecodeError::ImageTooLarge)
    }
    fn segment(&mut self) -> Result<(usize, &'a [u8])> {
        let marker = self.word()?;
        let length = self
            .word()?
            .checked_sub(2)
            .ok_or(DecodeError::InvalidData)?;
        Ok((marker, self.take(length)?))
    }
}

#[derive(Clone, PartialEq)]
struct Coding {
    levels: usize,
    width: usize,
    height: usize,
    style: u8,
    reversible: bool,
    precincts: Vec<(usize, usize)>,
}
impl Default for Coding {
    fn default() -> Self {
        Self {
            levels: 0,
            width: 64,
            height: 64,
            style: 0,
            reversible: true,
            precincts: alloc::vec![(15, 15)],
        }
    }
}
#[derive(Clone, Default, PartialEq)]
struct Quantization {
    guard: usize,
    style: u8,
    steps: Vec<(usize, usize)>,
}
#[derive(Clone, PartialEq)]
struct Component {
    precision: usize,
    dx: usize,
    dy: usize,
    coding: Option<Coding>,
    quant: Option<Quantization>,
    roi: usize,
}
#[derive(Clone, PartialEq)]
struct Header {
    components: Vec<Component>,
    coding: Coding,
    quant: Quantization,
    layers: usize,
    progression: u8,
    mct: bool,
    flags: u8,
}

struct Decoded {
    width: usize,
    height: usize,
    planes: Vec<Vec<f64>>,
    precision: Vec<usize>,
    subsampled: bool,
    origin: (usize, usize),
    sampling: Vec<(usize, usize)>,
}

fn coding(data: &[u8], component: bool) -> Result<(Coding, usize, u8, bool, u8)> {
    let mut r = Reader::new(data);
    let flags = r.byte()?;
    let (progression, layers, mct) = if component {
        (0, 1, false)
    } else {
        (r.byte()?, r.word()?, r.byte()? != 0)
    };
    let levels = usize::from(r.byte()?);
    let w = usize::from(r.byte()?) + 2;
    let h = usize::from(r.byte()?) + 2;
    let style = r.byte()?;
    let transform = r.byte()?;
    if levels > 32
        || w > 10
        || h > 10
        || w + h > 12
        || transform > 1
        || style & 0xc0 != 0
        || layers == 0
    {
        return Err(DecodeError::InvalidHeader);
    }
    let mut precincts = Vec::new();
    for resolution in 0..=levels {
        let value = if flags & 1 != 0 { r.byte()? } else { 255 };
        let size = (usize::from(value & 15), usize::from(value >> 4));
        if resolution != 0 && (size.0 == 0 || size.1 == 0) {
            return Err(DecodeError::InvalidHeader);
        }
        precincts.push(size);
    }
    Ok((
        Coding {
            levels,
            width: 1 << w,
            height: 1 << h,
            style,
            reversible: transform == 1,
            precincts,
        },
        layers,
        progression,
        mct,
        flags,
    ))
}
fn quantization(data: &[u8]) -> Result<Quantization> {
    let mut r = Reader::new(data);
    let flags = r.byte()?;
    let style = flags & 31;
    if style > 2 {
        return Err(DecodeError::UnsupportedFeature);
    }
    let mut steps = Vec::new();
    while r.position < data.len() {
        let step = if style == 0 {
            (usize::from(r.byte()?) >> 3, 0)
        } else {
            let v = r.word()?;
            (v >> 11, v & 2047)
        };
        steps.push(step);
    }
    if steps.is_empty() || style == 1 && steps.len() != 1 {
        return Err(DecodeError::InvalidData);
    }
    Ok(Quantization {
        guard: usize::from(flags >> 5),
        style,
        steps,
    })
}
fn component_index(r: &mut Reader<'_>, count: usize) -> Result<usize> {
    let index = if count < 257 {
        usize::from(r.byte()?)
    } else {
        r.word()?
    };
    if index >= count {
        return Err(DecodeError::InvalidData);
    }
    Ok(index)
}
fn update(header: &mut Header, marker: usize, data: &[u8]) -> Result<()> {
    match marker {
        0xff52 => {
            let (c, l, p, m, f) = coding(data, false)?;
            header.coding = c;
            header.layers = l;
            header.progression = p;
            header.mct = m;
            header.flags = f;
        }
        0xff53 => {
            let mut r = Reader::new(data);
            let index = component_index(&mut r, header.components.len())?;
            header.components[index].coding = Some(coding(&data[r.position..], true)?.0);
        }
        0xff5c => header.quant = quantization(data)?,
        0xff5d => {
            let mut r = Reader::new(data);
            let index = component_index(&mut r, header.components.len())?;
            header.components[index].quant = Some(quantization(&data[r.position..])?);
        }
        0xff5e => {
            let mut r = Reader::new(data);
            let index = component_index(&mut r, header.components.len())?;
            if r.byte()? != 0 {
                return Err(DecodeError::UnsupportedFeature);
            }
            header.components[index].roi = usize::from(r.byte()?);
        }
        0xff5f..=0xff61 => return Err(DecodeError::UnsupportedFeature),
        _ => {}
    }
    Ok(())
}

fn decode_codestream(data: &[u8], budget: &mut Budget) -> Result<Decoded> {
    let mut r = Reader::new(data);
    if r.word()? != 0xff4f {
        return Err(DecodeError::InvalidHeader);
    }
    let (marker, siz) = r.segment()?;
    if marker != 0xff51 {
        return Err(DecodeError::InvalidHeader);
    }
    let mut s = Reader::new(siz);
    s.word()?;
    let width = s.long()?;
    let height = s.long()?;
    let x0 = s.long()?;
    let y0 = s.long()?;
    let tw = s.long()?;
    let th = s.long()?;
    let tx0 = s.long()?;
    let ty0 = s.long()?;
    let count = s.word()?;
    if width <= x0
        || height <= y0
        || tw == 0
        || th == 0
        || tx0 > x0
        || ty0 > y0
        || x0.saturating_sub(tx0) >= tw
        || y0.saturating_sub(ty0) >= th
        || count == 0
        || count > 16
    {
        return Err(DecodeError::InvalidHeader);
    }
    pixel_len((width - x0) as u32, (height - y0) as u32)?;
    let mut components = Vec::new();
    for _ in 0..count {
        let p = s.byte()?;
        let dx = usize::from(s.byte()?);
        let dy = usize::from(s.byte()?);
        if dx == 0 || dy == 0 || p & 127 > 37 {
            return Err(DecodeError::InvalidHeader);
        }
        components.push(Component {
            precision: usize::from(p & 127) + 1,
            dx,
            dy,
            coding: None,
            quant: None,
            roi: 0,
        });
    }
    let mut header = Header {
        components,
        coding: Coding::default(),
        quant: Quantization::default(),
        layers: 1,
        progression: 0,
        mct: false,
        flags: 0,
    };
    while r.bytes.get(r.position..r.position + 2) != Some(&[0xff, 0x90]) {
        let (marker, segment) = r.segment()?;
        update(&mut header, marker, segment)?;
    }
    let mut planes = Vec::new();
    for component in &header.components {
        let cw = width.div_ceil(component.dx) - x0.div_ceil(component.dx);
        let ch = height.div_ceil(component.dy) - y0.div_ceil(component.dy);
        planes.push(budget.zeroed::<f64>(cw * ch)?);
    }
    let columns = (width - tx0).div_ceil(tw);
    let rows = (height - ty0).div_ceil(th);
    #[derive(Clone, Default)]
    struct Tile<'a> {
        header: Option<Header>,
        bodies: Vec<&'a [u8]>,
        next: usize,
        count: Option<usize>,
    }
    let mut tiles = budget.zeroed::<Tile<'_>>(
        columns
            .checked_mul(rows)
            .ok_or(DecodeError::ImageTooLarge)?,
    )?;
    let mut finished = false;
    while r.position < data.len() {
        let start = r.position;
        let marker = r.word()?;
        if marker == 0xffd9 {
            finished = true;
            break;
        }
        if marker != 0xff90 {
            return Err(DecodeError::InvalidData);
        }
        if r.word()? != 10 {
            return Err(DecodeError::InvalidData);
        }
        let tile = r.word()?;
        let length = r.long()?;
        let part = r.byte()?;
        let parts = r.byte()?;
        let state = tiles.get_mut(tile).ok_or(DecodeError::InvalidData)?;
        if usize::from(part) != state.next
            || state.count.is_some_and(|count| {
                state.next >= count || parts != 0 && usize::from(parts) != count
            })
            || parts != 0 && part >= parts
        {
            return Err(DecodeError::InvalidData);
        }
        if parts != 0 {
            state.count = Some(usize::from(parts));
        }
        state.next += 1;
        let end = if length == 0 {
            data.len().checked_sub(2).ok_or(DecodeError::InvalidData)?
        } else {
            start
                .checked_add(length)
                .ok_or(DecodeError::ImageTooLarge)?
        };
        let mut tile_header = state.header.clone().unwrap_or_else(|| header.clone());
        while r.bytes.get(r.position..r.position + 2) != Some(&[0xff, 0x93]) {
            let (marker, segment) = r.segment()?;
            update(&mut tile_header, marker, segment)?;
        }
        r.word()?;
        if end < r.position {
            return Err(DecodeError::InvalidData);
        }
        let body = r.take(end - r.position)?;
        if state
            .header
            .as_ref()
            .is_some_and(|previous| previous != &tile_header)
        {
            return Err(DecodeError::UnsupportedFeature);
        }
        state.header = Some(tile_header);
        budget.claim(size_of::<&[u8]>())?;
        state.bodies.push(body);
    }
    if !finished
        || tiles
            .iter()
            .any(|tile| tile.header.is_none() || tile.count.is_some_and(|count| tile.next != count))
    {
        return Err(DecodeError::InvalidData);
    }
    for (tile, state) in tiles.into_iter().enumerate() {
        let tile_header = state.header.as_ref().ok_or(DecodeError::InvalidData)?;
        let allocated = budget.used;
        let mut joined = Vec::new();
        let body = if state.bodies.len() == 1 {
            state.bodies[0]
        } else {
            let length = state
                .bodies
                .iter()
                .try_fold(0usize, |sum, body| sum.checked_add(body.len()))
                .ok_or(DecodeError::ImageTooLarge)?;
            joined = budget.zeroed::<u8>(length)?;
            let mut at = 0;
            for body in &state.bodies {
                joined[at..at + body.len()].copy_from_slice(body);
                at += body.len();
            }
            &joined
        };
        let (left, top) = (
            (tx0 + (tile % columns) * tw).max(x0),
            (ty0 + (tile / columns) * th).max(y0),
        );
        let right = (tx0 + (tile % columns + 1) * tw).min(width);
        let bottom = (ty0 + (tile / columns + 1) * th).min(height);
        let decoded = tile_data(tile_header, body, (left, top, right, bottom), budget)?;
        for c in 0..count {
            let component = &header.components[c];
            let cx0 = left.div_ceil(component.dx);
            let cy0 = top.div_ceil(component.dy);
            let cw = right.div_ceil(component.dx) - cx0;
            let ch = bottom.div_ceil(component.dy) - cy0;
            let origin_x = x0.div_ceil(component.dx);
            let origin_y = y0.div_ceil(component.dy);
            let stride = width.div_ceil(component.dx) - origin_x;
            if decoded[c].len() != cw * ch {
                return Err(DecodeError::InvalidData);
            }
            for y in 0..ch {
                let start = (cy0 + y - origin_y) * stride + cx0 - origin_x;
                planes[c][start..start + cw].copy_from_slice(&decoded[c][y * cw..(y + 1) * cw]);
            }
        }
        drop(decoded);
        drop(joined);
        budget.used = allocated;
    }
    Ok(Decoded {
        width: width - x0,
        height: height - y0,
        planes,
        origin: (x0, y0),
        sampling: header.components.iter().map(|c| (c.dx, c.dy)).collect(),
        precision: header.components.iter().map(|c| c.precision).collect(),
        subsampled: !header.mct
            && header.components.len() == 3
            && header.components[1..]
                .iter()
                .any(|c| c.dx > header.components[0].dx || c.dy > header.components[0].dy),
    })
}

struct Packet<'a> {
    bytes: &'a [u8],
    position: usize,
    left: u8,
    current: u8,
    previous: u8,
}
impl<'a> Packet<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            left: 0,
            current: 0,
            previous: 0,
        }
    }
    fn bit(&mut self) -> Result<bool> {
        if self.left == 0 {
            self.current = *self
                .bytes
                .get(self.position)
                .ok_or(DecodeError::InvalidData)?;
            self.position += 1;
            self.left = if self.previous == 255 { 7 } else { 8 };
            if self.left == 7 && self.current & 128 != 0 {
                return Err(DecodeError::InvalidData);
            }
            self.previous = self.current;
        }
        self.left -= 1;
        Ok((self.current >> self.left) & 1 != 0)
    }
    fn bits(&mut self, count: usize) -> Result<usize> {
        if count > 31 {
            return Err(DecodeError::ImageTooLarge);
        }
        let mut value = 0;
        for _ in 0..count {
            value = value * 2 + usize::from(self.bit()?);
        }
        Ok(value)
    }
    const fn align(&mut self) -> Result<()> {
        if self.left == 0 && self.previous == 255 {
            self.position += 1;
            if self.position > self.bytes.len() {
                return Err(DecodeError::InvalidData);
            }
        }
        self.left = 0;
        self.previous = 0;
        Ok(())
    }
}
#[derive(Clone, Default)]
struct Node {
    low: usize,
    known: bool,
}
struct TagTree {
    widths: Vec<usize>,
    levels: Vec<Vec<Node>>,
}
impl TagTree {
    fn new(mut width: usize, mut height: usize, budget: &mut Budget) -> Result<Self> {
        let mut levels = Vec::new();
        let mut widths = Vec::new();
        loop {
            widths.push(width);
            levels.push(budget.zeroed(width * height)?);
            if width == 1 && height == 1 {
                break;
            }
            width = width.div_ceil(2);
            height = height.div_ceil(2);
        }
        Ok(Self { widths, levels })
    }
    fn included(
        &mut self,
        x: usize,
        y: usize,
        threshold: usize,
        bits: &mut Packet<'_>,
    ) -> Result<bool> {
        let mut low = 0;
        let mut result = false;
        for level in (0..self.levels.len()).rev() {
            let index = (y >> level) * self.widths[level] + (x >> level);
            let node = &mut self.levels[level][index];
            node.low = node.low.max(low);
            while node.low < threshold && !node.known {
                if bits.bit()? {
                    node.known = true;
                } else {
                    node.low += 1;
                }
            }
            low = node.low;
            result = node.known && node.low < threshold;
        }
        Ok(result)
    }
}

#[derive(Clone, Default)]
struct Segment {
    passes: usize,
    data: Vec<u8>,
}

// The first lazy segment has ten passes; subsequent raw and MQ segments alternate.
const fn segment_passes(style: u8, start: usize) -> usize {
    if style & 4 != 0 {
        1
    } else if style & 1 != 0 {
        if start == 0 {
            10
        } else if (start - 10).is_multiple_of(3) {
            2
        } else {
            1
        }
    } else {
        109
    }
}

#[derive(Clone, Default)]
struct Block {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    passes: usize,
    zero: usize,
    lblock: usize,
    segments: Vec<Segment>,
}
struct Precinct {
    columns: usize,
    blocks: Vec<Block>,
    inclusion: Option<TagTree>,
    zero: Option<TagTree>,
}
struct Band {
    orientation: usize,
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
    precincts: Vec<Precinct>,
}
fn precinct(
    bounds: (usize, usize, usize, usize),
    origin: (usize, usize),
    block_size: (usize, usize),
    budget: &mut Budget,
) -> Result<Precinct> {
    let (x0, y0, x1, y1) = bounds;
    let (bw, bh) = block_size;
    let columns = if x1 <= x0 {
        0
    } else {
        (x1 - 1) / bw - x0 / bw + 1
    };
    let rows = if y1 <= y0 {
        0
    } else {
        (y1 - 1) / bh - y0 / bh + 1
    };
    let mut blocks = budget.zeroed::<Block>(columns * rows)?;
    for (i, block) in blocks.iter_mut().enumerate() {
        let left = ((x0 / bw + i % columns) * bw).max(x0);
        let top = ((y0 / bh + i / columns) * bh).max(y0);
        let right = ((x0 / bw + i % columns + 1) * bw).min(x1);
        let bottom = ((y0 / bh + i / columns + 1) * bh).min(y1);
        *block = Block {
            x: left - origin.0,
            y: top - origin.1,
            width: right - left,
            height: bottom - top,
            lblock: 3,
            ..Block::default()
        };
    }
    let (inclusion, zero) = if blocks.is_empty() {
        (None, None)
    } else {
        (
            Some(TagTree::new(columns, rows, budget)?),
            Some(TagTree::new(columns, rows, budget)?),
        )
    };
    Ok(Precinct {
        columns,
        blocks,
        inclusion,
        zero,
    })
}
const fn ceil_offset(value: usize, offset: usize, divisor: usize) -> usize {
    if value >= offset {
        (value - offset).div_ceil(divisor)
    } else {
        0
    }
}
struct Resolution {
    bands: Vec<Band>,
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
    positions: Vec<(usize, usize)>,
}
fn resolutions(
    component: &Component,
    bounds: (usize, usize, usize, usize),
    default: &Coding,
    budget: &mut Budget,
) -> Result<Vec<Resolution>> {
    let coding = component.coding.as_ref().unwrap_or(default);
    let (x0, y0, x1, y1) = (
        bounds.0.div_ceil(component.dx),
        bounds.1.div_ceil(component.dy),
        bounds.2.div_ceil(component.dx),
        bounds.3.div_ceil(component.dy),
    );
    let mut out = Vec::new();
    for resolution in 0..=coding.levels {
        let scale = 1usize
            .checked_shl((coding.levels - resolution) as u32)
            .ok_or(DecodeError::ImageTooLarge)?;
        let rx0 = x0.div_ceil(scale);
        let ry0 = y0.div_ceil(scale);
        let rx1 = x1.div_ceil(scale);
        let ry1 = y1.div_ceil(scale);
        let (px, py) = coding
            .precincts
            .get(resolution)
            .copied()
            .ok_or(DecodeError::InvalidHeader)?;
        let (pw, ph) = (1usize << px, 1usize << py);
        let (pc0, pr0) = (rx0 / pw, ry0 / ph);
        let columns = if rx1 == rx0 {
            0
        } else {
            rx1.div_ceil(pw) - pc0
        };
        let rows = if ry1 == ry0 {
            0
        } else {
            ry1.div_ceil(ph) - pr0
        };
        let mut positions = budget.zeroed::<(usize, usize)>(columns * rows)?;
        for (i, position) in positions.iter_mut().enumerate() {
            *position = (
                ((pc0 + i % columns) * pw * scale * component.dx).max(bounds.0),
                ((pr0 + i / columns) * ph * scale * component.dy).max(bounds.1),
            );
        }
        let mut bands = Vec::new();
        for orientation in if resolution == 0 { 0..1 } else { 1..4 } {
            let (bx0, by0, bx1, by1, bpw, bph) = if resolution == 0 {
                (rx0, ry0, rx1, ry1, pw, ph)
            } else {
                let ox = if orientation & 1 != 0 { scale } else { 0 };
                let oy = if orientation & 2 != 0 { scale } else { 0 };
                (
                    ceil_offset(x0, ox, scale * 2),
                    ceil_offset(y0, oy, scale * 2),
                    ceil_offset(x1, ox, scale * 2),
                    ceil_offset(y1, oy, scale * 2),
                    pw / 2,
                    ph / 2,
                )
            };
            let mut precincts = Vec::new();
            budget.claim(columns * rows * size_of::<Precinct>())?;
            for i in 0..columns * rows {
                let (left, top) = ((pc0 + i % columns) * bpw, (pr0 + i / columns) * bph);
                precincts.push(precinct(
                    (
                        left.max(bx0),
                        top.max(by0),
                        (left + bpw).min(bx1),
                        (top + bph).min(by1),
                    ),
                    (bx0, by0),
                    (coding.width.min(bpw), coding.height.min(bph)),
                    budget,
                )?);
            }
            bands.push(Band {
                orientation,
                x0: bx0,
                y0: by0,
                width: bx1 - bx0,
                height: by1 - by0,
                precincts,
            });
        }
        out.push(Resolution {
            bands,
            x0: rx0,
            y0: ry0,
            width: rx1 - rx0,
            height: ry1 - ry0,
            positions,
        });
    }
    Ok(out)
}
fn read_packet(
    resolution: &mut Resolution,
    precinct_index: usize,
    style: u8,
    layer: usize,
    body: &[u8],
    flags: u8,
    budget: &mut Budget,
) -> Result<usize> {
    let start = if flags & 2 != 0 {
        if body.get(..2) != Some(&[0xff, 0x91]) {
            return Err(DecodeError::InvalidData);
        }
        6
    } else {
        0
    };
    let mut bits = Packet::new(body.get(start..).ok_or(DecodeError::InvalidData)?);
    let present = bits.bit()?;
    let mut lengths = Vec::new();
    if present {
        for (band_index, band) in resolution.bands.iter_mut().enumerate() {
            let precinct = &mut band.precincts[precinct_index];
            for (index, block) in precinct.blocks.iter_mut().enumerate() {
                let included = if block.passes == 0 {
                    precinct
                        .inclusion
                        .as_mut()
                        .expect("nonempty band")
                        .included(
                            index % precinct.columns,
                            index / precinct.columns,
                            layer + 1,
                            &mut bits,
                        )?
                } else {
                    bits.bit()?
                };
                if !included {
                    continue;
                }
                if block.passes == 0 {
                    let mut zero = 0;
                    while !precinct.zero.as_mut().expect("nonempty band").included(
                        index % precinct.columns,
                        index / precinct.columns,
                        zero + 1,
                        &mut bits,
                    )? {
                        zero += 1;
                        if zero > 38 {
                            return Err(DecodeError::InvalidData);
                        }
                    }
                    block.zero = zero;
                }
                let passes = if !bits.bit()? {
                    1
                } else if !bits.bit()? {
                    2
                } else {
                    let n = bits.bits(2)?;
                    if n < 3 {
                        3 + n
                    } else {
                        let n = bits.bits(5)?;
                        if n < 31 { 6 + n } else { 37 + bits.bits(7)? }
                    }
                };
                while bits.bit()? {
                    block.lblock += 1;
                    if block.lblock > 31 {
                        return Err(DecodeError::ImageTooLarge);
                    }
                }
                let mut pending = passes;
                while pending > 0 {
                    let start = block.passes - block.segments.last().map_or(0, |s| s.passes);
                    if block
                        .segments
                        .last()
                        .is_none_or(|s| s.passes == segment_passes(style, start))
                    {
                        budget.claim(size_of::<Segment>())?;
                        block.segments.push(Segment::default());
                    }
                    let segment = block.segments.last_mut().expect("segment added");
                    let start = block.passes - segment.passes;
                    let n = pending.min(segment_passes(style, start) - segment.passes);
                    let length = bits.bits(block.lblock + n.ilog2() as usize)?;
                    segment.passes += n;
                    block.passes += n;
                    pending -= n;
                    if block.passes > 114 {
                        return Err(DecodeError::InvalidData);
                    }
                    lengths.push((band_index, index, block.segments.len() - 1, length));
                }
            }
        }
    }
    bits.align()?;
    let mut offset = start + bits.position;
    if flags & 4 != 0 {
        if body.get(offset..offset + 2) != Some(&[0xff, 0x92]) {
            return Err(DecodeError::InvalidData);
        }
        offset += 2;
    }
    for (band_index, index, segment, length) in lengths {
        let block = &mut resolution.bands[band_index].precincts[precinct_index].blocks[index];
        let end = offset
            .checked_add(length)
            .ok_or(DecodeError::ImageTooLarge)?;
        let bytes = body.get(offset..end).ok_or(DecodeError::InvalidData)?;
        budget.claim(length)?;
        let data = &mut block.segments[segment].data;
        data.try_reserve_exact(length)
            .map_err(|_| DecodeError::ImageTooLarge)?;
        data.extend_from_slice(bytes);
        offset = end;
    }
    Ok(offset)
}

fn tile_data(
    header: &Header,
    body: &[u8],
    bounds: (usize, usize, usize, usize),
    budget: &mut Budget,
) -> Result<Vec<Vec<f64>>> {
    let allocated = budget.used;
    let mut components = Vec::new();
    for component in &header.components {
        components.push(resolutions(component, bounds, &header.coding, budget)?);
    }
    let capacity = components
        .iter()
        .flat_map(|c| c.iter())
        .try_fold(0usize, |sum, r| {
            r.positions
                .len()
                .checked_mul(header.layers)
                .and_then(|n| sum.checked_add(n))
        })
        .ok_or(DecodeError::ImageTooLarge)?;
    let mut packets = budget.zeroed::<(usize, usize, usize, usize, usize, usize)>(capacity)?;
    let mut next = 0;
    for (c, component) in components.iter().enumerate() {
        for (r, resolution) in component.iter().enumerate() {
            for (p, &(x, y)) in resolution.positions.iter().enumerate() {
                for layer in 0..header.layers {
                    packets[next] = (c, r, p, layer, x.max(bounds.0), y.max(bounds.1));
                    next += 1;
                }
            }
        }
    }
    if header.progression > 4 {
        return Err(DecodeError::UnsupportedFeature);
    }
    packets.sort_unstable_by_key(|&(c, r, p, l, x, y)| match header.progression {
        0 => (l, r, c, p, 0),
        1 => (r, l, c, p, 0),
        2 => (r, y, x, c, l),
        3 => (y, x, c, r, l),
        _ => (c, y, x, r, l),
    });
    let mut offset = 0;
    for (c, resolution, precinct, layer, _, _) in packets {
        let style = header.components[c]
            .coding
            .as_ref()
            .unwrap_or(&header.coding)
            .style;
        offset += read_packet(
            &mut components[c][resolution],
            precinct,
            style,
            layer,
            body.get(offset..).ok_or(DecodeError::InvalidData)?,
            header.flags,
            budget,
        )?;
    }
    if offset != body.len() {
        return Err(DecodeError::InvalidData);
    }
    let mut decoded = Vec::new();
    for (c, resolutions) in components.iter().enumerate() {
        decoded.push(reconstruct(
            resolutions,
            &header.components[c],
            &header.coding,
            &header.quant,
            budget,
        )?);
    }
    if header.mct && decoded.len() >= 3 {
        if decoded[..3].iter().any(|p| p.len() != decoded[0].len()) {
            return Err(DecodeError::UnsupportedFeature);
        }
        let reversible = header.components[0]
            .coding
            .as_ref()
            .unwrap_or(&header.coding)
            .reversible;
        for i in 0..decoded[0].len() {
            let y = decoded[0][i];
            let u = decoded[1][i];
            let v = decoded[2][i];
            let (r, g, b) = if reversible {
                let g = y - floor((u + v) * 0.25);
                (v + g, g, u + g)
            } else {
                (
                    y + 1.402 * v,
                    y - 0.344_13 * u - 0.714_14 * v,
                    y + 1.772 * u,
                )
            };
            decoded[0][i] = r;
            decoded[1][i] = g;
            decoded[2][i] = b;
        }
    }
    for (component, values) in header.components.iter().zip(&mut decoded) {
        let shift = (1u64 << (component.precision - 1)) as f64;
        let max = ((1u64 << component.precision) - 1) as f64;
        for value in values {
            *value = (*value + shift).clamp(0.0, max);
        }
    }
    drop(components);
    budget.used = allocated
        + decoded
            .iter()
            .map(|plane| plane.len() * size_of::<f64>())
            .sum::<usize>();
    Ok(decoded)
}

fn reconstruct(
    resolutions: &[Resolution],
    component: &Component,
    default: &Coding,
    default_quant: &Quantization,
    budget: &mut Budget,
) -> Result<Vec<f64>> {
    let coding = component.coding.as_ref().unwrap_or(default);
    let quant = component.quant.as_ref().unwrap_or(default_quant);
    let mut output = Vec::new();
    let mut previous_width = 0;
    let mut previous_x0 = 0;
    let mut previous_y0 = 0;
    for (r, resolution) in resolutions.iter().enumerate() {
        let mut bands = Vec::new();
        for band in &resolution.bands {
            let index = if r == 0 {
                0
            } else {
                1 + (r - 1) * 3 + band.orientation - 1
            };
            let (mut exponent, mantissa) = *quant
                .steps
                .get(if quant.style == 1 { 0 } else { index })
                .ok_or(DecodeError::InvalidData)?;
            if quant.style == 1 && r > 0 {
                exponent = exponent
                    .checked_sub(r - 1)
                    .ok_or(DecodeError::InvalidData)?;
            }
            let planes = quant
                .guard
                .checked_add(exponent)
                .and_then(|n| n.checked_sub(1))
                .ok_or(DecodeError::InvalidData)?;
            let gain = if band.orientation == 0 {
                0
            } else if band.orientation == 3 {
                2
            } else {
                1
            };
            let power = component.precision as i32 + gain - exponent as i32;
            let step = if coding.reversible {
                1.0
            } else {
                let factor = if power >= 0 {
                    (1u64
                        .checked_shl(power as u32)
                        .ok_or(DecodeError::ImageTooLarge)?) as f64
                } else {
                    1.0 / (1u64
                        .checked_shl((-power) as u32)
                        .ok_or(DecodeError::ImageTooLarge)?) as f64
                };
                factor * (1.0 + mantissa as f64 / 2048.0)
            };
            let mut coefficients = budget.zeroed::<f64>(band.width * band.height)?;
            for block in band.precincts.iter().flat_map(|precinct| &precinct.blocks) {
                if block.passes == 0 {
                    continue;
                }
                let values = decode_codeblock(
                    &block.segments,
                    (block.width, block.height),
                    planes
                        .checked_sub(block.zero)
                        .ok_or(DecodeError::InvalidData)?,
                    block.passes,
                    band.orientation,
                    coding.style,
                    budget,
                )?;
                for y in 0..block.height {
                    for x in 0..block.width {
                        let mut value = values[y * block.width + x];
                        if coding.reversible {
                            value = (value as i64) as f64;
                        }
                        if component.roi > 0 {
                            if component.roi > 38 {
                                return Err(DecodeError::UnsupportedFeature);
                            }
                            let scale = (1u64 << component.roi) as f64;
                            if value.abs() >= scale {
                                value = (value / scale) as i64 as f64;
                            }
                        }
                        coefficients[(block.y + y) * band.width + block.x + x] = value * step;
                    }
                }
                budget.release(values);
            }
            bands.push(coefficients);
        }
        if r == 0 {
            output = bands.remove(0);
        } else {
            let mut expanded = budget.zeroed::<f64>(resolution.width * resolution.height)?;
            for y in 0..resolution.height {
                for x in 0..resolution.width {
                    let gx = resolution.x0 + x;
                    let gy = resolution.y0 + y;
                    let orientation = (gx & 1) | ((gy & 1) << 1);
                    expanded[y * resolution.width + x] = if orientation == 0 {
                        *output
                            .get((gy / 2 - previous_y0) * previous_width + gx / 2 - previous_x0)
                            .ok_or(DecodeError::InvalidData)?
                    } else {
                        let band = &resolution.bands[orientation - 1];
                        *bands[orientation - 1]
                            .get((gy / 2 - band.y0) * band.width + gx / 2 - band.x0)
                            .ok_or(DecodeError::InvalidData)?
                    };
                }
            }
            if resolution.width == 0 || resolution.height == 0 {
                budget.release(output);
                for band in bands {
                    budget.release(band);
                }
                output = expanded;
                previous_width = resolution.width;
                previous_x0 = resolution.x0;
                previous_y0 = resolution.y0;
                continue;
            }
            for row in expanded.chunks_exact_mut(resolution.width) {
                inverse_wavelet_line(row, resolution.x0, coding.reversible);
            }
            let mut column = budget.zeroed::<f64>(resolution.height)?;
            for x in 0..resolution.width {
                for y in 0..resolution.height {
                    column[y] = expanded[y * resolution.width + x];
                }
                inverse_wavelet_line(&mut column, resolution.y0, coding.reversible);
                for y in 0..resolution.height {
                    expanded[y * resolution.width + x] = column[y];
                }
            }
            budget.release(column);
            budget.release(output);
            for band in bands {
                budget.release(band);
            }
            output = expanded;
        }
        previous_width = resolution.width;
        previous_x0 = resolution.x0;
        previous_y0 = resolution.y0;
    }
    Ok(output)
}

// MARK: Arithmetic coding and code-blocks

struct Arithmetic<'a> {
    data: &'a [u8],
    position: usize,
    interval: u32,
    code: u32,
    count: u32,
    states: [usize; 19],
    mps: [bool; 19],
    raw: bool,
    raw_position: usize,
    raw_current: u8,
    raw_count: u8,
}

impl<'a> Arithmetic<'a> {
    fn new(data: &'a [u8]) -> Self {
        let mut decoder = Self {
            data,
            position: 0,
            interval: 0x8000,
            code: u32::from(data.first().copied().unwrap_or(255)) << 16,
            count: 0,
            states: [0; 19],
            mps: [false; 19],
            raw: false,
            raw_position: 0,
            raw_current: 0,
            raw_count: 0,
        };
        decoder.reset();
        decoder.byte();
        decoder.code <<= 7;
        decoder.count -= 7;
        decoder
    }
    fn reset(&mut self) {
        self.states.fill(0);
        self.mps.fill(false);
        self.states[0] = 4;
        self.states[17] = 3;
        self.states[18] = 46;
    }
    fn byte(&mut self) {
        let current = self.data.get(self.position).copied().unwrap_or(255);
        let next = self.data.get(self.position + 1).copied().unwrap_or(255);
        if current == 255 {
            if next > 0x8f {
                self.code = self.code.wrapping_add(0xff00);
                self.count = 8;
            } else {
                self.position += 1;
                self.code = self.code.wrapping_add(u32::from(next) << 9);
                self.count = 7;
            }
        } else {
            self.position += 1;
            self.code = self.code.wrapping_add(u32::from(next) << 8);
            self.count = 8;
        }
    }
    fn raw_bit(&mut self) -> bool {
        if self.raw_count == 0 {
            let next = self.data.get(self.raw_position).copied().unwrap_or(255);
            if self.raw_current == 255 && next > 0x8f {
                self.raw_current = 255;
                self.raw_count = 8;
            } else {
                self.raw_count = if self.raw_current == 255 { 7 } else { 8 };
                self.raw_current = next;
                self.raw_position += 1;
            }
        }
        self.raw_count -= 1;
        (self.raw_current >> self.raw_count) & 1 != 0
    }
    fn bit(&mut self, context: usize) -> bool {
        if self.raw {
            return self.raw_bit();
        }
        let (qe, nmps, nlps, swap) = STATES[self.states[context]];
        self.interval -= qe;
        let value;
        if self.code >> 16 < qe {
            if self.interval < qe {
                value = self.mps[context];
                self.states[context] = nmps;
            } else {
                value = !self.mps[context];
                if swap {
                    self.mps[context] = !self.mps[context];
                }
                self.states[context] = nlps;
            }
            self.interval = qe;
        } else {
            self.code -= qe << 16;
            if self.interval & 0x8000 != 0 {
                return self.mps[context];
            }
            if self.interval < qe {
                value = !self.mps[context];
                if swap {
                    self.mps[context] = !self.mps[context];
                }
                self.states[context] = nlps;
            } else {
                value = self.mps[context];
                self.states[context] = nmps;
            }
        }
        while self.interval < 0x8000 {
            if self.count == 0 {
                self.byte();
            }
            self.interval <<= 1;
            self.code <<= 1;
            self.count -= 1;
        }
        value
    }
}

struct Samples {
    width: usize,
    height: usize,
    flags: Vec<u8>,
    values: Vec<i64>,
    causal: bool,
}
impl Samples {
    fn neighbour(&self, x: isize, y: isize, origin: usize) -> u8 {
        if x < 0
            || y < 0
            || x >= self.width as isize
            || y >= self.height as isize
            || self.causal && y as usize / 4 > origin / 4
        {
            return 0;
        }
        self.flags[y as usize * self.width + x as usize]
    }
    fn context(&self, x: usize, y: usize, band: usize) -> usize {
        let x = x as isize;
        let yy = y as isize;
        let s = |dx, dy| usize::from(self.neighbour(x + dx, yy + dy, y) & 1);
        let mut h = s(-1, 0) + s(1, 0);
        let mut v = s(0, -1) + s(0, 1);
        let d = s(-1, -1) + s(1, -1) + s(-1, 1) + s(1, 1);
        if band == 3 {
            let sum = h + v;
            if d >= 3 {
                8
            } else if d == 2 {
                if sum > 0 { 7 } else { 6 }
            } else if d == 1 {
                if sum >= 2 {
                    5
                } else if sum == 1 {
                    4
                } else {
                    3
                }
            } else {
                sum.min(2)
            }
        } else {
            if band == 1 {
                core::mem::swap(&mut h, &mut v);
            }
            if h == 2 {
                8
            } else if h == 1 {
                if v > 0 {
                    7
                } else if d > 0 {
                    6
                } else {
                    5
                }
            } else if v == 2 {
                4
            } else if v == 1 {
                3
            } else {
                d.min(2)
            }
        }
    }
    fn sign(&mut self, decoder: &mut Arithmetic<'_>, x: usize, y: usize, plane: usize) {
        let contribution = |dx: isize, dy: isize| {
            let f = self.neighbour(x as isize + dx, y as isize + dy, y);
            if f & 1 == 0 {
                0
            } else if f & 2 == 0 {
                1i32
            } else {
                -1
            }
        };
        let h = (contribution(-1, 0) + contribution(1, 0)).clamp(-1, 1);
        let v = (contribution(0, -1) + contribution(0, 1)).clamp(-1, 1);
        let invert = h < 0 || h == 0 && v < 0;
        let (h, v) = if invert { (-h, -v) } else { (h, v) };
        let context = if h == 0 {
            if v == 0 { 9 } else { 10 }
        } else {
            (12 + v) as usize
        };
        let negative = decoder.bit(context) ^ (invert && !decoder.raw);
        let index = y * self.width + x;
        self.flags[index] |= 1 | if negative { 2 } else { 0 };
        self.values[index] = 3i64 << plane;
    }
}

fn decode_codeblock(
    segments: &[Segment],
    size: (usize, usize),
    planes: usize,
    passes: usize,
    band: usize,
    style: u8,
    budget: &mut Budget,
) -> Result<Vec<f64>> {
    let (width, height) = size;
    if planes > 38 || passes > planes * 3 {
        return Err(DecodeError::InvalidData);
    }
    let mut samples = Samples {
        width,
        height,
        flags: budget.zeroed(width * height)?,
        values: budget.zeroed(width * height)?,
        causal: style & 8 != 0,
    };
    let mut decoder = Arithmetic::new(&[]);
    let mut segment_index = 0;
    let mut segment_end = 0;
    for pass in 0..passes {
        if pass == segment_end {
            let segment = segments
                .get(segment_index)
                .ok_or(DecodeError::InvalidData)?;
            let (states, mps) = (decoder.states, decoder.mps);
            decoder = Arithmetic::new(&segment.data);
            decoder.states = states;
            decoder.mps = mps;
            decoder.raw = style & 1 != 0 && pass >= 10 && pass % 3 != 0;
            segment_end += segment.passes;
            segment_index += 1;
        }
        let plane = planes
            .checked_sub(1 + pass.div_ceil(3))
            .ok_or(DecodeError::InvalidData)?;
        let kind = pass % 3;
        for stripe in (0..height).step_by(4) {
            for x in 0..width {
                let mut first = 0;
                if kind == 0
                    && stripe + 4 <= height
                    && (0..4).all(|dy| {
                        samples.flags[(stripe + dy) * width + x] & 5 == 0
                            && samples.context(x, stripe + dy, band) == 0
                    })
                {
                    if !decoder.bit(17) {
                        continue;
                    }
                    first = usize::from(decoder.bit(18)) * 2 + usize::from(decoder.bit(18));
                    samples.sign(&mut decoder, x, stripe + first, plane);
                    first += 1;
                }
                for dy in first..4.min(height - stripe) {
                    let y = stripe + dy;
                    let index = y * width + x;
                    let flags = samples.flags[index];
                    let context = samples.context(x, y, band);
                    match kind {
                        1 if flags & 1 == 0 && context != 0 => {
                            samples.flags[index] |= 4;
                            if decoder.bit(context) {
                                samples.sign(&mut decoder, x, y, plane);
                            }
                        }
                        2 if flags & 1 != 0 && flags & 4 == 0 => {
                            let context = if flags & 8 != 0 {
                                16
                            } else if context == 0 {
                                14
                            } else {
                                15
                            };
                            let delta = 1i64 << plane;
                            samples.values[index] +=
                                if decoder.bit(context) { delta } else { -delta };
                            samples.flags[index] |= 8;
                        }
                        0 if flags & 5 == 0 && decoder.bit(context) => {
                            samples.sign(&mut decoder, x, y, plane);
                        }
                        _ => {}
                    }
                }
            }
        }
        if kind == 0 {
            for f in &mut samples.flags {
                *f &= !4;
            }
            if style & 32 != 0 {
                let mut marker = 0;
                for _ in 0..4 {
                    marker = marker * 2 + u8::from(decoder.bit(18));
                }
                if marker != 10 {
                    return Err(DecodeError::InvalidData);
                }
            }
        }
        if style & 2 != 0 && !decoder.raw {
            decoder.reset();
        }
    }
    let mut out = budget.zeroed::<f64>(width * height)?;
    for ((value, &magnitude), &flags) in out.iter_mut().zip(&samples.values).zip(&samples.flags) {
        *value = if flags & 2 != 0 {
            -(magnitude as f64)
        } else {
            magnitude as f64
        } * 0.5;
    }
    budget.release(samples.flags);
    budget.release(samples.values);
    Ok(out)
}

// MARK: Inverse wavelets
fn floor(value: f64) -> f64 {
    let integer = value as i64;
    if (integer as f64) > value {
        (integer - 1) as f64
    } else {
        integer as f64
    }
}

fn inverse_wavelet_line(values: &mut [f64], start: usize, reversible: bool) {
    if values.len() == 1 {
        if start & 1 != 0 {
            values[0] *= 0.5;
        }
        return;
    }
    let neighbour = |values: &[f64], index: usize| {
        let left = if index == 0 { 1 } else { index - 1 };
        let right = if index + 1 == values.len() {
            index - 1
        } else {
            index + 1
        };
        values[left] + values[right]
    };
    let even = start & 1;
    if reversible {
        for i in (even..values.len()).step_by(2) {
            values[i] -= floor((neighbour(values, i) + 2.0) * 0.25);
        }
        for i in ((1 - even)..values.len()).step_by(2) {
            values[i] += floor(neighbour(values, i) * 0.5);
        }
    } else {
        for (i, value) in values.iter_mut().enumerate() {
            *value *= if (i + start) & 1 == 0 {
                1.230_174_104_914_001
            } else {
                1.0 / 1.230_174_104_914_001
            };
        }
        for (parity, coefficient) in [
            (even, 0.443_506_852_043_971),
            (1 - even, 0.882_911_075_530_934),
            (even, -0.052_980_118_572_961),
            (1 - even, -1.586_134_342_059_924),
        ] {
            for i in (parity..values.len()).step_by(2) {
                values[i] -= coefficient * neighbour(values, i);
            }
        }
    }
}

#[rustfmt::skip]
const STATES: [(u32, usize, usize, bool); 47] = [
    (0x5601,  1,  1, true ), (0x3401,  2,  6, false), (0x1801,  3,  9, false),
    (0x0ac1,  4, 12, false), (0x0521,  5, 29, false), (0x0221, 38, 33, false),
    (0x5601,  7,  6, true ), (0x5401,  8, 14, false), (0x4801,  9, 14, false),
    (0x3801, 10, 14, false), (0x3001, 11, 17, false), (0x2401, 12, 18, false),
    (0x1c01, 13, 20, false), (0x1601, 29, 21, false), (0x5601, 15, 14, true ),
    (0x5401, 16, 14, false), (0x5101, 17, 15, false), (0x4801, 18, 16, false),
    (0x3801, 19, 17, false), (0x3401, 20, 18, false), (0x3001, 21, 19, false),
    (0x2801, 22, 19, false), (0x2401, 23, 20, false), (0x2201, 24, 21, false),
    (0x1c01, 25, 22, false), (0x1801, 26, 23, false), (0x1601, 27, 24, false),
    (0x1401, 28, 25, false), (0x1201, 29, 26, false), (0x1101, 30, 27, false),
    (0x0ac1, 31, 28, false), (0x09c1, 32, 29, false), (0x08a1, 33, 30, false),
    (0x0521, 34, 31, false), (0x0441, 35, 32, false), (0x02a1, 36, 33, false),
    (0x0221, 37, 34, false), (0x0141, 38, 35, false), (0x0111, 39, 36, false),
    (0x0085, 40, 37, false), (0x0049, 41, 38, false), (0x0025, 42, 39, false),
    (0x0015, 43, 40, false), (0x0009, 44, 41, false), (0x0005, 45, 42, false),
    (0x0001, 45, 43, false), (0x5601, 46, 46, false),
];

// MARK: Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jp2_and_raw_codestreams_decode() {
        let jp2 = crate::decode(include_bytes!("../tests/images/jpeg2000/rgb.jp2")).expect("JP2");
        let raw =
            crate::decode(include_bytes!("../tests/images/jpeg2000/rgb.j2k")).expect("codestream");
        assert_eq!(jp2, raw);
        assert_eq!(jp2.format(), Format::Jpeg2000);
        assert_eq!((jp2.width(), jp2.height()), (32, 32));
        assert_eq!(jp2.frames().len(), 1);
        assert!(!jp2.is_animated());
        for y in 0..32 {
            for x in 0..32 {
                let offset = (y * 32 + x) * 4;
                assert_eq!(
                    &jp2.pixels()[offset..offset + 4],
                    &[x as u8 * 8, y as u8 * 8, (x + y) as u8 * 4, 255]
                );
            }
        }
    }

    #[test]
    fn tiles_layers_and_progression_orders_preserve_pixels() {
        let expected =
            crate::decode(include_bytes!("../tests/images/jpeg2000/rgb.jp2")).expect("RGB");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/images/jpeg2000");
        for name in ["tiles", "layers", "rlcp", "rpcl", "pcrl", "cprl"] {
            let data = std::fs::read(root.join(format!("{name}.jp2"))).expect("fixture");
            let actual = crate::decode(&data).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!(actual.pixels(), expected.pixels(), "{name}");
        }
        let odd = crate::decode(include_bytes!("../tests/images/jpeg2000/odd.jp2"))
            .expect("odd tiled JPEG 2000");
        assert_eq!((odd.width(), odd.height()), (19, 13));
        for y in 0..13 {
            for x in 0..19 {
                let index = (y * 19 + x) * 4;
                assert_eq!(
                    &odd.pixels()[index..index + 4],
                    &[x as u8 * 11, y as u8 * 17, (x + y) as u8 * 7, 255]
                );
            }
        }
    }

    #[test]
    fn lossy_alpha_gray_and_icc_match_references() {
        for name in ["rgb", "lossy", "rgba", "gray16", "icc-p3"] {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
            let data = std::fs::read(root.join(format!("images/jpeg2000/{name}.jp2")))
                .expect("JP2 fixture");
            let expected = std::fs::read(root.join(format!("reference/jpeg2000/{name}.jp2.rgba")))
                .expect("OpenJPEG reference");
            let actual = crate::decode(&data).expect("JPEG 2000");
            assert_eq!(actual.pixels().len(), expected.len());
            for (actual, expected) in actual.pixels().iter().zip(&expected) {
                // Wavelet and sample conversion implementations can round differently.
                assert!(
                    actual.abs_diff(*expected) <= 1,
                    "{name}: {actual} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn precincts_tile_parts_and_coding_modes_preserve_pixels() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/images/jpeg2000");
        let expected: Vec<u8> = (0..96)
            .flat_map(|y| {
                (0..128).flat_map(move |x| {
                    [
                        (x * 3 + y * 5) as u8,
                        (x * 3 + y * 5 + 60) as u8,
                        (x * 3 + y * 5 + 120) as u8,
                        255,
                    ]
                })
            })
            .collect();
        for name in [
            "precincts-lrcp",
            "precincts-rlcp",
            "precincts-rpcl",
            "precincts-pcrl",
            "precincts-cprl",
            "mode-1",
            "mode-4",
            "mode-63",
            "parts-l",
            "parts-r",
        ] {
            let bytes = std::fs::read(root.join(format!("{name}.jp2"))).expect("OpenJPEG fixture");
            let image = crate::decode(&bytes).unwrap_or_else(|error| panic!("{name}: {error}"));
            assert_eq!((image.width(), image.height()), (128, 96), "{name}");
            assert_eq!(image.pixels(), expected, "{name}");
        }
    }

    #[test]
    fn spatial_progression_handles_subsampled_components_with_odd_origins() {
        for bytes in [
            include_bytes!("../tests/images/jpeg2000/subsampled-rpcl.j2k").as_slice(),
            include_bytes!("../tests/images/jpeg2000/subsampled-pcrl.j2k").as_slice(),
            include_bytes!("../tests/images/jpeg2000/subsampled-cprl.j2k").as_slice(),
        ] {
            let decoded =
                decode_codestream(bytes, &mut Budget::default()).expect("subsampled codestream");
            assert_eq!((decoded.width, decoded.height), (128, 96));
            assert_eq!(decoded.origin, (3, 5));
            assert_eq!(decoded.sampling, [(1, 1), (2, 2), (2, 2)]);
            for y in 0..96 {
                for x in 0..128 {
                    assert_eq!(
                        decoded.planes[0][y * 128 + x],
                        ((x * 3 + y * 5) % 256) as f64
                    );
                }
            }
            assert_eq!(decoded.planes[1].len(), 64 * 48);
            assert_eq!(decoded.planes[2].len(), 64 * 48);
            assert!(decoded.planes[1].iter().all(|&v| v == 120.0));
            assert!(decoded.planes[2].iter().all(|&v| v == 140.0));
        }
    }

    #[test]
    fn large_image_does_not_double_charge_planes_or_keep_scratch_allocations() {
        let image = crate::decode(include_bytes!("../tests/images/jpeg2000/large.jp2"))
            .expect("12 megapixel JPEG 2000");
        assert_eq!((image.width(), image.height()), (4000, 3000));
        assert!(
            image
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [80, 120, 160, 255])
        );
    }

    #[test]
    fn missing_duplicate_and_misnumbered_tile_parts_fail() {
        let bytes = include_bytes!("../tests/images/jpeg2000/parts-l.jp2");
        let positions: Vec<_> = bytes
            .windows(2)
            .enumerate()
            .filter_map(|(i, marker)| (marker == [255, 0x90]).then_some(i))
            .collect();
        assert!(positions.len() > 1);
        for (at, value) in [
            (positions[0] + 10, 1),
            (positions[1] + 10, 0),
            (positions[0] + 11, 1),
        ] {
            let mut damaged = bytes.to_vec();
            damaged[at] = value;
            assert!(crate::decode(&damaged).is_err());
        }
        // Removing a tile-part while keeping a complete JP2 container must fail.
        let mut damaged = bytes.to_vec();
        let start = positions[1];
        let length =
            u32::from_be_bytes(bytes[start + 6..start + 10].try_into().expect("length")) as usize;
        damaged.drain(start..start + length);
        let jp2c = bytes
            .windows(4)
            .position(|kind| kind == b"jp2c")
            .expect("codestream");
        let size = u32::from_be_bytes(bytes[jp2c - 4..jp2c].try_into().expect("box length"));
        damaged[jp2c - 4..jp2c].copy_from_slice(&(size - length as u32).to_be_bytes());
        assert!(crate::decode(&damaged).is_err());
    }

    #[test]
    fn extensions_signatures_and_decode_only() {
        for extension in ["J2C", "j2k", "jp2", "jpc"] {
            assert_eq!(Format::from_extension(extension), Some(Format::Jpeg2000));
        }
        assert_eq!(
            Format::from_signature(b"\xff\x4f\xff\x51"),
            Some(Format::Jpeg2000)
        );
        assert_eq!(
            Format::from_signature(b"\0\0\0\x0cjP  \r\n\x87\n"),
            Some(Format::Jpeg2000)
        );
        assert!(!Format::Jpeg2000.can_encode());
        let bitmap = crate::Bitmap::new(1, 1, alloc::vec![0; 4]).expect("bitmap");
        assert_eq!(
            crate::encode(&bitmap, Format::Jpeg2000),
            Err(crate::EncodeError::UnsupportedFormat)
        );
    }

    #[test]
    fn malformed_and_oversized_codestreams_fail() {
        let bytes = include_bytes!("../tests/images/jpeg2000/rgb.j2k");
        for end in 4..64 {
            assert!(crate::decode(&bytes[..end]).is_err(), "length {end}");
        }
        let mut data = bytes.to_vec();
        // SIZ: Xsiz and Ysiz follow Lsiz and Rsiz.
        data[8..12].copy_from_slice(&100_000u32.to_be_bytes());
        data[12..16].copy_from_slice(&100_000u32.to_be_bytes());
        assert!(crate::decode(&data).is_err());
        assert!(crate::decode(b"\0\0\0\x0cjP  \r\n\x87\n").is_err());
    }
}

#[cfg(test)]
mod corruption_tests {
    #[test]
    fn damaged_codestreams_do_not_panic() {
        for source in [
            include_bytes!("../tests/images/jpeg2000/rgb.jp2").as_slice(),
            include_bytes!("../tests/images/jpeg2000/rgba.jp2").as_slice(),
            include_bytes!("../tests/images/jpeg2000/odd.jp2").as_slice(),
            include_bytes!("../tests/images/jpeg2000/precincts-pcrl.jp2").as_slice(),
            include_bytes!("../tests/images/jpeg2000/mode-63.jp2").as_slice(),
            include_bytes!("../tests/images/jpeg2000/parts-l.jp2").as_slice(),
        ] {
            for index in (0..source.len()).step_by(if source.len() > 4096 { 101 } else { 11 }) {
                let mut data = source.to_vec();
                data[index] ^= 255;
                let _ = crate::decode(&data);
            }
        }
    }
}
