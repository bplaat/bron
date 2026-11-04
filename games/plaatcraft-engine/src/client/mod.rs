/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The game client of the consoles: the menus, the chunks streamed on a worker thread, the
//! SQLite worlds and settings on the SD card, the integrated server, the LAN, the HUD, the
//! sound effects and the controls of the Wii Remote and the Wii U GamePad. A port implements
//! [`Platform`] with its GPU, input, sound output, threads and sockets and runs [`Game`].

mod bench;
mod files;
mod font;
mod game;
mod hud;
pub mod input;
mod light;
mod local;
pub mod mesh;
mod network;
mod platform;
mod render;
mod settings;
pub mod sound;
mod storage;
mod stream;
mod sync;

pub use bench::bench;
pub use game::{Game, viewport};
pub use light::{NIGHT_DAYLIGHT, linear_palette, linear_rgb, palette};
pub use platform::{
    Address, Audio, Controller, Fog, Gpu, Io, Lock, Mtx, Mtx44, Pass, Platform, Signal, Socket,
    State, Texture, Thread, Vertex,
};
