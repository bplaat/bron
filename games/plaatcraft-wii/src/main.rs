/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! PlaatCraft for the Wii, the plaatcraft-engine client drawn with GX and played with a
//! Wii Remote and Nunchuk.

#![no_std]
#![no_main]
#![cfg_attr(feature = "bench", allow(dead_code, unused_imports))]

use alloc::boxed::Box;
use alloc::string::String;
use core::ffi::c_int;
use core::sync::atomic::{AtomicBool, Ordering};

use libogc_sys::*;
use plaatcraft_engine::client::{Game, Gpu, Platform, Thread};
use plaatcraft_engine::menu::{Keyboard, Platform as Menu};
use plaatcraft_engine::server::Limits;

use crate::audio::Aesnd;
use crate::gx::Gx;
use crate::input::Remote;
use crate::system::{Condvar, Mutex, WiiSocket};

mod audio;
mod gx;
mod input;
mod memory;
mod system;
mod textures;

// The power button of the Wii and of the Wii Remotes, libogc calls it from an interrupt
static POWER_PRESSED: AtomicBool = AtomicBool::new(false);

struct Wii;

impl Platform for Wii {
    type Lock = Mutex;
    type Signal = Condvar;
    type Socket = WiiSocket;
    type Gpu = Gx;
    type Audio = Aesnd;
    type Controller = Remote;

    const REMOTE_MANAGEMENT: bool = true;
    const NAME: &'static str = "Wii";
    // The render distance is fixed
    const MENU: Menu = Menu {
        keyboard: Keyboard::OnScreen,
        quits: true,
        distance: false,
        confirm: "+",
        open: "Press A on a world",
        controls: &[
            "Stick look - D-pad up/down move - D-pad left/right slot",
            "A place - B break - C jump - Z sneak/down",
            "1 pick - 2 blocks - Minus tools - +/HOME pause",
        ],
    };
    // Less fluid flows than on computers, the Wii has less time per frame
    const LIMITS: Limits = Limits {
        fluid_radius: 2,
        fluid_updates: 128,
    };
    const RENDER_DISTANCE: i64 = 8;
    const MAX_GUESTS: usize = 3;
    const INVENTORY_HELP: &'static str = "A assign - Z/C slot - B/2 close";

    fn spawn(thread: Thread, entry: Box<dyn FnOnce() + Send>) {
        system::spawn(thread, entry);
    }

    fn seconds() -> f64 {
        system::seconds()
    }

    fn random() -> u64 {
        unsafe { gettime() }.wrapping_mul(6_364_136_223_846_793_005)
    }

    fn idle() {
        unsafe { VIDEO_WaitVSync() };
    }

    fn connect_network() -> Result<[u8; 4], i32> {
        system::connect()
    }

    // The Wii VFS has no other storage backend than the FAT32 SD card
    fn mount_storage() -> bool {
        unsafe { wiiMountSd(1) != 0 }
    }

    fn nickname() -> Option<String> {
        system::nickname()
    }

    fn running() -> bool {
        !POWER_PRESSED.load(Ordering::Relaxed)
    }
}

extern "C" fn power() {
    POWER_PRESSED.store(true, Ordering::Relaxed);
}

#[cfg(feature = "bench")]
#[unsafe(export_name = "main")]
pub extern "C" fn bench_main() -> c_int {
    let line = alloc::format!("{}\0", plaatcraft_engine::client::bench::<Wii>());
    // The results go to stdout, which is Dolphin's debug UART
    unsafe { puts(line.as_ptr().cast()) };
    // Returning reloads the system menu, the script stops Dolphin after the results
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(not(feature = "bench"))]
#[unsafe(export_name = "main")]
pub extern "C" fn app_main() -> c_int {
    let Some(gx) = Gx::new() else {
        return 1;
    };
    let remote = Remote::new(gx.size());
    unsafe { SYS_SetPowerCallback(Some(power)) };
    let mut game = Game::<Wii>::new(gx, Aesnd::new(), remote);
    game.run();
    drop(game);
    if POWER_PRESSED.load(Ordering::Relaxed) {
        unsafe { SYS_ResetSystem(SYS_POWEROFF, 0, 0) };
    }
    0
}
