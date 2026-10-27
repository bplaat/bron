/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sound effects of plaatcraft-engine, rendered from their sfxr samples and mixed by
//! baudio.

use baudio::{Audio, Sound};
use plaatcraft_engine::camera;
use plaatcraft_engine::sound::{Effect, Footsteps, Play};
pub(crate) use plaatcraft_engine::sound::{Movement, ground};

use crate::settings::{MAX_VOLUME, Settings};

/// The audio device with the rendered effects
struct Mixer {
    audio: Option<Audio>,
    effects: [Sound; Effect::ALL.len()],
}

impl Mixer {
    fn play(&self, play: Play) {
        if let Some(audio) = &self.audio {
            audio.play_with(
                &self.effects[play.effect as usize],
                baudio::Play {
                    volume: play.volume,
                    pan: play.pan,
                    pitch: play.pitch,
                    ..baudio::Play::default()
                },
            );
        }
    }
}

pub(crate) struct Sounds {
    mixer: Mixer,
    footsteps: Footsteps,
}

impl Sounds {
    /// Renders all sounds, the game stays silent without an audio device
    pub(crate) fn new(settings: &Settings) -> Self {
        let audio = Audio::new()
            .inspect_err(|error| eprintln!("Audio unavailable: {error}"))
            .ok();
        let sounds = Self {
            mixer: Mixer {
                audio,
                effects: Effect::ALL.map(|effect| Sound::from_sfxr(&effect.sample())),
            },
            footsteps: Footsteps::default(),
        };
        sounds.apply(settings);
        sounds
    }

    /// Sets the master volume from the volume setting
    pub(crate) fn apply(&self, settings: &Settings) {
        if let Some(audio) = &self.mixer.audio {
            // Loudness is perceived logarithmically, a square curve feels more even
            let volume = f32::from(settings.volume) / f32::from(MAX_VOLUME);
            audio.set_volume(volume.powi(2));
        }
    }

    /// A menu, hotbar or inventory click
    pub(crate) fn click(&self) {
        self.mixer.play(Play::click());
    }

    /// A broken or placed block
    pub(crate) fn block(
        &self,
        camera: &camera::Camera,
        position: [i64; 3],
        material: u8,
        broken: bool,
    ) {
        Play::block(camera, position, material, broken, |play| {
            self.mixer.play(play)
        });
    }

    /// Footsteps, splashes and landings since `before`
    pub(crate) fn movement(
        &mut self,
        before: Movement,
        camera: &camera::Camera,
        ground: Option<u8>,
    ) {
        let mixer = &self.mixer;
        self.footsteps
            .update(before, camera, ground, |play| mixer.play(play));
    }
}
