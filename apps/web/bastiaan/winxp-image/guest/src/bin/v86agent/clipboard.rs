/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Clipboard sync: text, HTML and images, each format travels in chunks

use std::{ptr, slice};

use v86::protocol::*;
use v86::win32::*;

use crate::{CHUNK_SIZE, Device};

const FORMATS: usize = V86_CLIP_COUNT as usize;
const HTML_PREFIX: &str = "<html><body>\r\n<!--StartFragment-->";
const HTML_SUFFIX: &str = "<!--EndFragment-->\r\n</body></html>";

/// The data of each format
type Formats = [Option<Vec<u8>>; FORMATS];

#[derive(Default)]
pub(crate) struct Clipboard {
    pub(crate) html_format: u32,
    last_hash: u32,
    /// The clipboard the host is sending, the format that's coming in with its size
    incoming: Formats,
    current: Option<(usize, usize)>,
}

/// FNV-1a over all formats, so a clipboard the host just set isn't echoed back
fn hash(formats: &Formats) -> u32 {
    let mut hash = 2166136261_u32;
    for (format, data) in formats.iter().enumerate() {
        if let Some(data) = data {
            for byte in (format as u32)
                .to_le_bytes()
                .iter()
                .chain(&(data.len() as u32).to_le_bytes())
                .chain(data)
            {
                hash = (hash ^ u32::from(*byte)).wrapping_mul(16777619);
            }
        }
    }
    hash
}

/// Converts the data of a clipboard format while it's locked, the clipboard must be open
fn read(format: u32, convert: impl FnOnce(&[u8]) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    // SAFETY: The memory of a clipboard format holds GlobalSize bytes while it's locked
    unsafe {
        let memory = GetClipboardData(format);
        let data = if memory.is_null() {
            ptr::null_mut()
        } else {
            GlobalLock(memory)
        };
        if data.is_null() {
            return None;
        }
        let converted = convert(slice::from_raw_parts(data.cast::<u8>(), GlobalSize(memory)));
        GlobalUnlock(memory);
        converted
    }
}

/// Puts data of the size on the open clipboard, filled in place in its global memory
fn write(format: u32, size: usize, fill: impl FnOnce(&mut [u8])) {
    // SAFETY: The locked memory holds size bytes, the clipboard owns it once it's set
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, size);
        let target = if memory.is_null() {
            ptr::null_mut()
        } else {
            GlobalLock(memory)
        };
        if target.is_null() {
            if !memory.is_null() {
                GlobalFree(memory);
            }
            return;
        }
        fill(slice::from_raw_parts_mut(target.cast::<u8>(), size));
        GlobalUnlock(memory);
        if SetClipboardData(format, memory).is_null() {
            GlobalFree(memory);
        }
    }
}

/// A copy that gives up when memory runs out, clipboard formats can be large
fn try_copy(data: &[u8]) -> Option<Vec<u8>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(data.len()).ok()?;
    copy.extend_from_slice(data);
    Some(copy)
}

/// UTF-16 text without its NUL
fn text_from(data: &[u8]) -> Option<Vec<u8>> {
    let end = data
        .as_chunks::<2>()
        .0
        .iter()
        .position(|unit| *unit == [0, 0])
        .map_or(data.len() & !1, |i| i * 2);
    try_copy(&data[..end])
}

/// CF_HTML is an ASCII header with the byte offsets of the fragment in the UTF-8 data that follows
fn html_fragment(data: &[u8]) -> Option<Vec<u8>> {
    let header = &data[..data.len().min(511)];
    let header = &header[..header
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(header.len())];
    let offset = |key: &str| {
        let start = header
            .windows(key.len())
            .position(|window| window == key.as_bytes())?
            + key.len();
        Some(
            header[start..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .fold(0_usize, |value, digit| {
                    value * 10 + usize::from(digit - b'0')
                }),
        )
    };
    let (start, end) = (
        offset("StartFragment:").unwrap_or(0),
        offset("EndFragment:").unwrap_or(0),
    );
    if start == 0 || start > end || end > data.len() {
        return None;
    }
    try_copy(&data[start..end])
}

/// Puts an HTML fragment on the open clipboard as CF_HTML
fn write_html(format: u32, fragment: &[u8]) {
    let header =
        |start_html: usize, end_html: usize, start_fragment: usize, end_fragment: usize| {
            format!(
                "Version:0.9\r\nStartHTML:{start_html:010}\r\nEndHTML:{end_html:010}\r\n\
             StartFragment:{start_fragment:010}\r\nEndFragment:{end_fragment:010}\r\n"
            )
        };
    let start_html = header(0, 0, 0, 0).len();
    let start_fragment = start_html + HTML_PREFIX.len();
    let end_fragment = start_fragment + fragment.len();
    let end_html = end_fragment + HTML_SUFFIX.len();
    let header = header(start_html, end_html, start_fragment, end_fragment);
    write(format, end_html + 1, |document| {
        for (range, part) in [
            (0..start_html, header.as_bytes()),
            (start_html..start_fragment, HTML_PREFIX.as_bytes()),
            (start_fragment..end_fragment, fragment),
            (end_fragment..end_html, HTML_SUFFIX.as_bytes()),
        ] {
            document[range].copy_from_slice(part);
        }
        document[end_html] = 0;
    });
}

/// Any CF_DIB (palette, 16/24/32 bit, RLE) drawn into a 32 bit top-down DIB section, as width,
/// height and BGRX pixels
fn image_from_dib(dib: &[u8]) -> Option<Vec<u8>> {
    if dib.len() < size_of::<BITMAPINFOHEADER>() {
        return None;
    }
    // SAFETY: The data holds a header
    let info = unsafe { dib.as_ptr().cast::<BITMAPINFOHEADER>().read_unaligned() };
    let (width, height) = (info.biWidth, info.biHeight.unsigned_abs() as i32);
    if (info.biSize as usize) < size_of::<BITMAPINFOHEADER>()
        || width <= 0
        || height <= 0
        || width as u64 * height as u64 * 4 > u64::from(V86_CLIP_MAX - 8)
    {
        return None;
    }
    let colors = match info.biClrUsed {
        0 if info.biBitCount <= 8 => 1 << info.biBitCount,
        used => used as usize,
    };
    let masks = if info.biCompression == BI_BITFIELDS
        && info.biSize as usize == size_of::<BITMAPINFOHEADER>()
    {
        12
    } else {
        0
    };
    let offset = info.biSize as usize + colors * 4 + masks;
    let pixels_size =
        (width as usize * usize::from(info.biBitCount)).div_ceil(32) * 4 * height as usize;
    match info.biCompression {
        BI_RGB | BI_BITFIELDS if offset <= dib.len() && pixels_size <= dib.len() - offset => {}
        BI_RLE8 | BI_RLE4 if offset <= dib.len() => {}
        _ => return None,
    }

    let out = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width,
        biHeight: -height,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        ..Default::default()
    };
    let count = width as usize * height as usize;
    // SAFETY: The DIB section holds count pixels, the source DIB was checked above, the GDI
    // objects are released after
    unsafe {
        let mut pixels = ptr::null_mut();
        let section = CreateDIBSection(
            ptr::null_mut(),
            &out,
            DIB_RGB_COLORS,
            &mut pixels,
            ptr::null_mut(),
            0,
        );
        if section.is_null() {
            return None;
        }
        let dc = CreateCompatibleDC(ptr::null_mut());
        let old = SelectObject(dc, section);
        let drawn = SetDIBitsToDevice(
            dc,
            0,
            0,
            width as u32,
            height as u32,
            0,
            0,
            0,
            height as u32,
            dib.as_ptr().add(offset).cast(),
            dib.as_ptr().cast(),
            DIB_RGB_COLORS,
        ) > 0;
        let mut image = Vec::new();
        let image = (drawn && image.try_reserve_exact(8 + count * 4).is_ok()).then(|| {
            GdiFlush();
            image.extend_from_slice(&(width as u32).to_le_bytes());
            image.extend_from_slice(&(height as u32).to_le_bytes());
            for pixel in slice::from_raw_parts(pixels.cast::<u32>(), count) {
                image.extend_from_slice(&(pixel & 0xFFFFFF).to_le_bytes());
            }
            image
        });
        SelectObject(dc, old);
        DeleteDC(dc);
        DeleteObject(section);
        image
    }
}

/// Puts an image on the open clipboard as a bottom-up CF_DIB, the layout old apps expect
fn write_image(image: &[u8]) {
    let word = |i: usize| Some(u32::from_le_bytes(image.get(i..i + 4)?.try_into().ok()?));
    let (Some(width), Some(height)) = (word(0), word(4)) else {
        return;
    };
    let (width, height) = (width as usize, height as usize);
    let Some(stride) = width.checked_mul(4) else {
        return;
    };
    let pixels = &image[8..];
    if width == 0
        || height == 0
        || stride
            .checked_mul(height)
            .is_none_or(|size| size > pixels.len())
    {
        return;
    }
    let info = BITMAPINFOHEADER {
        biSize: size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: width as i32,
        biHeight: height as i32,
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: (stride * height) as u32,
        ..Default::default()
    };
    let header_size = size_of::<BITMAPINFOHEADER>();
    write(CF_DIB, header_size + stride * height, |dib| {
        // SAFETY: The header is plain data
        let header = unsafe { slice::from_raw_parts((&raw const info).cast::<u8>(), header_size) };
        dib[..header_size].copy_from_slice(header);
        let rows = pixels[..stride * height].chunks_exact(stride).rev();
        for (target, row) in dib[header_size..].chunks_exact_mut(stride).zip(rows) {
            target.copy_from_slice(row);
        }
    });
}

impl Clipboard {
    /// The clipboard changed: sends it to the host unless it came from there
    pub(crate) fn changed(&mut self, hwnd: HWND, device: &Device) {
        // SAFETY: The window is the agent's
        if unsafe { OpenClipboard(hwnd) } == 0 {
            return;
        }
        let formats: Formats = [
            read(CF_UNICODETEXT, text_from),
            (self.html_format != 0)
                .then(|| read(self.html_format, html_fragment))
                .flatten(),
            read(CF_DIB, image_from_dib),
        ];
        // SAFETY: The clipboard is open
        unsafe { CloseClipboard() };

        let hash = hash(&formats);
        if hash == self.last_hash {
            return;
        }
        self.last_hash = hash;
        for (format, data) in formats.iter().enumerate() {
            let Some(data) = data else {
                continue;
            };
            let head = [format as u32, data.len() as u32]
                .map(u32::to_le_bytes)
                .concat();
            device.send(V86_MSG_CLIP_FORMAT, &head);
            for chunk in data.chunks(CHUNK_SIZE) {
                device.send(V86_MSG_CLIP_DATA, chunk);
            }
        }
        device.send(V86_MSG_CLIP_END, &[]);
    }

    /// The host starts a format: u32 format and u32 size
    pub(crate) fn begin(&mut self, data: &[u8]) {
        let word = |i: usize| {
            data.get(i..i + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        self.current = (|| match (word(0), word(4)) {
            (Some(format), Some(size)) if format < V86_CLIP_COUNT && size <= V86_CLIP_MAX => {
                // A format that doesn't fit in memory is left out
                let mut data = Vec::new();
                data.try_reserve_exact(size as usize).ok()?;
                self.incoming[format as usize] = Some(data);
                Some((format as usize, size as usize))
            }
            _ => None,
        })();
    }

    pub(crate) fn data(&mut self, data: &[u8]) {
        if let Some((format, size)) = self.current
            && let Some(incoming) = &mut self.incoming[format]
        {
            let room = size - incoming.len();
            incoming.extend_from_slice(&data[..data.len().min(room)]);
        }
    }

    /// The host clipboard is complete, it replaces the guest one
    pub(crate) fn end(&mut self, hwnd: HWND) {
        let formats = std::mem::take(&mut self.incoming);
        self.current = None;
        self.last_hash = hash(&formats);
        // SAFETY: The window is the agent's, the clipboard is closed after
        unsafe {
            if OpenClipboard(hwnd) == 0 {
                return;
            }
            EmptyClipboard();
        }
        let [text, html, image] = &formats;
        if let Some(text) = text {
            let size = text.len() & !1;
            write(CF_UNICODETEXT, size + 2, |target| {
                target[..size].copy_from_slice(&text[..size]);
                target[size..].fill(0);
            });
        }
        if let Some(html) = html
            && self.html_format != 0
        {
            write_html(self.html_format, html);
        }
        if let Some(image) = image {
            write_image(image);
        }
        // SAFETY: The clipboard is open
        unsafe { CloseClipboard() };
    }
}
