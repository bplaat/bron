/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The DirectDraw cases in a fullscreen 640x480 mode: blits checked through locks, flips and
//! palettes checked by the page on the shown picture.

use std::ffi::c_void;
use std::ptr;

use v86::win32::*;

use crate::{Report, create_window, destroy_window, settle};

const DDSCL_FULLSCREEN: u32 = 0x1;
const DDSCL_EXCLUSIVE: u32 = 0x10;
const DDSD_CAPS: u32 = 0x1;
const DDSD_HEIGHT: u32 = 0x2;
const DDSD_WIDTH: u32 = 0x4;
const DDSD_BACKBUFFERCOUNT: u32 = 0x20;
const DDSCAPS_COMPLEX: u32 = 0x8;
const DDSCAPS_FLIP: u32 = 0x10;
const DDSCAPS_BACKBUFFER: u32 = 0x4;
const DDSCAPS_OFFSCREENPLAIN: u32 = 0x40;
const DDSCAPS_PRIMARYSURFACE: u32 = 0x200;
const DDSCAPS_VIDEOMEMORY: u32 = 0x4000;
const DDLOCK_WAIT: u32 = 0x1;
const DDBLT_COLORFILL: u32 = 0x400;
const DDBLT_DDFX: u32 = 0x800;
const DDBLT_KEYDESTOVERRIDE: u32 = 0x4000;
const DDBLT_KEYSRCOVERRIDE: u32 = 0x1_0000;
const DDBLT_WAIT: u32 = 0x100_0000;
const DDBLTFX_MIRRORLEFTRIGHT: u32 = 0x2;
const DDBLTFX_MIRRORUPDOWN: u32 = 0x4;
const DDFLIP_WAIT: u32 = 0x1;
const DDWAITVB_BLOCKBEGIN: u32 = 0x1;
const DDPCAPS_8BIT: u32 = 0x4;
const DDPCAPS_ALLOW256: u32 = 0x40;
const DDCAPS_BLT: u32 = 0x40;
const DDERR_VERTICALBLANKINPROGRESS: i32 = 0x8876_0219_u32 as i32;
const DD_OK: i32 = 0;
const IID_IDIRECTDRAW7: GUID = GUID(
    0x15e6_5ec0,
    0x3b9c,
    0x11d2,
    [0xb9, 0x2f, 0x00, 0x60, 0x97, 0x97, 0xea, 0x5b],
);

type Method = *const c_void;

/// A COM object: a pointer to its table of methods
#[repr(C)]
struct Object {
    vtable: *const Method,
}

impl Object {
    /// The method at `index` of the object's table, as the given function type
    ///
    /// # Safety
    ///
    /// The object is alive and `F` is the method's signature.
    unsafe fn method<F: Copy>(this: *mut Object, index: usize) -> F {
        // SAFETY: The caller names a method of the interface with its signature
        unsafe { std::mem::transmute_copy(&*(*this).vtable.add(index)) }
    }

    /// # Safety
    ///
    /// The object is alive.
    unsafe fn release(this: *mut Object) {
        if !this.is_null() {
            // SAFETY: IUnknown::Release is the third method of every interface
            unsafe { Self::method::<unsafe extern "system" fn(*mut Object) -> u32>(this, 2)(this) };
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PixelFormat {
    size: u32,
    flags: u32,
    four_cc: u32,
    bits: u32,
    red: u32,
    green: u32,
    blue: u32,
    alpha: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SurfaceDesc2 {
    size: u32,
    flags: u32,
    height: u32,
    width: u32,
    pitch: i32,
    back_buffer_count: u32,
    mip_map_count: u32,
    alpha_bit_depth: u32,
    reserved: u32,
    surface: usize,
    color_keys: [u32; 8],
    pixel_format: PixelFormat,
    caps: [u32; 4],
    texture_stage: u32,
}
const _: () = assert!(size_of::<SurfaceDesc2>() == 124);

#[repr(C)]
#[derive(Clone, Copy)]
struct BltFx {
    size: u32,
    ddfx: u32,
    reserved: [u32; 18],
    fill_color: u32,
    dest_key: [u32; 2],
    src_key: [u32; 2],
}
const _: () = assert!(size_of::<BltFx>() == 100);

impl BltFx {
    fn new() -> Self {
        Self {
            size: size_of::<Self>() as u32,
            ddfx: 0,
            reserved: [0; 18],
            fill_color: 0,
            dest_key: [0; 2],
            src_key: [0; 2],
        }
    }
}

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "ddraw")]
unsafe extern "system" {
    fn DirectDrawCreateEx(
        guid: *const GUID,
        dd: *mut *mut Object,
        iid: *const GUID,
        outer: *mut c_void,
    ) -> i32;
}

// IDirectDraw7 and IDirectDrawSurface7 methods by their place in the table
type SetCooperativeLevel = unsafe extern "system" fn(*mut Object, HWND, u32) -> i32;
type SetDisplayMode = unsafe extern "system" fn(*mut Object, u32, u32, u32, u32, u32) -> i32;
type CreateSurface =
    unsafe extern "system" fn(*mut Object, *mut SurfaceDesc2, *mut *mut Object, *mut c_void) -> i32;
type CreatePalette =
    unsafe extern "system" fn(*mut Object, u32, *const u32, *mut *mut Object, *mut c_void) -> i32;
type GetCaps = unsafe extern "system" fn(*mut Object, *mut u32, *mut u32) -> i32;
type GetScanLine = unsafe extern "system" fn(*mut Object, *mut u32) -> i32;
type WaitForVerticalBlank = unsafe extern "system" fn(*mut Object, u32, HANDLE) -> i32;
type RestoreDisplayMode = unsafe extern "system" fn(*mut Object) -> i32;
type GetAttachedSurface =
    unsafe extern "system" fn(*mut Object, *mut [u32; 4], *mut *mut Object) -> i32;
type Blt = unsafe extern "system" fn(
    *mut Object,
    *mut Rect,
    *mut Object,
    *mut Rect,
    u32,
    *mut BltFx,
) -> i32;
type Flip = unsafe extern "system" fn(*mut Object, *mut Object, u32) -> i32;
type Lock =
    unsafe extern "system" fn(*mut Object, *mut Rect, *mut SurfaceDesc2, u32, HANDLE) -> i32;
type Unlock = unsafe extern "system" fn(*mut Object, *mut Rect) -> i32;
type SetPalette = unsafe extern "system" fn(*mut Object, *mut Object) -> i32;
const DD_CREATE_PALETTE: usize = 5;
const DD_CREATE_SURFACE: usize = 6;
const DD_GET_CAPS: usize = 11;
const DD_GET_SCAN_LINE: usize = 16;
const DD_RESTORE_DISPLAY_MODE: usize = 19;
const DD_SET_COOPERATIVE_LEVEL: usize = 20;
const DD_SET_DISPLAY_MODE: usize = 21;
const DD_WAIT_FOR_VERTICAL_BLANK: usize = 22;
const SURFACE_BLT: usize = 5;
const SURFACE_FLIP: usize = 11;
const SURFACE_GET_ATTACHED_SURFACE: usize = 12;
const SURFACE_LOCK: usize = 25;
const SURFACE_SET_PALETTE: usize = 31;
const SURFACE_UNLOCK: usize = 32;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 480;
/// DDCAPS of DirectX 7, in words
const CAPS_WORDS: usize = 95;

/// A locked surface's pixels, the bytes per pixel of the mode
struct Pixels {
    data: Vec<u8>,
    pitch: usize,
    bytes: usize,
}

impl Pixels {
    fn at(&self, x: usize, y: usize) -> u32 {
        let start = y * self.pitch + x * self.bytes;
        let mut value = [0; 4];
        value[..self.bytes].copy_from_slice(&self.data[start..start + self.bytes]);
        u32::from_le_bytes(value)
    }
}

struct Test {
    dd: *mut Object,
    primary: *mut Object,
    back: *mut Object,
    offscreen: *mut Object,
    bpp: u32,
}

impl Test {
    /// A distinct pixel value of the mode for a number
    fn color(&self, n: u32) -> u32 {
        match self.bpp {
            8 => n & 0xFF,
            16 => (n * 0x0841 + 0x1234) & 0xFFFF,
            _ => (n * 0x010203 + 0x304050) & 0xFF_FFFF,
        }
    }

    /// # Safety
    ///
    /// The surface is alive.
    unsafe fn lock(
        &self,
        surface: *mut Object,
        width: usize,
        height: usize,
    ) -> Result<Pixels, String> {
        let mut desc = SurfaceDesc2 {
            size: size_of::<SurfaceDesc2>() as u32,
            ..SurfaceDesc2::default()
        };
        // SAFETY: The surface is alive, the lock is released before returning
        unsafe {
            let result = Object::method::<Lock>(surface, SURFACE_LOCK)(
                surface,
                ptr::null_mut(),
                &mut desc,
                DDLOCK_WAIT,
                ptr::null_mut(),
            );
            if result != DD_OK {
                return Err(format!("Lock failed with {result:#x}"));
            }
            let bytes = (self.bpp / 8) as usize;
            let pitch = desc.pitch as usize;
            let mut data = vec![0; pitch * height];
            for y in 0..height {
                ptr::copy_nonoverlapping(
                    (desc.surface as *const u8).add(y * pitch),
                    data.as_mut_ptr().add(y * pitch),
                    width * bytes,
                );
            }
            Object::method::<Unlock>(surface, SURFACE_UNLOCK)(surface, ptr::null_mut());
            Ok(Pixels { data, pitch, bytes })
        }
    }

    /// # Safety
    ///
    /// The surface is alive.
    unsafe fn write(
        &self,
        surface: *mut Object,
        width: usize,
        height: usize,
        pixel: impl Fn(usize, usize) -> u32,
    ) -> Result<(), String> {
        let mut desc = SurfaceDesc2 {
            size: size_of::<SurfaceDesc2>() as u32,
            ..SurfaceDesc2::default()
        };
        // SAFETY: The surface is alive, the lock is released before returning
        unsafe {
            let result = Object::method::<Lock>(surface, SURFACE_LOCK)(
                surface,
                ptr::null_mut(),
                &mut desc,
                DDLOCK_WAIT,
                ptr::null_mut(),
            );
            if result != DD_OK {
                return Err(format!("Lock failed with {result:#x}"));
            }
            let bytes = (self.bpp / 8) as usize;
            for y in 0..height {
                for x in 0..width {
                    let value = pixel(x, y).to_le_bytes();
                    ptr::copy_nonoverlapping(
                        value.as_ptr(),
                        (desc.surface as *mut u8).add(y * desc.pitch as usize + x * bytes),
                        bytes,
                    );
                }
            }
            Object::method::<Unlock>(surface, SURFACE_UNLOCK)(surface, ptr::null_mut());
            Ok(())
        }
    }

    /// # Safety
    ///
    /// The surfaces are alive.
    unsafe fn blt(
        &self,
        dst: *mut Object,
        dst_rect: Option<Rect>,
        src: *mut Object,
        src_rect: Option<Rect>,
        flags: u32,
        fx: &mut BltFx,
    ) -> Result<(), String> {
        let rect = |rect: Option<Rect>| rect.map(Box::new);
        let (mut dst_rect, mut src_rect) = (rect(dst_rect), rect(src_rect));
        let pointer = |rect: &mut Option<Box<Rect>>| {
            rect.as_mut()
                .map_or(ptr::null_mut(), |rect| &mut **rect as *mut Rect)
        };
        // SAFETY: The surfaces are alive, the rectangles and effects outlive the call
        let result = unsafe {
            Object::method::<Blt>(dst, SURFACE_BLT)(
                dst,
                pointer(&mut dst_rect),
                src,
                pointer(&mut src_rect),
                flags | DDBLT_WAIT,
                fx,
            )
        };
        if result == DD_OK {
            Ok(())
        } else {
            Err(format!("Blt failed with {result:#x}"))
        }
    }
}

/// Checks a locked rectangle against the expected pixels
fn expect(
    pixels: &Pixels,
    width: usize,
    height: usize,
    expected: impl Fn(usize, usize) -> u32,
) -> Result<(), String> {
    for y in 0..height {
        for x in 0..width {
            let (got, want) = (pixels.at(x, y), expected(x, y));
            if got != want {
                return Err(format!("pixel {x},{y} is {got:#x}, expected {want:#x}"));
            }
        }
    }
    Ok(())
}

/// The surfaces of the mode: a primary with a back buffer and a 64x64 offscreen surface in VRAM
///
/// # Safety
///
/// The window is ours.
unsafe fn open(report: &mut Report, hwnd: HWND, bpp: u32) -> Result<Test, String> {
    // SAFETY: COM calls on objects created here, released by close
    unsafe {
        let mut dd = ptr::null_mut();
        if DirectDrawCreateEx(ptr::null(), &mut dd, &IID_IDIRECTDRAW7, ptr::null_mut()) != DD_OK {
            return Err("DirectDrawCreateEx failed".into());
        }
        let mut test = Test {
            dd,
            primary: ptr::null_mut(),
            back: ptr::null_mut(),
            offscreen: ptr::null_mut(),
            bpp,
        };
        let result = Object::method::<SetCooperativeLevel>(dd, DD_SET_COOPERATIVE_LEVEL)(
            dd,
            hwnd,
            DDSCL_EXCLUSIVE | DDSCL_FULLSCREEN,
        );
        if result != DD_OK {
            close(&test);
            return Err(format!("SetCooperativeLevel failed with {result:#x}"));
        }
        let result =
            Object::method::<SetDisplayMode>(dd, DD_SET_DISPLAY_MODE)(dd, WIDTH, HEIGHT, bpp, 0, 0);
        if result != DD_OK {
            close(&test);
            return Err(format!("SetDisplayMode failed with {result:#x}"));
        }
        let mut hal = [0_u32; CAPS_WORDS];
        let mut hel = [0_u32; CAPS_WORDS];
        hal[0] = (CAPS_WORDS * 4) as u32;
        hel[0] = hal[0];
        Object::method::<GetCaps>(dd, DD_GET_CAPS)(dd, hal.as_mut_ptr(), hel.as_mut_ptr());
        report.log(&format!(
            "{bpp} bpp: HAL caps {:#x}, color key caps {:#x}, effects {:#x}",
            hal[1], hal[3], hal[4]
        ));
        if hal[1] & DDCAPS_BLT == 0 {
            close(&test);
            return Err("the HAL has no blits".into());
        }

        let mut desc = SurfaceDesc2 {
            size: size_of::<SurfaceDesc2>() as u32,
            flags: DDSD_CAPS | DDSD_BACKBUFFERCOUNT,
            back_buffer_count: 1,
            caps: [
                DDSCAPS_PRIMARYSURFACE | DDSCAPS_FLIP | DDSCAPS_COMPLEX,
                0,
                0,
                0,
            ],
            ..SurfaceDesc2::default()
        };
        let result = Object::method::<CreateSurface>(dd, DD_CREATE_SURFACE)(
            dd,
            &mut desc,
            &mut test.primary,
            ptr::null_mut(),
        );
        if result != DD_OK {
            close(&test);
            return Err(format!("creating the flip chain failed with {result:#x}"));
        }
        let mut caps = [DDSCAPS_BACKBUFFER, 0, 0, 0];
        Object::method::<GetAttachedSurface>(test.primary, SURFACE_GET_ATTACHED_SURFACE)(
            test.primary,
            &mut caps,
            &mut test.back,
        );
        let mut desc = SurfaceDesc2 {
            size: size_of::<SurfaceDesc2>() as u32,
            flags: DDSD_CAPS | DDSD_WIDTH | DDSD_HEIGHT,
            width: 64,
            height: 64,
            caps: [DDSCAPS_OFFSCREENPLAIN | DDSCAPS_VIDEOMEMORY, 0, 0, 0],
            ..SurfaceDesc2::default()
        };
        let result = Object::method::<CreateSurface>(dd, DD_CREATE_SURFACE)(
            dd,
            &mut desc,
            &mut test.offscreen,
            ptr::null_mut(),
        );
        if result != DD_OK || test.back.is_null() {
            close(&test);
            return Err(format!(
                "creating the offscreen surface failed with {result:#x}"
            ));
        }
        Ok(test)
    }
}

/// # Safety
///
/// The objects are alive or null.
unsafe fn close(test: &Test) {
    // SAFETY: The objects belong to the test, the attached back buffer goes with its primary
    unsafe {
        Object::release(test.offscreen);
        Object::release(test.primary);
        Object::method::<RestoreDisplayMode>(test.dd, DD_RESTORE_DISPLAY_MODE)(test.dd);
        Object::release(test.dd);
    }
}

pub fn run(report: &mut Report, bpp: u32) {
    let hwnd = create_window(64, 64);
    settle();
    // SAFETY: The window is ours, the test closes what it opens
    let test = match unsafe { open(report, hwnd, bpp) } {
        Ok(test) => test,
        Err(error) => {
            report.check(&format!("{bpp} bpp DirectDraw setup"), Err(error));
            // SAFETY: The window is the test's
            unsafe { destroy_window(hwnd) };
            return;
        }
    };
    report.check(&format!("{bpp} bpp DirectDraw setup"), Ok(()));
    let name = |case: &str| format!("{bpp} bpp DirectDraw {case}");
    let (back, offscreen) = (test.back, test.offscreen);
    let source = |x: usize, y: usize| test.color((x * 7 + y * 13) as u32 % 200 + 1);

    // SAFETY: The surfaces are alive until close
    unsafe {
        let mut fx = BltFx::new();
        fx.fill_color = test.color(42);
        let result = test
            .blt(
                back,
                Some(Rect {
                    left: 10,
                    top: 20,
                    right: 110,
                    bottom: 70,
                }),
                ptr::null_mut(),
                None,
                DDBLT_COLORFILL,
                &mut fx,
            )
            .and_then(|()| {
                let pixels = test.lock(back, 110, 70)?;
                expect(&pixels, 110, 70, |x, y| {
                    if x >= 10 && y >= 20 {
                        test.color(42)
                    } else {
                        pixels.at(x, y)
                    }
                })
            });
        report.check(&name("color fill"), result);

        let result = test.write(offscreen, 64, 64, source).and_then(|()| {
            test.blt(
                back,
                Some(Rect {
                    left: 200,
                    top: 100,
                    right: 264,
                    bottom: 164,
                }),
                offscreen,
                None,
                0,
                &mut BltFx::new(),
            )?;
            let pixels = test.lock(back, 264, 164)?;
            expect(&pixels, 264, 164, |x, y| {
                if x >= 200 && y >= 100 {
                    source(x - 200, y - 100)
                } else {
                    pixels.at(x, y)
                }
            })
        });
        report.check(&name("copy from VRAM"), result);

        let key = source(0, 0);
        let mut fx = BltFx::new();
        fx.src_key = [key, key];
        let result = (|| {
            let mut fill = BltFx::new();
            fill.fill_color = test.color(5);
            test.blt(
                back,
                Some(Rect {
                    left: 0,
                    top: 200,
                    right: 64,
                    bottom: 264,
                }),
                ptr::null_mut(),
                None,
                DDBLT_COLORFILL,
                &mut fill,
            )?;
            test.blt(
                back,
                Some(Rect {
                    left: 0,
                    top: 200,
                    right: 64,
                    bottom: 264,
                }),
                offscreen,
                None,
                DDBLT_KEYSRCOVERRIDE,
                &mut fx,
            )?;
            let pixels = test.lock(back, 64, 264)?;
            for y in 0..64 {
                for x in 0..64 {
                    let want = if source(x, y) == key {
                        test.color(5)
                    } else {
                        source(x, y)
                    };
                    if pixels.at(x, 200 + y) != want {
                        return Err(format!(
                            "pixel {x},{y} is {:#x}, expected {want:#x}",
                            pixels.at(x, 200 + y)
                        ));
                    }
                }
            }
            Ok(())
        })();
        report.check(&name("source color key"), result);

        let result = (|| {
            let mut fill = BltFx::new();
            fill.fill_color = test.color(6);
            test.blt(
                back,
                Some(Rect {
                    left: 100,
                    top: 200,
                    right: 164,
                    bottom: 264,
                }),
                ptr::null_mut(),
                None,
                DDBLT_COLORFILL,
                &mut fill,
            )?;
            fill.fill_color = test.color(7);
            test.blt(
                back,
                Some(Rect {
                    left: 100,
                    top: 200,
                    right: 132,
                    bottom: 264,
                }),
                ptr::null_mut(),
                None,
                DDBLT_COLORFILL,
                &mut fill,
            )?;
            let mut fx = BltFx::new();
            fx.dest_key = [test.color(7), test.color(7)];
            test.blt(
                back,
                Some(Rect {
                    left: 100,
                    top: 200,
                    right: 164,
                    bottom: 264,
                }),
                offscreen,
                None,
                DDBLT_KEYDESTOVERRIDE,
                &mut fx,
            )?;
            let pixels = test.lock(back, 164, 264)?;
            for y in 0..64 {
                for x in 0..64 {
                    let want = if x < 32 { source(x, y) } else { test.color(6) };
                    if pixels.at(100 + x, 200 + y) != want {
                        return Err(format!(
                            "pixel {x},{y} is {:#x}, expected {want:#x}",
                            pixels.at(100 + x, 200 + y)
                        ));
                    }
                }
            }
            Ok(())
        })();
        report.check(&name("destination color key"), result);

        let result = (|| {
            test.blt(
                back,
                Some(Rect {
                    left: 300,
                    top: 200,
                    right: 428,
                    bottom: 328,
                }),
                offscreen,
                None,
                0,
                &mut BltFx::new(),
            )?;
            let mut fx = BltFx::new();
            fx.ddfx = DDBLTFX_MIRRORLEFTRIGHT | DDBLTFX_MIRRORUPDOWN;
            test.blt(
                back,
                Some(Rect {
                    left: 450,
                    top: 200,
                    right: 514,
                    bottom: 264,
                }),
                offscreen,
                None,
                DDBLT_DDFX,
                &mut fx,
            )?;
            let pixels = test.lock(back, 514, 328)?;
            for y in 0..128 {
                for x in 0..128 {
                    if pixels.at(300 + x, 200 + y) != source(x / 2, y / 2) {
                        return Err(format!(
                            "stretched pixel {x},{y} is {:#x}",
                            pixels.at(300 + x, 200 + y)
                        ));
                    }
                }
            }
            for y in 0..64 {
                for x in 0..64 {
                    if pixels.at(450 + x, 200 + y) != source(63 - x, 63 - y) {
                        return Err(format!(
                            "mirrored pixel {x},{y} is {:#x}",
                            pixels.at(450 + x, 200 + y)
                        ));
                    }
                }
            }
            Ok(())
        })();
        report.check(&name("stretch and mirror"), result);

        let result = (|| {
            test.blt(
                back,
                Some(Rect {
                    left: 0,
                    top: 400,
                    right: 64,
                    bottom: 464,
                }),
                offscreen,
                None,
                0,
                &mut BltFx::new(),
            )?;
            test.blt(
                back,
                Some(Rect {
                    left: 10,
                    top: 405,
                    right: 74,
                    bottom: 469,
                }),
                back,
                Some(Rect {
                    left: 0,
                    top: 400,
                    right: 64,
                    bottom: 464,
                }),
                0,
                &mut BltFx::new(),
            )?;
            let pixels = test.lock(back, 74, 469)?;
            for y in 0..64 {
                for x in 0..64 {
                    if pixels.at(10 + x, 405 + y) != source(x, y) {
                        return Err(format!(
                            "pixel {x},{y} is {:#x}",
                            pixels.at(10 + x, 405 + y)
                        ));
                    }
                }
            }
            Ok(())
        })();
        report.check(&name("overlapping copy"), result);

        let result = (|| {
            let dd = test.dd;
            if Object::method::<WaitForVerticalBlank>(dd, DD_WAIT_FOR_VERTICAL_BLANK)(
                dd,
                DDWAITVB_BLOCKBEGIN,
                ptr::null_mut(),
            ) != DD_OK
            {
                return Err("WaitForVerticalBlank failed".into());
            }
            let mut lines = Vec::new();
            for _ in 0..200 {
                let mut line = 0;
                match Object::method::<GetScanLine>(dd, DD_GET_SCAN_LINE)(dd, &mut line) {
                    DD_OK => lines.push(line),
                    DDERR_VERTICALBLANKINPROGRESS => {}
                    error => return Err(format!("GetScanLine failed with {error:#x}")),
                }
            }
            lines.dedup();
            if lines.len() < 2 || lines.iter().any(|&line| line >= HEIGHT) {
                return Err(format!("scanlines {lines:?}"));
            }
            Ok(())
        })();
        report.check(&name("vertical blank and scanline"), result);

        // The page checks what is shown: the back buffer's color after a flip, through the palette
        // in 8 bpp
        let mut palette = ptr::null_mut::<Object>();
        if bpp == 8 {
            let mut entries = [0_u32; 256];
            // PALETTEENTRY: red in the low byte, then green and blue
            entries[42] = 0x0000_80FF;
            let dd = test.dd;
            Object::method::<CreatePalette>(dd, DD_CREATE_PALETTE)(
                dd,
                DDPCAPS_8BIT | DDPCAPS_ALLOW256,
                entries.as_ptr(),
                &mut palette,
                ptr::null_mut(),
            );
            Object::method::<SetPalette>(test.primary, SURFACE_SET_PALETTE)(test.primary, palette);
        }
        let mut fill = BltFx::new();
        fill.fill_color = test.color(42);
        let result = test
            .blt(
                back,
                None,
                ptr::null_mut(),
                None,
                DDBLT_COLORFILL,
                &mut fill,
            )
            .and_then(|()| {
                let result = Object::method::<Flip>(test.primary, SURFACE_FLIP)(
                    test.primary,
                    ptr::null_mut(),
                    DDFLIP_WAIT,
                );
                if result != DD_OK {
                    return Err(format!("Flip failed with {result:#x}"));
                }
                let rgb = match bpp {
                    8 => 0xFF8000,
                    16 => {
                        let c = test.color(42);
                        let expand = |value: u32, bits: u32| {
                            (value << (8 - bits)) | (value >> (2 * bits - 8))
                        };
                        expand(c >> 11, 5) << 16 | expand((c >> 5) & 63, 6) << 8 | expand(c & 31, 5)
                    }
                    _ => test.color(42),
                };
                report.line(&format!("CHECK-SCREEN 320 240 {rgb:06x} {bpp} bpp flip"));
                Ok(())
            });
        report.check(&name("flip"), result);
        Object::release(palette);
        close(&test);
        destroy_window(hwnd);
    }
    settle();
}
