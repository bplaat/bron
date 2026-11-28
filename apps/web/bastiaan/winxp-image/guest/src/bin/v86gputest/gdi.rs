/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The GDI cases: each draws into a DIB section and onto the screen, the screen read back has to
//! match the engine's picture.

use std::ptr;

use v86::win32::*;

use crate::{Dib, HBITMAP, Report, SRCCOPY, create_window, destroy_window, settle};

const WIDTH: i32 = 200;
const HEIGHT: i32 = 150;

const PATCOPY: u32 = 0x00F0_0021;
const PATINVERT: u32 = 0x005A_0049;
const DSTINVERT: u32 = 0x0055_0009;
const BLACKNESS: u32 = 0x0000_0042;
const WHITENESS: u32 = 0x00FF_0062;
const SRCINVERT: u32 = 0x0066_0046;
const SRCAND: u32 = 0x0088_00C6;
const SRCPAINT: u32 = 0x00EE_0086;
const NOTSRCCOPY: u32 = 0x0033_0008;
const MERGECOPY: u32 = 0x00C0_00CA;
/// Pattern where the source is set, destination elsewhere: transparent text and masks
const PSDPXAX: u32 = 0x00B8_074A;
const HS_DIAGCROSS: i32 = 5;
const OPAQUE: i32 = 2;
const TRANSPARENT: i32 = 1;
const COLORONCOLOR: i32 = 3;
const BLACKONWHITE: i32 = 1;
const ETO_OPAQUE: u32 = 2;
const R2_XORPEN: i32 = 7;
const NONANTIALIASED_QUALITY: u32 = 3;
const AC_SRC_ALPHA: u8 = 1;

type HGDIOBJ = HANDLE;

#[repr(C)]
#[derive(Clone, Copy)]
struct BlendFunction {
    op: u8,
    flags: u8,
    constant: u8,
    format: u8,
}

#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateSolidBrush(color: u32) -> HGDIOBJ;
    fn CreateHatchBrush(style: i32, color: u32) -> HGDIOBJ;
    fn CreatePatternBrush(bitmap: HBITMAP) -> HGDIOBJ;
    fn CreatePen(style: i32, width: i32, color: u32) -> HGDIOBJ;
    fn CreateBitmap(width: i32, height: i32, planes: u32, bpp: u32, bits: *const u8) -> HBITMAP;
    fn CreateEllipticRgn(left: i32, top: i32, right: i32, bottom: i32) -> HGDIOBJ;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strike_out: u32,
        charset: u32,
        precision: u32,
        clip: u32,
        quality: u32,
        pitch: u32,
        face: *const u16,
    ) -> HGDIOBJ;
    fn PatBlt(dc: HDC, x: i32, y: i32, width: i32, height: i32, rop: u32) -> BOOL;
    fn BitBlt(
        dc: HDC,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        src: HDC,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> BOOL;
    fn StretchBlt(
        dc: HDC,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        src: HDC,
        sx: i32,
        sy: i32,
        swidth: i32,
        sheight: i32,
        rop: u32,
    ) -> BOOL;
    fn SetStretchBltMode(dc: HDC, mode: i32) -> i32;
    fn SetTextColor(dc: HDC, color: u32) -> u32;
    fn SetBkColor(dc: HDC, color: u32) -> u32;
    fn SetBkMode(dc: HDC, mode: i32) -> i32;
    fn SetROP2(dc: HDC, mode: i32) -> i32;
    fn SetBrushOrgEx(dc: HDC, x: i32, y: i32, old: *mut Point) -> BOOL;
    fn TextOutW(dc: HDC, x: i32, y: i32, text: *const u16, count: i32) -> BOOL;
    fn ExtTextOutW(
        dc: HDC,
        x: i32,
        y: i32,
        options: u32,
        rect: *const [i32; 4],
        text: *const u16,
        count: u32,
        dx: *const i32,
    ) -> BOOL;
    fn MoveToEx(dc: HDC, x: i32, y: i32, old: *mut Point) -> BOOL;
    fn LineTo(dc: HDC, x: i32, y: i32) -> BOOL;
    fn Polyline(dc: HDC, points: *const Point, count: i32) -> BOOL;
    fn SelectClipRgn(dc: HDC, region: HGDIOBJ) -> i32;
}

#[link(name = "msimg32")]
unsafe extern "system" {
    fn AlphaBlend(
        dc: HDC,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: HDC,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        blend: BlendFunction,
    ) -> BOOL;
    fn TransparentBlt(
        dc: HDC,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: HDC,
        sx: i32,
        sy: i32,
        sw: i32,
        sh: i32,
        key: u32,
    ) -> BOOL;
}

/// The sources the cases draw from, the same for both pictures
struct Sources {
    rgb: Dib,
    indexed: Dib,
    nibbles: Dib,
    true_color: Dib,
    alpha: Dib,
    mono: HBITMAP,
    mono_dc: HDC,
    pattern: HBITMAP,
}

impl Sources {
    fn new() -> Self {
        let mut rgb = Dib::new(64, 48, 32, &[]);
        rgb.noise(1);
        let palette: Vec<u32> = (0..256).map(|i: u32| (i * 0x010305) & 0xFF_FFFF).collect();
        let mut indexed = Dib::new(64, 48, 8, &palette);
        indexed.noise(2);
        let mut nibbles = Dib::new(64, 48, 4, &palette[..16]);
        nibbles.noise(3);
        let mut true_color = Dib::new(64, 48, 24, &[]);
        true_color.noise(4);
        // Premultiplied: no channel above its alpha
        let mut alpha = Dib::new(64, 48, 32, &[]);
        alpha.noise(5);
        for pixel in alpha.bytes_mut().chunks_mut(4) {
            let a = pixel[3];
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * u16::from(a)) / 255) as u8;
            }
        }
        let mono_bits: Vec<u8> = (0..48 * 8).map(|i| (i * 37 % 251) as u8).collect();
        let pattern_bits: [u8; 16] = [
            0x81, 0, 0x42, 0, 0x24, 0, 0x18, 0, 0x18, 0, 0x24, 0, 0x42, 0, 0x81, 0,
        ];
        // SAFETY: The bits hold the rows of the bitmaps, padded to words
        unsafe {
            let mono = CreateBitmap(64, 48, 1, 1, mono_bits.as_ptr());
            let mono_dc = CreateCompatibleDC(ptr::null_mut());
            SelectObject(mono_dc, mono);
            Self {
                rgb,
                indexed,
                nibbles,
                true_color,
                alpha,
                mono,
                mono_dc,
                pattern: CreateBitmap(8, 8, 1, 1, pattern_bits.as_ptr()),
            }
        }
    }
}

impl Drop for Sources {
    fn drop(&mut self) {
        // SAFETY: The objects belong to the sources
        unsafe {
            DeleteDC(self.mono_dc);
            DeleteObject(self.mono);
            DeleteObject(self.pattern);
        }
    }
}

/// Runs `draw` with a brush selected
///
/// # Safety
///
/// The DC and brush are valid.
unsafe fn with_object(dc: HDC, object: HGDIOBJ, draw: impl FnOnce()) {
    // SAFETY: The caller passes valid objects, the old one is selected back
    unsafe {
        let old = SelectObject(dc, object);
        draw();
        SelectObject(dc, old);
        DeleteObject(object);
    }
}

type Case = fn(HDC, &Sources);

const CASES: &[(&str, Case)] = &[
    // SAFETY: GDI calls on the DC and the test's sources
    ("solid fills", |dc, _| unsafe {
        with_object(dc, CreateSolidBrush(0x00336699), || {
            PatBlt(dc, 10, 10, 120, 70, PATCOPY);
            PatBlt(dc, 50, 40, 100, 90, PATINVERT);
        });
        PatBlt(dc, 5, 100, 60, 30, BLACKNESS);
        PatBlt(dc, 70, 100, 60, 30, WHITENESS);
        PatBlt(dc, 20, 20, 150, 100, DSTINVERT);
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("hatch and pattern brushes", |dc, sources| unsafe {
        SetBkMode(dc, OPAQUE);
        SetBkColor(dc, 0x00F0E0D0);
        SetTextColor(dc, 0x00102030);
        with_object(dc, CreateHatchBrush(HS_DIAGCROSS, 0x000000FF), || {
            PatBlt(dc, 3, 7, 130, 90, PATCOPY);
        });
        SetBrushOrgEx(dc, 3, 5, ptr::null_mut());
        with_object(dc, CreatePatternBrush(sources.pattern), || {
            PatBlt(dc, 60, 40, 130, 100, PATINVERT);
        });
        SetBrushOrgEx(dc, 0, 0, ptr::null_mut());
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("32 bpp sources with ROPs", |dc, sources| unsafe {
        let src = sources.rgb.dc;
        BitBlt(dc, 0, 0, 64, 48, src, 0, 0, SRCCOPY);
        BitBlt(dc, 70, 0, 64, 48, src, 0, 0, SRCINVERT);
        BitBlt(dc, 0, 50, 64, 48, src, 0, 0, SRCAND);
        BitBlt(dc, 70, 50, 64, 48, src, 0, 0, SRCPAINT);
        BitBlt(dc, 0, 100, 64, 48, src, 0, 0, NOTSRCCOPY);
        with_object(dc, CreateSolidBrush(0x0055AA33), || {
            BitBlt(dc, 70, 100, 50, 40, src, 10, 5, MERGECOPY);
            BitBlt(dc, 130, 60, 60, 40, src, 2, 3, PSDPXAX);
        });
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("indexed and 24 bpp sources", |dc, sources| unsafe {
        BitBlt(dc, 0, 0, 64, 48, sources.indexed.dc, 0, 0, SRCCOPY);
        BitBlt(dc, 70, 0, 64, 48, sources.nibbles.dc, 0, 0, SRCCOPY);
        BitBlt(dc, 0, 50, 64, 48, sources.true_color.dc, 0, 0, SRCCOPY);
        SetTextColor(dc, 0x000080FF);
        SetBkColor(dc, 0x00203040);
        BitBlt(dc, 70, 50, 64, 48, sources.mono_dc, 0, 0, SRCCOPY);
        with_object(dc, CreateSolidBrush(0x00FF00FF), || {
            BitBlt(dc, 130, 100, 64, 48, sources.mono_dc, 0, 0, PSDPXAX);
        });
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("overlapping screen copies", |dc, sources| unsafe {
        BitBlt(dc, 0, 0, WIDTH, HEIGHT, sources.rgb.dc, 0, 0, SRCCOPY);
        BitBlt(dc, 5, 3, 120, 90, dc, 0, 0, SRCCOPY);
        BitBlt(dc, 40, 30, 120, 90, dc, 47, 36, SRCCOPY);
        BitBlt(dc, 100, 20, 80, 60, dc, 90, 20, SRCCOPY);
        BitBlt(dc, 10, 60, 80, 60, dc, 10, 70, SRCCOPY);
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("complex clipping", |dc, sources| unsafe {
        let region = CreateEllipticRgn(10, 10, 190, 140);
        SelectClipRgn(dc, region);
        with_object(dc, CreateSolidBrush(0x00804020), || {
            PatBlt(dc, 0, 0, WIDTH, HEIGHT, PATCOPY);
        });
        BitBlt(dc, 30, 20, 64, 48, sources.rgb.dc, 0, 0, SRCCOPY);
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, 0x00FFFFFF);
        let text = w!("Clipped text, Windows XP");
        TextOutW(dc, 0, 70, text.as_ptr(), text.len() as i32 - 1);
        SelectClipRgn(dc, ptr::null_mut());
        DeleteObject(region);
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("text", |dc, _| unsafe {
        let face = w!("Tahoma");
        let font = CreateFontW(
            -13,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            0,
            0,
            0,
            NONANTIALIASED_QUALITY,
            0,
            face.as_ptr(),
        );
        with_object(dc, font, || {
            let text = w!("The quick brown fox jumps");
            SetBkMode(dc, OPAQUE);
            SetBkColor(dc, 0x00FFFFC0);
            SetTextColor(dc, 0x00000080);
            TextOutW(dc, 4, 4, text.as_ptr(), text.len() as i32 - 1);
            SetBkMode(dc, TRANSPARENT);
            SetTextColor(dc, 0x00008000);
            TextOutW(dc, 4, 24, text.as_ptr(), text.len() as i32 - 1);
            let rect = [0, 50, 150, 80];
            SetBkColor(dc, 0x00C0FFC0);
            ExtTextOutW(
                dc,
                10,
                55,
                ETO_OPAQUE,
                &rect,
                text.as_ptr(),
                12,
                ptr::null(),
            );
        });
        let fixed = w!("Courier New");
        let font = CreateFontW(
            -12,
            0,
            0,
            0,
            700,
            0,
            0,
            0,
            0,
            0,
            0,
            NONANTIALIASED_QUALITY,
            1,
            fixed.as_ptr(),
        );
        with_object(dc, font, || {
            let text = w!("Monospaced 0123");
            TextOutW(dc, 4, 100, text.as_ptr(), text.len() as i32 - 1);
        });
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("lines", |dc, _| unsafe {
        with_object(dc, CreatePen(0, 0, 0x000000FF), || {
            MoveToEx(dc, 10, 10, ptr::null_mut());
            LineTo(dc, 180, 10);
            LineTo(dc, 180, 120);
            LineTo(dc, 10, 120);
            LineTo(dc, 10, 10);
            MoveToEx(dc, 20, 30, ptr::null_mut());
            LineTo(dc, 150, 100);
            let points = [
                Point { x: 30, y: 40 },
                Point { x: 120, y: 40 },
                Point { x: 120, y: 90 },
                Point { x: 30, y: 90 },
            ];
            Polyline(dc, points.as_ptr(), 4);
            SetROP2(dc, R2_XORPEN);
            MoveToEx(dc, 100, 5, ptr::null_mut());
            LineTo(dc, 100, 140);
            MoveToEx(dc, 190, 70, ptr::null_mut());
            LineTo(dc, 5, 70);
        });
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("stretching", |dc, sources| unsafe {
        SetStretchBltMode(dc, COLORONCOLOR);
        StretchBlt(dc, 0, 0, 128, 96, sources.rgb.dc, 0, 0, 64, 48, SRCCOPY);
        StretchBlt(
            dc,
            130,
            0,
            64,
            144,
            sources.indexed.dc,
            0,
            0,
            32,
            48,
            SRCCOPY,
        );
        SetStretchBltMode(dc, BLACKONWHITE);
        StretchBlt(
            dc,
            0,
            100,
            96,
            48,
            sources.true_color.dc,
            0,
            0,
            32,
            16,
            SRCCOPY,
        );
        StretchBlt(dc, 100, 100, 30, 40, sources.rgb.dc, 0, 0, 30, 40, SRCCOPY);
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("shrinking", |dc, sources| unsafe {
        SetStretchBltMode(dc, COLORONCOLOR);
        StretchBlt(dc, 0, 0, 32, 24, sources.rgb.dc, 0, 0, 64, 48, SRCCOPY);
        StretchBlt(dc, 40, 0, 50, 37, sources.rgb.dc, 0, 0, 64, 48, SRCCOPY);
        StretchBlt(dc, 100, 0, 90, 70, sources.rgb.dc, 0, 0, 64, 48, SRCCOPY);
    }),
    // SAFETY: GDI calls on the DC and the test's sources
    ("transparent copies", |dc, sources| unsafe {
        let key = sources.rgb.pixel(3, 3);
        TransparentBlt(dc, 0, 0, 64, 48, sources.rgb.dc, 0, 0, 64, 48, key);
        TransparentBlt(
            dc,
            70,
            0,
            64,
            48,
            sources.indexed.dc,
            0,
            0,
            64,
            48,
            0x00030608,
        );
        TransparentBlt(dc, 0, 50, 128, 96, sources.rgb.dc, 0, 0, 64, 48, key);
    }),
];

/// Blending rounds a little differently than the engine, one step per channel is allowed
const ALPHA_CASES: &[(&str, Case)] = &[(
    "alpha blending",
    // SAFETY: GDI calls on the DC and the test's sources
    |dc, sources| unsafe {
        let constant = BlendFunction {
            op: 0,
            flags: 0,
            constant: 128,
            format: 0,
        };
        AlphaBlend(dc, 0, 0, 64, 48, sources.rgb.dc, 0, 0, 64, 48, constant);
        let per_pixel = BlendFunction {
            op: 0,
            flags: 0,
            constant: 255,
            format: AC_SRC_ALPHA,
        };
        AlphaBlend(dc, 70, 0, 64, 48, sources.alpha.dc, 0, 0, 64, 48, per_pixel);
        let both = BlendFunction {
            op: 0,
            flags: 0,
            constant: 100,
            format: AC_SRC_ALPHA,
        };
        AlphaBlend(dc, 0, 60, 64, 48, sources.alpha.dc, 0, 0, 64, 48, both);
    },
)];

/// Compares the picture on the screen to the engine's, allowing `tolerance` per channel
fn compare(expected: &Dib, actual: &Dib, tolerance: u32) -> Result<(), String> {
    let mut wrong = 0;
    let mut first = None;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (a, b) = (expected.pixel(x, y), actual.pixel(x, y));
            let channels = if expected.bpp == 16 {
                [
                    (a >> 11, b >> 11),
                    ((a >> 5) & 63, (b >> 5) & 63),
                    (a & 31, b & 31),
                ]
            } else {
                [
                    (a >> 16, b >> 16),
                    ((a >> 8) & 255, (b >> 8) & 255),
                    (a & 255, b & 255),
                ]
            };
            if channels.iter().any(|&(a, b)| a.abs_diff(b) > tolerance) {
                wrong += 1;
                first.get_or_insert((x, y, a, b));
            }
        }
    }
    match first {
        None => Ok(()),
        Some((x, y, a, b)) => Err(format!(
            "{wrong} pixels differ, first at {x},{y}: expected {a:06x}, got {b:06x}"
        )),
    }
}

pub fn run(report: &mut Report, bpp: u32) {
    let hwnd = create_window(WIDTH, HEIGHT);
    settle();
    let sources = Sources::new();
    let mut base = Dib::new(WIDTH, HEIGHT, bpp as u16, &[]);
    base.noise(9);
    let expected = Dib::new(WIDTH, HEIGHT, bpp as u16, &[]);
    let actual = Dib::new(WIDTH, HEIGHT, bpp as u16, &[]);
    for (cases, tolerance) in [(CASES, 0), (ALPHA_CASES, 1)] {
        for (name, case) in cases {
            // SAFETY: The window DC is released again, the DIBs belong to this test
            let result = unsafe {
                let screen = GetDC(hwnd);
                BitBlt(expected.dc, 0, 0, WIDTH, HEIGHT, base.dc, 0, 0, SRCCOPY);
                BitBlt(screen, 0, 0, WIDTH, HEIGHT, base.dc, 0, 0, SRCCOPY);
                case(expected.dc, &sources);
                case(screen, &sources);
                GdiFlush();
                BitBlt(actual.dc, 0, 0, WIDTH, HEIGHT, screen, 0, 0, SRCCOPY);
                ReleaseDC(hwnd, screen);
                compare(&expected, &actual, tolerance)
            };
            report.check(&format!("{bpp} bpp {name}"), result);
        }
    }
    // SAFETY: The window is the test's
    unsafe { destroy_window(hwnd) };
}
