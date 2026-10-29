/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The saved worlds and settings, serde types encoded with postcard and validated when loaded.
use alloc::string::String;
use alloc::vec::Vec;

use plaatcraft_engine::menu;
use plaatcraft_engine::protocol::{Player, validate_name};
use plaatcraft_engine::world::{self, Edit, HEIGHT};
use serde::{Deserialize, Serialize};

use crate::local::MAX_GUESTS;
use crate::stream::RENDER_DISTANCE;

pub const MAX_SAVE_BYTES: usize = 8 * 1024 * 1024;
pub const WORLD_SLOTS: usize = 16;

/// The settings of the game and the player name, which all worlds share like on the desktop.
#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub game: menu::Settings,
    pub username: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            game: menu::Settings {
                distance: RENDER_DISTANCE as u8,
                show_stats: false,
                volume: menu::MAX_VOLUME,
                auto_jump: true,
            },
            username: "Wii".into(),
        }
    }
}
impl Settings {
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("Settings always encode")
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let settings: Self = postcard::from_bytes(bytes).ok()?;
        let valid =
            settings.game.volume <= menu::MAX_VOLUME && validate_name(&settings.username).is_ok();
        valid.then_some(settings)
    }
}

/// A saved world: its blocks are the generated terrain with the edits.
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub name: String,
    pub seed: u64,
    /// When the world was saved, in seconds since 1970.
    pub saved: i64,
    /// Counts the saves of the world.
    pub sequence: u64,
    pub player: Player,
    /// The players that joined over the LAN, by name.
    pub guests: Vec<(String, Player)>,
    /// The blocks whose fluid still flows.
    pub fluids: Vec<[i64; 3]>,
    pub edits: Vec<Edit>,
}

/// The longest world name.
pub const MAX_NAME_LENGTH: usize = 24;
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= MAX_NAME_LENGTH
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" -_".contains(&b))
}

impl Snapshot {
    pub fn encode(&self) -> Option<Vec<u8>> {
        let data = postcard::to_allocvec(self).ok()?;
        (data.len() <= MAX_SAVE_BYTES).then_some(data)
    }
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let save: Self = postcard::from_bytes(bytes).ok()?;
        let valid_edit = |edit: &Edit| {
            (3..HEIGHT as i64).contains(&edit.position[1])
                && edit.position[0].unsigned_abs() <= 1_000_000_000
                && edit.position[2].unsigned_abs() <= 1_000_000_000
                && (edit.material == 0
                    || world::fluid(edit.material).is_some()
                    || world::valid_material(edit.material))
        };
        let valid = valid_name(&save.name)
            && save.player.validate().is_ok()
            && save.guests.len() <= MAX_GUESTS
            && save.fluids.len() <= MAX_SAVE_BYTES / 24
            && save
                .guests
                .iter()
                .all(|(name, player)| validate_name(name).is_ok() && player.validate().is_ok())
            && save.edits.iter().all(valid_edit);
        valid.then_some(save)
    }
}
