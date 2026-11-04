/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sound effects of the engine, rendered from their sfxr samples, played through the
//! console's [`Audio`] with the volume setting.

use alloc::vec::Vec;

use super::platform::Audio;
use crate::camera::Camera;
use crate::menu::MAX_VOLUME;
use crate::sound::{Effect, Footsteps, Movement, Play};

/// Renders an effect like baudio does on the desktop, as 16-bit mono samples at
/// [`sfxr::SAMPLE_RATE`].
pub fn samples(effect: Effect) -> Vec<i16> {
    let mut generator = sfxr::Generator::new(effect.sample());
    generator.volume = 0.5;
    let mut samples = Vec::new();
    let mut chunk = [0.0; 1024];
    while !generator.finished() {
        generator.generate(&mut chunk);
        samples.extend(chunk.map(|sample| (sample * 32767.0) as i16));
    }
    samples
}

pub(super) struct Sounds<A> {
    audio: A,
    volume: f32,
}

impl<A: Audio> Sounds<A> {
    pub(super) const fn new(audio: A) -> Self {
        Self { audio, volume: 1.0 }
    }

    pub(super) fn set_volume(&mut self, volume: u8) {
        self.volume = f32::from(volume) / f32::from(MAX_VOLUME);
    }

    fn play(audio: &mut A, volume: f32, play: Play) {
        let pan = play.pan.clamp(-1.0, 1.0);
        let side = |side: f32| play.volume * volume * side.min(1.0);
        audio.play(play.effect, [side(1.0 - pan), side(1.0 + pan)], play.pitch);
    }

    /// A menu or hotbar click
    pub(super) fn click(&mut self) {
        Self::play(&mut self.audio, self.volume, Play::click());
    }

    /// Taking off or landing from flight
    pub(super) fn fly(&mut self, flying: bool) {
        Self::play(&mut self.audio, self.volume, Play::fly(flying));
    }

    /// A broken or placed block
    pub(super) fn block(
        &mut self,
        camera: &Camera,
        position: [i64; 3],
        material: u8,
        broken: bool,
    ) {
        let (audio, volume) = (&mut self.audio, self.volume);
        Play::block(camera, position, material, broken, |play| {
            Self::play(audio, volume, play);
        });
    }

    /// Jumps, footsteps, splashes and landings since `before`
    pub(super) fn movement(
        &mut self,
        footsteps: &mut Footsteps,
        before: Movement,
        camera: &Camera,
        ground: Option<u8>,
    ) {
        let (audio, volume) = (&mut self.audio, self.volume);
        footsteps.update(before, camera, ground, |play| {
            Self::play(audio, volume, play);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Output {
        effects: Vec<Effect>,
    }
    impl Audio for Output {
        fn play(&mut self, effect: Effect, _: [f32; 2], _: f32) {
            self.effects.push(effect);
        }
    }

    #[test]
    fn one_players_flying_does_not_reset_another_players_footsteps() {
        let mut sounds = Sounds::new(Output::default());
        let mut walking = Footsteps::default();
        let mut flying = Footsteps::default();
        let mut camera = Camera {
            grounded: true,
            ..Camera::default()
        };
        let before = Movement::of(&camera);
        camera.position[0] += 1.2;
        sounds.movement(&mut walking, before, &camera, Some(3));
        assert!(sounds.audio.effects.is_empty());
        let flyer = Camera {
            flying: true,
            ..Camera::default()
        };
        sounds.movement(&mut flying, Movement::of(&flyer), &flyer, None);
        let before = Movement::of(&camera);
        camera.position[0] += 1.2;
        sounds.movement(&mut walking, before, &camera, Some(3));
        assert_eq!(sounds.audio.effects.len(), 1);
    }
}
