/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The world of the integrated server on the Wii: the engine's game rules run on the streamed
//! chunks, the stream stores the edits in the world's chunks, the players and the flowing fluid
//! are kept until the stream saves them with the chunks.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use plaatcraft_engine::protocol::Player;
use plaatcraft_engine::server::{Limits, World};
use plaatcraft_engine::world::{ChunkData, Coord, Edit};

use crate::storage::Storage;
use crate::stream::Stream;

/// Less fluid flows than on computers, the Wii has less time per frame.
pub const LIMITS: Limits = Limits {
    fluid_radius: 2,
    fluid_updates: 128,
};
/// What a save writes besides the chunks: the host, the guests that played since the world
/// opened, and the blocks whose fluid still flows.
#[derive(Clone, Default, PartialEq)]
pub struct Saved {
    pub player: Player,
    pub guests: BTreeMap<String, Player>,
    pub fluids: Vec<[i64; 3]>,
}

/// The storage of the integrated server.
pub struct Local<'a> {
    pub stream: &'a mut Stream,
    pub saved: &'a mut Saved,
    pub storage: &'a Storage,
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
        match self.saved.guests.get(name) {
            Some(&player) => Ok(Some(player)),
            None => self.storage.guest(name),
        }
    }

    fn save_player(&mut self, guest: Option<&str>, player: &Player) -> Result<(), String> {
        match guest {
            None => self.saved.player = *player,
            Some(name) => _ = self.saved.guests.insert(name.into(), *player),
        }
        Ok(())
    }

    fn save_fluids(&mut self, positions: &[[i64; 3]]) -> Result<(), String> {
        self.saved.fluids = positions.to_vec();
        Ok(())
    }
}
