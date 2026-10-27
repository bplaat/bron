/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The platform independent part of PlaatCraft: world generation, lighting, chunk
//! meshing, player physics, block picking, the hotbar, and when sound effects play. It
//! only needs `alloc`, so the bwindow game and the Wii port share it.

#![no_std]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod biome;
pub mod camera;
pub mod hotbar;
mod lighting;
#[cfg(not(feature = "std"))]
mod math;
mod perlin;
pub mod raycast;
pub mod sound;
pub mod tiles;
pub mod world;
