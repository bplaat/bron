/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The settings of the Wii, encoded with postcard and validated when loaded.
use alloc::string::String;
use alloc::vec::Vec;

use plaatcraft_engine::menu;
use plaatcraft_engine::protocol::validate_name;
use serde::{Deserialize, Serialize};

use crate::stream::RENDER_DISTANCE;

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
