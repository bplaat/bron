/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Persisted performance, sound, and control preferences.

use std::io;

pub(crate) use plaatcraft_engine::menu::{
    MAX_DISTANCE, MAX_VOLUME, MIN_DISTANCE, Settings, distance_at,
};
use plaatcraft_engine::protocol::validate_name;
use preferences::{Preferences, Value};

use crate::server::Identity;

const DISTANCE: &str = "render_distance";
const SHOW_STATS: &str = "show_stats";
const VOLUME: &str = "volume";
const AUTO_JUMP: &str = "auto_jump";
// The chosen username, or the random one of a computer without a system account.
const USERNAME: &str = "username";

pub(crate) const DEFAULT_DISTANCE: u8 = if cfg!(any(target_os = "ios", target_os = "android")) {
    16
} else {
    32
};

/// The settings of a new player.
pub(crate) const fn defaults() -> Settings {
    Settings {
        distance: DEFAULT_DISTANCE,
        show_stats: true,
        volume: 80,
        auto_jump: true,
    }
}

/// The settings in the shared preferences, only written when they changed.
pub(crate) struct Store {
    prefs: Preferences,
    saved: Settings,
    identity: Identity,
}

// The name of the system account, when other players can see it as it is.
fn system_name() -> Option<String> {
    ["USER", "USERNAME", "LOGNAME"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|name| validate_name(name) == Ok(name.as_str()))
}

// Phones have no system account, so they get a random name to tell them apart.
fn random_name() -> String {
    let number = std::hash::BuildHasher::hash_one(
        &std::hash::RandomState::new(),
        std::time::SystemTime::now(),
    ) % 1000;
    format!("Player{number:03}")
}

impl Store {
    pub(crate) fn new() -> io::Result<Self> {
        let prefs = Preferences::new("nl.bplaat.PlaatCraft")?;
        let defaults = defaults();
        let saved = Settings {
            distance: match prefs.get(DISTANCE)? {
                Some(Value::Integer(value)) => u8::try_from(value)
                    .ok()
                    .filter(|value| (MIN_DISTANCE..=MAX_DISTANCE).contains(value))
                    .unwrap_or(defaults.distance),
                _ => defaults.distance,
            },
            show_stats: match prefs.get(SHOW_STATS)? {
                Some(Value::Bool(value)) => value,
                _ => defaults.show_stats,
            },
            volume: match prefs.get(VOLUME)? {
                Some(Value::Integer(value)) => u8::try_from(value)
                    .ok()
                    .filter(|value| *value <= MAX_VOLUME)
                    .unwrap_or(defaults.volume),
                _ => defaults.volume,
            },
            auto_jump: match prefs.get(AUTO_JUMP)? {
                Some(Value::Bool(value)) => value,
                _ => defaults.auto_jump,
            },
        };
        let name = match (prefs.get(USERNAME)?, system_name()) {
            (Some(Value::String(name)), _) if validate_name(&name).is_ok() => name,
            (_, Some(name)) => name,
            (_, None) => {
                let name = random_name();
                prefs.set(USERNAME, name.as_str())?;
                name
            }
        };
        Ok(Self {
            prefs,
            saved,
            identity: Identity { name },
        })
    }

    pub(crate) const fn settings(&self) -> Settings {
        self.saved
    }

    /// The player name other players see.
    pub(crate) const fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Changes the username, which other players see from the next time they join.
    pub(crate) fn set_name(&mut self, name: &str) -> Result<(), String> {
        let name = validate_name(name)?;
        self.prefs
            .set(USERNAME, name)
            .map_err(|error| error.to_string())?;
        name.clone_into(&mut self.identity.name);
        Ok(())
    }

    pub(crate) fn save(&mut self, settings: Settings) -> io::Result<()> {
        if settings.distance != self.saved.distance {
            self.prefs.set(DISTANCE, i64::from(settings.distance))?;
        }
        if settings.show_stats != self.saved.show_stats {
            self.prefs.set(SHOW_STATS, settings.show_stats)?;
        }
        if settings.volume != self.saved.volume {
            self.prefs.set(VOLUME, i64::from(settings.volume))?;
        }
        if settings.auto_jump != self.saved.auto_jump {
            self.prefs.set(AUTO_JUMP, settings.auto_jump)?;
        }
        self.saved = settings;
        Ok(())
    }
}
