/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The platform independent part of PlaatCraft: world generation, lighting, chunk
//! meshing, player physics and its client prediction, block picking, the hotbar, when sound
//! effects play, block particles, the game rules of the integrated server, the LAN protocol,
//! the other players' models, and the menus. It only needs `alloc`, so the bwindow game and
//! the Wii port share it, they add rendering, input, sound output, storage and networking.

#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

pub mod avatars;
mod biome;
pub mod camera;
pub mod flow;
pub mod hotbar;
mod lighting;
#[cfg(not(feature = "std"))]
mod math;
pub mod menu;
pub mod particles;
mod perlin;
pub mod prediction;
pub mod protocol;
pub mod raycast;
pub mod server;
pub mod sound;
pub mod tiles;
pub mod world;
