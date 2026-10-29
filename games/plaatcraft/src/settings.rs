/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Persisted performance, sound, and control preferences.

use std::io;

use preferences::{Preferences, Value};

use crate::server::Identity;

const DISTANCE: &str = "render_distance";
const SHOW_STATS: &str = "show_stats";
const VOLUME: &str = "volume";
const AUTO_JUMP: &str = "auto_jump";
// The chosen username, or the random one of a computer without a system account.
const USERNAME: &str = "username";

/// The sound volume in percent
pub(crate) const MAX_VOLUME: u8 = 100;

pub(crate) const MIN_DISTANCE: u8 = 4;
pub(crate) const MAX_DISTANCE: u8 = 128;
pub(crate) const DEFAULT_DISTANCE: u8 = if cfg!(any(target_os = "ios", target_os = "android")) {
    16
} else {
    32
};

fn distance_exponent() -> f32 {
    // Reserve the last quarter of the slider for half the maximum distance.
    let midpoint = f32::from(MAX_DISTANCE / 2 - MIN_DISTANCE);
    (midpoint / f32::from(MAX_DISTANCE - MIN_DISTANCE)).ln() / 0.75_f32.ln()
}

pub(crate) fn distance_at(fraction: f32) -> u8 {
    MIN_DISTANCE
        + (fraction.clamp(0.0, 1.0).powf(distance_exponent())
            * f32::from(MAX_DISTANCE - MIN_DISTANCE))
        .round() as u8
}

pub(crate) fn distance_fraction(distance: u8) -> f32 {
    (f32::from(distance.clamp(MIN_DISTANCE, MAX_DISTANCE) - MIN_DISTANCE)
        / f32::from(MAX_DISTANCE - MIN_DISTANCE))
    .powf(1.0 / distance_exponent())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Settings {
    pub(crate) distance: u8,
    pub(crate) show_stats: bool,
    pub(crate) volume: u8,
    pub(crate) auto_jump: bool,
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
        .find(|name| crate::protocol::validate_name(name) == Ok(name.as_str()))
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
        let defaults = Settings::default();
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
            (Some(Value::String(name)), _) if crate::protocol::validate_name(&name).is_ok() => name,
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
        let name = crate::protocol::validate_name(name)?;
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            distance: DEFAULT_DISTANCE,
            show_stats: true,
            volume: 80,
            auto_jump: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_slider_gives_short_distances_more_precision() {
        for (fraction, distance) in [(0.0, 4), (0.25, 8), (0.5, 26), (0.75, 64), (1.0, 128)] {
            assert_eq!(distance_at(fraction), distance);
        }
        assert!((distance_fraction(64) - 0.75).abs() < 0.0001);
        assert_eq!(distance_at(-1.0), MIN_DISTANCE);
        assert_eq!(distance_at(2.0), MAX_DISTANCE);
        for distance in MIN_DISTANCE..=MAX_DISTANCE {
            assert_eq!(distance_at(distance_fraction(distance)), distance);
        }
        let values: Vec<_> = (0..=1000)
            .map(|step| distance_at(step as f32 / 1000.0))
            .collect();
        assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
    }
}
