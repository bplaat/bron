/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![no_std]
#![no_main]

use core::ffi::{c_int, c_void};
use core::ptr;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};

use libogc_sys::*;

const FIFO_SIZE: usize = 256 * 1024;
const WPAD_CHANNEL_COUNT: c_int = 4;
const CLEAR_COLOR: GXColor = GXColor {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};
// The power button of the Wii and of the Wii Remotes, libogc calls it from an interrupt
static POWER_PRESSED: AtomicBool = AtomicBool::new(false);

struct HeapBuffer(NonNull<c_void>);

impl HeapBuffer {
    fn from_raw(pointer: *mut c_void) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }

    fn new_zeroed(size: usize) -> Option<Self> {
        let buffer = Self::from_raw(unsafe { memalign(32, size) })?;
        unsafe { ptr::write_bytes(buffer.as_ptr(), 0, size) };
        Some(buffer)
    }

    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for HeapBuffer {
    fn drop(&mut self) {
        unsafe { free(self.0.as_ptr()) }
    }
}

struct Framebuffer(HeapBuffer);

impl Framebuffer {
    fn new(mode: &GXRModeObj) -> Option<Self> {
        HeapBuffer::from_raw(unsafe { SYS_AllocateFramebuffer(mode) }).map(Self)
    }

    fn scanout(&self) -> *mut c_void {
        (self.0.as_ptr() as u32).wrapping_add(SYS_BASE_UNCACHED.wrapping_sub(SYS_BASE_CACHED))
            as *mut c_void
    }
}

struct FrameState {
    buffers: [Framebuffer; 2],
    current: usize,
    first_frame: bool,
}

impl FrameState {
    fn new(buffers: [Framebuffer; 2]) -> Self {
        Self {
            buffers,
            current: 0,
            first_frame: true,
        }
    }

    fn scanout(&self) -> *mut c_void {
        self.buffers[self.current].scanout()
    }

    unsafe fn present(&mut self) {
        unsafe {
            self.current ^= 1;
            GX_CopyDisp(self.scanout(), GX_TRUE);
            // The copy follows the drawing, so this waits for both before the frame is shown
            GX_DrawDone();
            VIDEO_SetNextFramebuffer(self.scanout());
            if self.first_frame {
                VIDEO_SetBlack(false);
                self.first_frame = false;
            }
            VIDEO_Flush();
            VIDEO_WaitVSync();
        }
    }
}

// Face coordinates and UVs match the WebGPU cube mesh.
const CORNERS: [[[f32; 3]; 4]; 6] = [
    [
        [-1.0, -1.0, 1.0],
        [1.0, -1.0, 1.0],
        [1.0, 1.0, 1.0],
        [-1.0, 1.0, 1.0],
    ],
    [
        [1.0, -1.0, -1.0],
        [-1.0, -1.0, -1.0],
        [-1.0, 1.0, -1.0],
        [1.0, 1.0, -1.0],
    ],
    [
        [1.0, -1.0, 1.0],
        [1.0, -1.0, -1.0],
        [1.0, 1.0, -1.0],
        [1.0, 1.0, 1.0],
    ],
    [
        [-1.0, -1.0, -1.0],
        [-1.0, -1.0, 1.0],
        [-1.0, 1.0, 1.0],
        [-1.0, 1.0, -1.0],
    ],
    [
        [-1.0, 1.0, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 1.0, -1.0],
        [-1.0, 1.0, -1.0],
    ],
    [
        [-1.0, -1.0, -1.0],
        [1.0, -1.0, -1.0],
        [1.0, -1.0, 1.0],
        [-1.0, -1.0, 1.0],
    ],
];
const NORMALS: [[f32; 3]; 6] = [
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
];

include!(concat!(env!("OUT_DIR"), "/texture.rs"));

#[repr(align(32))]
struct TextureData([u8; include_bytes!(concat!(env!("OUT_DIR"), "/cube.bin")).len()]);
static TEXTURE: TextureData = TextureData(*include_bytes!(concat!(env!("OUT_DIR"), "/cube.bin")));

unsafe fn draw_cube(elapsed: f32) {
    unsafe {
        let mut matrix = [[0.0; 4]; 3];
        let mut temp = [[0.0; 4]; 3];
        ps_guMtxRotRad(&mut matrix, b'x' as _, elapsed * 0.55);
        for (axis, angle) in [(b'z', elapsed * 0.35), (b'y', 0.4), (b'x', 0.25)] {
            ps_guMtxRotRad(&mut temp, axis as _, angle);
            ps_guMtxConcat(&temp, &matrix, &mut matrix);
        }
        matrix[2][3] = -4.5;
        GX_LoadPosMtxImm(&matrix, GX_PNMTX0);
        GX_Begin(GX_QUADS, GX_VTXFMT0, 24);
        for (corners, normal) in CORNERS.iter().zip(NORMALS) {
            let mut dot = 0.0;
            for (row, light) in [0.4, 0.8, 0.6].into_iter().enumerate() {
                for (col, normal) in normal.into_iter().enumerate() {
                    dot += matrix[row][col] * normal * light;
                }
            }
            let shade = ((0.35 + 0.65 * dot.abs() / sqrtf(1.16)) * 255.0) as u32;
            for (index, corner) in corners.iter().enumerate() {
                for value in corner {
                    ptr::write_volatile(wgPipe.cast::<f32>(), *value);
                }
                ptr::write_volatile(
                    wgPipe.cast::<u32>(),
                    shade << 24 | shade << 16 | shade << 8 | 255,
                );
                ptr::write_volatile(
                    wgPipe.cast::<f32>(),
                    if index == 1 || index == 2 { 1.0 } else { 0.0 },
                );
                ptr::write_volatile(wgPipe.cast::<f32>(), if index < 2 { 1.0 } else { 0.0 });
            }
        }
    }
}

extern "C" fn power() {
    POWER_PRESSED.store(true, Ordering::Relaxed);
}

fn home_pressed() -> bool {
    (0..WPAD_CHANNEL_COUNT)
        .any(|channel| unsafe { WPAD_ButtonsDown(channel) } & WPAD_BUTTON_HOME != 0)
}

#[unsafe(export_name = "main")]
pub extern "C" fn main() -> c_int {
    unsafe {
        VIDEO_Init();
        VIDEO_SetBlack(true);
        let mode = VIDEO_GetPreferredMode(ptr::null_mut());
        if mode.is_null() {
            return 1;
        }
        let mode_ref = &*mode;
        let Some(framebuffer0) = Framebuffer::new(mode_ref) else {
            return 1;
        };
        let Some(framebuffer1) = Framebuffer::new(mode_ref) else {
            return 1;
        };
        let Some(fifo) = HeapBuffer::new_zeroed(FIFO_SIZE) else {
            return 1;
        };
        let mut display = FrameState::new([framebuffer0, framebuffer1]);
        let aspect = if CONF_GetAspectRatio() == CONF_ASPECT_16_9 {
            16.0 / 9.0
        } else {
            4.0 / 3.0
        };

        VIDEO_Configure(mode);
        VIDEO_SetNextFramebuffer(display.scanout());
        VIDEO_Flush();
        VIDEO_WaitVSync();
        if mode_ref.vi_tv_mode & VI_NON_INTERLACE != 0 {
            VIDEO_WaitVSync();
        }

        GX_Init(fifo.as_ptr(), FIFO_SIZE as u32);
        GX_SetViewport(
            0.0,
            0.0,
            mode_ref.fb_width as f32,
            mode_ref.efb_height as f32,
            0.0,
            1.0,
        );
        GX_SetScissor(0, 0, mode_ref.fb_width as u32, mode_ref.efb_height as u32);
        let yscale = GX_GetYScaleFactor(mode_ref.efb_height, mode_ref.xfb_height);
        let xfb_height = GX_SetDispCopyYScale(yscale);
        GX_SetDispCopySrc(0, 0, mode_ref.fb_width, mode_ref.efb_height);
        GX_SetDispCopyDst(mode_ref.fb_width, xfb_height as u16);
        GX_SetCopyFilter(
            mode_ref.aa,
            mode_ref.sample_pattern.as_ptr(),
            GX_TRUE,
            mode_ref.vfilter.as_ptr(),
        );
        GX_SetFieldMode(
            mode_ref.field_rendering,
            if mode_ref.vi_height == 2 * mode_ref.xfb_height {
                GX_ENABLE
            } else {
                GX_DISABLE
            },
        );
        GX_SetPixelFmt(
            if mode_ref.aa != 0 {
                GX_PF_RGB565_Z16
            } else {
                GX_PF_RGB8_Z24
            },
            GX_ZC_LINEAR,
        );
        GX_SetCopyClear(CLEAR_COLOR, GX_MAX_Z24);
        GX_CopyDisp(display.scanout(), GX_TRUE);

        WPAD_Init();
        SYS_SetPowerCallback(Some(power));

        GX_ClearVtxDesc();
        GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
        GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS as u32, GX_POS_XYZ, GX_F32, 0);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
        GX_SetNumChans(1);
        GX_SetChanCtrl(
            GX_COLOR0A0 as i32,
            GX_DISABLE,
            GX_SRC_VTX,
            GX_SRC_VTX,
            0,
            GX_DF_NONE,
            GX_AF_NONE,
        );
        GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0 as u32, GX_TEX_ST, GX_F32, 0);
        GX_SetNumTexGens(1);
        GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2X4, GX_TG_TEX0, GX_IDENTITY);
        let mut texture = GXTexObj { val: [0; 8] };
        DCFlushRange(TEXTURE.0.as_ptr().cast_mut().cast(), TEXTURE.0.len() as u32);
        GX_InitTexObj(
            &mut texture,
            TEXTURE.0.as_ptr().cast_mut().cast(),
            TEXTURE_WIDTH,
            TEXTURE_HEIGHT,
            GX_TF_RGBA8,
            GX_REPEAT,
            GX_REPEAT,
            GX_FALSE,
        );
        GX_InitTexObjLOD(
            &mut texture,
            GX_LINEAR,
            GX_LINEAR,
            0.0,
            0.0,
            0.0,
            GX_FALSE,
            GX_FALSE,
            GX_ANISO_1,
        );
        GX_LoadTexObj(&texture, GX_TEXMAP0 as u8);
        GX_SetZMode(GX_TRUE, GX_LEQUAL, GX_TRUE);
        GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0 as u8, GX_TEXMAP0, GX_COLOR0A0);
        GX_SetTevOp(GX_TEVSTAGE0, GX_MODULATE);
        GX_SetCullMode(GX_CULL_NONE);
        let mut projection = [[0.0; 4]; 4];
        guPerspective(&mut projection, 45.0, aspect, 0.1, 100.0);
        GX_LoadProjectionMtx(&projection, GX_PERSPECTIVE);
        let started = gettime();
        while !POWER_PRESSED.load(Ordering::Relaxed) {
            WPAD_ScanPads();
            if home_pressed() {
                break;
            }
            draw_cube((gettime() - started) as f32 / 60_750_000.0);
            display.present();
        }

        // HOME returns to the loader, a power button turns the Wii off
        if POWER_PRESSED.load(Ordering::Relaxed) {
            SYS_ResetSystem(SYS_POWEROFF, 0, 0);
        }
    }
    0
}
