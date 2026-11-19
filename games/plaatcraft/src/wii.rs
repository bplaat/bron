/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! PlaatCraft for the Wii, the engine client drawn with GX and played with a
//! Wii Remote and Nunchuk.

#![cfg_attr(feature = "bench", allow(dead_code, unused_imports))]

use core::sync::atomic::{AtomicBool, Ordering};
use std::process::ExitCode;

use libogc_sys::*;
use plaatcraft::client::{Game, Gpu, Platform, Thread};
use plaatcraft::menu::{Keyboard, Platform as Menu};
use plaatcraft::server::Limits;

use crate::wii::audio::Aesnd;
use crate::wii::gx::Gx;
use crate::wii::input::Remote;
use crate::wii::system::{Condvar, Mutex, WiiSocket};

mod audio;
mod gx;
mod input;
mod memory;
mod system;
mod textures;
mod usb;

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
pub(crate) fn main() {
    println!("{}", plaatcraft::client::bench::<Wii>());
    // Returning reloads the system menu, the script stops Dolphin after the results
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(not(feature = "bench"))]
pub(crate) fn main() -> ExitCode {
    let Some(gx) = Gx::new() else {
        return ExitCode::FAILURE;
    };
    let remote = Remote::new(gx.size());
    unsafe { SYS_SetPowerCallback(Some(power)) };
    let mut game = Game::<Wii>::new(gx, Aesnd::new(), remote);
    game.run();
    drop(game);
    if POWER_PRESSED.load(Ordering::Relaxed) {
        unsafe { SYS_ResetSystem(SYS_POWEROFF, 0, 0) };
    }
    ExitCode::SUCCESS
}
