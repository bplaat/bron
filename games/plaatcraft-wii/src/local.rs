/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The world of the integrated server on the Wii: the engine's game rules run on the streamed
//! chunks, the players and the flowing fluid are kept for the save on the SD card.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use plaatcraft_engine::protocol::Player;
use plaatcraft_engine::server::{Limits, World};
use plaatcraft_engine::world::{ChunkData, Coord, Edit};

use crate::stream::Stream;

/// Less fluid flows than on computers, the Wii has less time per frame.
pub const LIMITS: Limits = Limits {
    fluid_radius: 2,
    fluid_updates: 128,
};
/// The most remembered LAN guests of a world.
pub const MAX_GUESTS: usize = 64;

/// The saved parts of a played world besides its edits.
#[derive(Default)]
pub struct Saved {
    pub player: Player,
    pub guests: BTreeMap<String, Player>,
    pub fluids: Vec<[i64; 3]>,
}

/// The storage of the integrated server.
pub struct Local<'a> {
    pub stream: &'a mut Stream,
    pub saved: &'a mut Saved,
}

impl World for Local<'_> {
    fn chunk(&mut self, coord: Coord) -> Option<ChunkData> {
        self.stream.chunk(coord)
    }

    fn save_edits(&mut self, edits: &[Edit]) -> Result<u64, String> {
        self.stream.apply(edits);
        Ok(self.stream.revision)
    }

    fn revision(&mut self) -> Result<u64, String> {
        Ok(self.stream.revision)
    }

    fn player(&mut self) -> Result<Player, String> {
        Ok(self.saved.player)
    }

    fn guest(&mut self, name: &str) -> Result<Option<Player>, String> {
        Ok(self.saved.guests.get(name).copied())
    }

    fn save_player(&mut self, guest: Option<&str>, player: &Player) -> Result<(), String> {
        match guest {
            None => self.saved.player = *player,
            Some(name)
                if self.saved.guests.len() < MAX_GUESTS || self.saved.guests.contains_key(name) =>
            {
                self.saved.guests.insert(name.into(), *player);
            }
            Some(_) => {}
        }
        Ok(())
    }

    fn save_fluids(&mut self, positions: &[[i64; 3]]) -> Result<(), String> {
        self.saved.fluids = positions.to_vec();
        Ok(())
    }
}
