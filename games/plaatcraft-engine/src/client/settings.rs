/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The settings of the console, encoded with postcard and validated when loaded.

use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use super::platform::Platform;
use crate::menu;
use crate::protocol::validate_name;

/// The settings of the game and the player name, which all worlds share like on the desktop.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Settings {
    pub(super) game: menu::Settings,
    pub(super) username: String,
}

impl Settings {
    /// The settings of a new console, with the name of its user.
    pub(super) fn new<P: Platform>() -> Self {
        Self {
            game: menu::Settings {
                distance: P::RENDER_DISTANCE as u8,
                show_stats: false,
                volume: menu::MAX_VOLUME,
                auto_jump: true,
            },
            username: P::nickname()
                .filter(|name| validate_name(name).is_ok())
                .unwrap_or_else(|| P::NAME.into()),
        }
    }

    pub(super) fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("Settings always encode")
    }

    pub(super) fn decode(bytes: &[u8]) -> Option<Self> {
        let settings: Self = postcard::from_bytes(bytes).ok()?;
        let valid =
            settings.game.volume <= menu::MAX_VOLUME && validate_name(&settings.username).is_ok();
        valid.then_some(settings)
    }
}
