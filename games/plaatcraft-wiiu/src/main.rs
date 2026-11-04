/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! PlaatCraft for the Wii U, the plaatcraft-engine client drawn with GX2 on the TV and the
//! GamePad, played with the GamePad or a Wii Remote and Nunchuk.

#![no_std]
#![no_main]
#![cfg_attr(feature = "bench", allow(dead_code, unused_imports))]

use alloc::boxed::Box;
use alloc::string::String;
use core::ffi::c_int;

use plaatcraft_engine::client::{Game, Gpu, Platform, Thread};
use plaatcraft_engine::menu::{Keyboard, Platform as Menu};
use plaatcraft_engine::server::Limits;
use wut_sys::*;

use crate::audio::Ax;
use crate::gx2::Gx2;
use crate::input::Controllers;
use crate::system::{Condvar, Mutex, WiiUSocket};

mod audio;
mod gx2;
mod input;
mod memory;
mod system;

struct WiiU;

impl Platform for WiiU {
    type Lock = Mutex;
    type Signal = Condvar;
    type Socket = WiiUSocket;
    type Gpu = Gx2;
    type Audio = Ax;
    type Controller = Controllers;

    const MAX_LOCAL_PLAYERS: usize = 5;
    const NAME: &'static str = "Wii U";
    // The render distance is fixed
    const MENU: Menu = Menu {
        keyboard: Keyboard::OnScreen,
        quits: true,
        distance: false,
        confirm: "+",
        open: "Press A on a world",
        controls: &[
            "+ pause - Sticks walk and look - ZL break - ZR place - A jump - B sneak",
            "L/R or D-pad slot - X blocks - Y pick - Minus tools - Wii Remote works too",
        ],
    };
    // The worker builds chunks on its own core, so more fluid flows than on the Wii
    const LIMITS: Limits = Limits {
        fluid_radius: 3,
        fluid_updates: 256,
    };
    const RENDER_DISTANCE: i64 = 12;
    const MAX_GUESTS: usize = 7;
    const INVENTORY_HELP: &'static str = "A assign - L/R slot - B close";

    fn spawn(thread: Thread, entry: Box<dyn FnOnce() + Send>) {
        system::spawn(thread, entry);
    }

    fn seconds() -> f64 {
        system::seconds()
    }

    fn random() -> u64 {
        let mut random = [0u8; 8];
        if unsafe { getentropy(random.as_mut_ptr().cast(), random.len()) } != 0 {
            return (unsafe { OSGetTime() } as u64).wrapping_mul(6_364_136_223_846_793_005);
        }
        u64::from_ne_bytes(random)
    }

    fn idle() {
        system::sleep_frame();
    }

    fn connect_network() -> Result<[u8; 4], i32> {
        system::connect()
    }

    // wutpicolibc mounts the SD card on first use
    fn mount_storage() -> bool {
        true
    }

    fn nickname() -> Option<String> {
        None
    }

    fn running() -> bool {
        unsafe { WHBProcIsRunning() != FALSE }
    }
}

#[cfg(feature = "bench")]
#[unsafe(export_name = "main")]
pub extern "C" fn bench_main() -> c_int {
    let line = alloc::format!("{}\n\0", plaatcraft_engine::client::bench::<WiiU>());
    unsafe { printf(c"%s".as_ptr(), line.as_ptr()) };
    0
}

#[cfg(not(feature = "bench"))]
#[unsafe(export_name = "main")]
pub extern "C" fn app_main() -> c_int {
    unsafe { WHBProcInit() };
    let Some(gx2) = Gx2::new() else {
        unsafe { WHBProcShutdown() };
        return 1;
    };
    let controllers = Controllers::new(gx2.size());
    let mut game = Game::<WiiU>::new(gx2, Ax::new(), controllers);
    let quit = game.run();
    // ProcUI's foreground-release callbacks still need GX2 during the exit transition.
    // A system-requested exit has already stopped ProcUI in the game's running check.
    if quit {
        unsafe {
            SYSLaunchMenu();
            while WHBProcIsRunning() != FALSE {}
        }
    }
    drop(game);
    unsafe { WHBProcShutdown() };
    0
}
