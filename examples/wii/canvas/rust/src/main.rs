/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![no_std]
#![no_main]
#![allow(unsafe_op_in_unsafe_fn)]

use alloc::format;
use core::ffi::{c_int, c_void};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::libogc::*;
use crate::memory::{AlignedBuffer, Framebuffer};
use crate::render::*;
use crate::texture::Texture;

mod font;
mod libogc;
mod memory;
mod render;
mod texture;

const FIFO_SIZE: usize = 256 * 1024;
const CLEAR_COLOR: GXColor = GXColor {
    r: 128,
    g: 128,
    b: 128,
    a: 255,
};
static RUNNING: AtomicBool = AtomicBool::new(true);

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
            GX_SetZMode(GX_ENABLE, GX_LEQUAL, GX_TRUE);
            GX_CopyDisp(self.scanout(), GX_TRUE);
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

struct Assets {
    font: Texture,
    dirt_grass: Texture,
    stone_coal: Texture,
    cursors: [Texture; 4],
}

impl Assets {
    fn load() -> Option<Self> {
        Some(Self {
            font: Texture::from_png(include_bytes!("assets/font.png")).ok()?,
            dirt_grass: Texture::from_png(include_bytes!("assets/dirt_grass.png")).ok()?,
            stone_coal: Texture::from_png(include_bytes!("assets/stone_coal.png")).ok()?,
            cursors: [
                Texture::from_png(include_bytes!("assets/cursor1.png")).ok()?,
                Texture::from_png(include_bytes!("assets/cursor2.png")).ok()?,
                Texture::from_png(include_bytes!("assets/cursor3.png")).ok()?,
                Texture::from_png(include_bytes!("assets/cursor4.png")).ok()?,
            ],
        })
    }
}

struct CanvasScene {
    assets: Assets,
    perspective: Mtx44,
    orthographic: Mtx44,
    screen_width: u32,
    screen_height: u32,
    framebuffer_width: u16,
    framebuffer_height: u16,
    rotation: f32,
}

impl CanvasScene {
    unsafe fn new(
        aspect: f32,
        mode: &GXRModeObj,
        screen_width: u32,
        screen_height: u32,
    ) -> Option<Self> {
        let assets = Assets::load()?;
        let mut perspective = [[0.0; 4]; 4];
        unsafe { guPerspective(&mut perspective, 45.0, aspect, 0.1, 1000.0) };
        let mut orthographic = [[0.0; 4]; 4];
        unsafe {
            guOrtho(
                &mut orthographic,
                0.0,
                screen_height as f32,
                0.0,
                screen_width as f32,
                -1.0,
                1.0,
            )
        };
        Some(Self {
            assets,
            perspective,
            orthographic,
            screen_width,
            screen_height,
            framebuffer_width: mode.fb_width,
            framebuffer_height: mode.xfb_height,
            rotation: 0.0,
        })
    }

    unsafe fn draw(&mut self, cursor: &IrData) {
        unsafe {
            self.rotation += 1.0;
            draw_cube(&self.assets.stone_coal, self.rotation, &self.perspective);
            GX_LoadProjectionMtx(&self.orthographic, GX_ORTHOGRAPHIC);
            GX_SetZMode(GX_DISABLE, GX_LEQUAL, GX_TRUE);
            GX_SetCullMode(GX_CULL_NONE);
            GX_SetBlendMode(GX_BM_BLEND, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
            GX_ClearVtxDesc();
            GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
            GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS as u32, GX_POS_XY, GX_F32, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
            GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0 as u32, GX_TEX_ST, GX_F32, 0);
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
            GX_SetCurrentMtx(GX_PNMTX0);
            GX_SetNumTexGens(1);
            GX_SetNumTevStages(1);
            GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2X4, GX_TG_TEX0, GX_IDENTITY);
            GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0 as u8, GX_TEXMAP0, GX_COLOR0A0);
            GX_SetTevOp(GX_TEVSTAGE0, GX_MODULATE);

            let mut transform = [[0.0; 4]; 3];
            rotate_degrees(&mut transform, b'z' as core::ffi::c_char, self.rotation);
            GX_LoadPosMtxImm(&transform, GX_PNMTX0);
            for (x, y, color) in [
                (50.0, 100.0, 0xffff_ffff),
                (100.0, 150.0, 0xff00_00ff),
                (150.0, 200.0, 0x00ff_00ff),
                (200.0, 250.0, 0x0000_ffff),
            ] {
                draw_image(
                    &self.assets.dirt_grass,
                    &transform,
                    x,
                    y,
                    100.0,
                    100.0,
                    color,
                );
            }

            let mut identity = [[0.0; 4]; 3];
            ps_guMtxIdentity(&mut identity);
            GX_LoadPosMtxImm(&identity, GX_PNMTX0);
            draw_text(
                &self.assets.font,
                &identity,
                "Hello Wii 🏠!",
                8.0,
                8.0,
                64.0,
                0xffff_ffff,
            );
            draw_text(
                &self.assets.font,
                &identity,
                "The quick brown fox jumps over the lazy dog.",
                8.0,
                80.0,
                24.0,
                0xff00_00ff,
            );
            let debug = format!(
                "framebuffer={}x{} viewport={}x{}",
                self.framebuffer_width,
                self.framebuffer_height,
                self.screen_width,
                self.screen_height
            );
            draw_text(
                &self.assets.font,
                &identity,
                &debug,
                8.0,
                112.0,
                24.0,
                0xffff_ffff,
            );

            let mut cursor_matrix = [[0.0; 4]; 3];
            rotate_degrees(&mut cursor_matrix, b'z' as core::ffi::c_char, cursor.angle);
            draw_image(
                &self.assets.cursors[0],
                &cursor_matrix,
                cursor.x - 48.0,
                cursor.y - 48.0,
                96.0,
                96.0,
                0xffff_ffff,
            );
            GX_SetBlendMode(GX_BM_NONE, GX_BL_SRCALPHA, GX_BL_INVSRCALPHA, GX_LO_CLEAR);
            GX_Flush();
        }
    }
}

extern "C" fn poweroff() {
    RUNNING.store(false, Ordering::Relaxed);
}

extern "C" fn wpad_poweroff(channel: c_int) {
    if channel == WPAD_CHAN_ALL {
        RUNNING.store(false, Ordering::Relaxed);
    }
}

#[unsafe(export_name = "main")]
pub extern "C" fn app_main() -> c_int {
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

        GX_Init(fifo.as_ptr(), fifo.size());
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
        GX_SetDispCopyGamma(GX_GM_1_0);
        GX_SetCopyClear(CLEAR_COLOR, GX_MAX_Z24);

        WPAD_Init();
        SYS_SetPowerCallback(Some(poweroff));
        WPAD_SetPowerButtonCallback(Some(wpad_poweroff));
        GX_SetZMode(GX_ENABLE, GX_ALWAYS, GX_TRUE);
        GX_CopyDisp(display.scanout(), GX_TRUE);

        let screen_height = mode_ref.vi_height as u32;
        let screen_width = (screen_height as f32 * aspect) as u32;
        WPAD_SetDataFormat(WPAD_CHAN_ALL, WPAD_FMT_BTNS_ACC_IR);
        WPAD_SetVRes(0, screen_width, screen_height);

        let Some(mut scene) = CanvasScene::new(aspect, mode_ref, screen_width, screen_height)
        else {
            return 1;
        };
        while RUNNING.load(Ordering::Relaxed) {
            WPAD_ScanPads();
            let mut ir = core::mem::MaybeUninit::<IrData>::zeroed();
            WPAD_IR(0, ir.as_mut_ptr());
            let ir = ir.assume_init();
            if WPAD_ButtonsDown(0) & WPAD_BUTTON_HOME != 0 {
                RUNNING.store(false, Ordering::Relaxed);
            }

            scene.draw(&ir);

            display.present();
        }
        WPAD_Disconnect(WPAD_CHAN_ALL);
    }
    0
}
