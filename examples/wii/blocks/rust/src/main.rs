/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![no_std]
#![no_main]

use core::f64::consts::TAU;
use core::ffi::{c_int, c_void};
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};

use libogc_sys::*;

use crate::materials::Materials;
use crate::memory::{AlignedBuffer, Framebuffer};
use crate::mesh::Mesh;
use crate::world::World;

mod materials;
mod memory;
mod mesh;
mod world;

const FIFO_SIZE: usize = 256 * 1024;
const CLEAR_COLOR: GXColor = GXColor {
    r: 46,
    g: 84,
    b: 158,
    a: 255,
};
// The power button of the Wii and of the Wii Remotes, libogc calls it from an interrupt
static POWER_PRESSED: AtomicBool = AtomicBool::new(false);

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

    fn present(&mut self) {
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

struct Demo {
    display: FrameState,
    _fifo: AlignedBuffer,
    materials: Materials,
    mesh: Mesh,
    started: u64,
}

impl Demo {
    fn new() -> Option<Self> {
        unsafe {
            VIDEO_Init();
            VIDEO_SetBlack(true);
            let mode = VIDEO_GetPreferredMode(ptr::null_mut());
            let mode_ref = mode.as_ref()?;
            let first = Framebuffer::new(mode_ref)?;
            let second = Framebuffer::new(mode_ref)?;
            let display = FrameState::new([first, second]);
            let mut fifo = AlignedBuffer::new(FIFO_SIZE)?;
            fifo.zero();
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
            let scale = GX_GetYScaleFactor(mode_ref.efb_height, mode_ref.xfb_height);
            let xfb_height = GX_SetDispCopyYScale(scale);
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

            let materials = Materials::load();
            configure_pipeline();
            let world = World::generate(gettime() as u32);
            let mesh = Mesh::build(&world.faces_by_material())?;
            let mut projection = [[0.0; 4]; 4];
            guPerspective(&mut projection, 50.0, aspect, 1.0, 500.0);
            GX_LoadProjectionMtx(&projection, GX_PERSPECTIVE);
            GX_SetFog(GX_FOG_LIN, 120.0, 300.0, 1.0, 500.0, CLEAR_COLOR);

            Some(Self {
                display,
                _fifo: fifo,
                materials,
                mesh,
                started: gettime(),
            })
        }
    }

    fn run(&mut self) {
        unsafe {
            while !POWER_PRESSED.load(Ordering::Relaxed) {
                WPAD_ScanPads();
                if WPAD_ButtonsDown(0) & WPAD_BUTTON_HOME != 0 {
                    break;
                }
                // Wii time-base ticks run at one quarter of the 243 MHz bus clock.
                let seconds = (gettime() - self.started) as f64 / 60_750_000.0;
                let angle = ((seconds * 0.16) % TAU) as f32;
                let (sin, cos) = (sinf(angle), cosf(angle));
                let eye = GuVector {
                    x: 32.0 + sin * 88.0,
                    y: 21.76 + 58.0,
                    z: 32.0 + cos * 88.0,
                };
                let up = GuVector {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                };
                let target = GuVector {
                    x: 32.0,
                    y: 21.76,
                    z: 32.0,
                };
                let mut view = [[0.0; 4]; 3];
                guLookAt(&mut view, &eye, &up, &target);
                GX_LoadPosMtxImm(&view, GX_PNMTX0);
                self.mesh.draw(&self.materials);
                self.display.present();
            }
        }
    }
}

fn configure_pipeline() {
    unsafe {
        GX_ClearVtxDesc();
        GX_SetVtxDesc(GX_VA_POS, GX_DIRECT);
        GX_SetVtxDesc(GX_VA_CLR0, GX_DIRECT);
        GX_SetVtxDesc(GX_VA_TEX0, GX_DIRECT);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_POS as u32, GX_POS_XYZ, GX_U8, 0);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_CLR0 as u32, GX_CLR_RGBA, GX_RGBA8, 0);
        GX_SetVtxAttrFmt(GX_VTXFMT0, GX_VA_TEX0 as u32, GX_TEX_ST, GX_U8, 0);
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
        GX_SetNumTexGens(1);
        GX_SetTexCoordGen(GX_TEXCOORD0, GX_TG_MTX2X4, GX_TG_TEX0, GX_IDENTITY);
        GX_SetTevOrder(GX_TEVSTAGE0, GX_TEXCOORD0 as u8, GX_TEXMAP0, GX_COLOR0A0);
        GX_SetTevOp(GX_TEVSTAGE0, GX_MODULATE);
        GX_SetCullMode(GX_CULL_NONE);
    }
}

extern "C" fn power() {
    POWER_PRESSED.store(true, Ordering::Relaxed);
}

#[unsafe(export_name = "main")]
pub extern "C" fn app_main() -> c_int {
    let Some(mut demo) = Demo::new() else {
        return 1;
    };
    demo.run();
    // HOME returns to the loader, a power button turns the Wii off
    if POWER_PRESSED.load(Ordering::Relaxed) {
        unsafe { SYS_ResetSystem(SYS_POWEROFF, 0, 0) };
    }
    0
}
