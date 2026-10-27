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
    r: 128,
    g: 128,
    b: 128,
    a: 255,
};
// The power button of the Wii and of the Wii Remotes, libogc calls it from an interrupt
static POWER_PRESSED: AtomicBool = AtomicBool::new(false);

struct HeapBuffer(NonNull<c_void>);

impl HeapBuffer {
    fn from_raw(pointer: *mut c_void) -> Option<Self> {
        NonNull::new(pointer).map(Self)
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

struct AlignedBuffer(HeapBuffer);

impl AlignedBuffer {
    fn new_zeroed(size: usize) -> Option<Self> {
        if size == 0 || !size.is_multiple_of(32) || size > u32::MAX as usize {
            return None;
        }
        let buffer = HeapBuffer::from_raw(unsafe { memalign(32, size) })?;
        unsafe { ptr::write_bytes(buffer.as_ptr(), 0, size) };
        Some(Self(buffer))
    }

    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
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
            GX_DrawDone();
            self.current ^= 1;
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_ENABLE);
            GX_CopyDisp(self.scanout(), GX_ENABLE);
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

struct TriangleScene {
    rotation: f32,
    list: PreparedDisplayList,
}

impl TriangleScene {
    unsafe fn new(aspect: f32, list: PreparedDisplayList) -> Self {
        let mut projection = [[0.0; 4]; 4];
        unsafe { guPerspective(&mut projection, 45.0, aspect, 0.1, 1000.0) };
        unsafe { GX_LoadProjectionMtx(&projection, GX_PERSPECTIVE) };
        Self {
            rotation: 0.0,
            list,
        }
    }

    unsafe fn draw(&mut self) {
        unsafe {
            self.rotation += 1.0;
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_ENABLE);
            for y in -2..2 {
                for x in -2..2 {
                    let mut matrix = [[0.0; 4]; 3];
                    let mut temp = [[0.0; 4]; 3];
                    let radians = self.rotation * (core::f32::consts::PI / 180.0);
                    ps_guMtxRotRad(&mut matrix, b'x' as core::ffi::c_char, radians);
                    ps_guMtxRotRad(&mut temp, b'y' as core::ffi::c_char, radians);
                    ps_guMtxConcat(&matrix, &temp, &mut matrix);
                    ps_guMtxTransApply(
                        &matrix,
                        &mut matrix,
                        (x * 2 + 1) as f32,
                        (y * 2 + 1) as f32,
                        -10.0,
                    );
                    GX_LoadPosMtxImm(&matrix, GX_PNMTX0);
                    self.list.draw();
                }
            }
        }
    }
}

#[repr(C, align(32))]
struct DisplayList([u8; 32]);

struct PreparedDisplayList(&'static DisplayList);

impl DisplayList {
    fn prepare(&'static self) -> PreparedDisplayList {
        unsafe { DCFlushRange(ptr::from_ref(self).cast_mut().cast(), 32) };
        PreparedDisplayList(self)
    }
}

impl PreparedDisplayList {
    unsafe fn draw(&self) {
        unsafe { GX_CallDispList(ptr::from_ref(self.0).cast(), 32) }
    }
}

#[rustfmt::skip]
static TRIANGLE_LIST: DisplayList = DisplayList([
    GX_TRIANGLES | GX_VTXFMT0, 0, 3,
    0, 1,       255, 0, 0, 255,
    255, 255,   0, 255, 0, 255,
    1, 255,     0, 0, 255, 255,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
]);

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
        let Some(fifo) = AlignedBuffer::new_zeroed(FIFO_SIZE) else {
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
            1,
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
        GX_SetDispCopyGamma(GX_GM_1_0);
        GX_SetCopyClear(CLEAR_COLOR, GX_MAX_Z24);
        GX_SetZMode(GX_ENABLE, GX_ALWAYS, GX_ENABLE);
        GX_CopyDisp(display.scanout(), GX_ENABLE);

        WPAD_Init();
        SYS_SetPowerCallback(Some(power));

        GX_ClearVtxDesc();
        GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
        GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
        GX_SetVtxAttrFmt(0, GX_VA_POS as u32, GX_POS_XY, GX_S8, 0);
        GX_SetVtxAttrFmt(0, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
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
        GX_SetNumTexGens(0);
        GX_SetTevOrder(0, GX_TEXCOORDNULL, GX_TEXMAP_NULL, GX_COLOR0A0);
        GX_SetTevOp(0, GX_PASSCLR);
        GX_SetCullMode(GX_CULL_NONE);
        let mut scene = TriangleScene::new(aspect, TRIANGLE_LIST.prepare());
        while !POWER_PRESSED.load(Ordering::Relaxed) {
            WPAD_ScanPads();
            if home_pressed() {
                break;
            }
            scene.draw();
            display.present();
        }

        // HOME returns to the loader, a power button turns the Wii off
        if POWER_PRESSED.load(Ordering::Relaxed) {
            SYS_ResetSystem(SYS_POWEROFF, 0, 0);
        }
    }
    0
}
