/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sound effects, designed as sfxr samples and mixed by baudio. Block sounds depend on the
//! surface of the material and come from the direction of the block.

use baudio::{Audio, Play, Sound};
use plaatcraft_engine::{camera, world};
use sfxr::{Sample, WaveType};

use crate::settings::{MAX_VOLUME, Settings};

const STONE: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.45,
    freq_ramp: -0.1,
    env_attack: 0.0,
    env_sustain: 0.02,
    env_decay: 0.12,
    env_punch: 0.5,
    hpf_freq: 0.15,
    ..Sample::new()
};

const DIRT: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.15,
    env_attack: 0.0,
    env_sustain: 0.03,
    env_decay: 0.14,
    env_punch: 0.3,
    lpf_freq: 0.45,
    ..Sample::new()
};

const SAND: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.6,
    env_attack: 0.01,
    env_sustain: 0.04,
    env_decay: 0.16,
    lpf_freq: 0.8,
    hpf_freq: 0.25,
    ..Sample::new()
};

const WOOD: Sample = Sample {
    duty: 0.5,
    base_freq: 0.24,
    freq_ramp: -0.15,
    env_attack: 0.0,
    env_sustain: 0.02,
    env_decay: 0.12,
    env_punch: 0.5,
    lpf_freq: 0.5,
    ..Sample::new()
};

const SOFT: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.5,
    env_attack: 0.02,
    env_sustain: 0.03,
    env_decay: 0.1,
    lpf_freq: 0.6,
    hpf_freq: 0.3,
    ..Sample::new()
};

const ICE: Sample = Sample {
    wave_type: WaveType::Sine,
    base_freq: 0.7,
    env_attack: 0.0,
    env_sustain: 0.02,
    env_decay: 0.2,
    env_punch: 0.3,
    pha_offset: 0.1,
    ..Sample::new()
};

const SPLASH: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.3,
    freq_ramp: -0.2,
    env_attack: 0.02,
    env_sustain: 0.06,
    env_decay: 0.35,
    lpf_freq: 0.5,
    lpf_ramp: -0.2,
    ..Sample::new()
};

/// The debris layered over a breaking block
const CRUMBLE: Sample = Sample {
    wave_type: WaveType::Noise,
    base_freq: 0.25,
    freq_ramp: -0.05,
    env_attack: 0.0,
    env_sustain: 0.05,
    env_decay: 0.25,
    lpf_freq: 0.55,
    repeat_speed: 0.55,
    ..Sample::new()
};

const CLICK: Sample = Sample {
    wave_type: WaveType::Sine,
    base_freq: 0.5,
    env_attack: 0.0,
    env_sustain: 0.01,
    env_decay: 0.07,
    hpf_freq: 0.1,
    ..Sample::new()
};

/// A footstep every this many blocks walked, a swim stroke every two
const STRIDE: f64 = 2.2;

/// The eye height above the feet
const EYE_HEIGHT: f64 = 1.7;

/// How a material sounds when it is broken, placed and walked on
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
    Stone,
    Dirt,
    Sand,
    Wood,
    Soft,
    Ice,
    Liquid,
}

impl Surface {
    fn of(material: u8) -> Self {
        if matches!(material, world::WATER | world::LAVA) {
            return Self::Liquid;
        }
        let name = world::MATERIALS
            .iter()
            .find(|(id, _)| *id == material)
            .map_or("", |(_, name)| name)
            .to_ascii_lowercase();
        let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
        // The grass plants are numbered, unlike the grass block
        let plant = name.starts_with("grass ") && name.ends_with(|c: char| c.is_ascii_digit());
        if has(&["log", "plank", "table", "cactus"]) {
            Self::Wood
        } else if plant || has(&["leaves", "wool"]) {
            Self::Soft
        } else if has(&["ice"]) {
            Self::Ice
        } else if has(&["gravel"]) {
            Self::Sand
        } else if has(&["stone", "rock"]) {
            Self::Stone
        } else if has(&["sand", "snow"]) {
            Self::Sand
        } else if has(&["grass", "dirt"]) {
            Self::Dirt
        } else {
            Self::Stone
        }
    }
}

/// The movement state of the player before an update
#[derive(Clone, Copy)]
pub(crate) struct Movement {
    position: [f64; 3],
    in_liquid: bool,
    grounded: bool,
    vertical_velocity: f64,
}

impl Movement {
    pub(crate) const fn of(camera: &camera::Camera) -> Self {
        Self {
            position: camera.position,
            in_liquid: camera.in_liquid,
            grounded: camera.grounded,
            vertical_velocity: camera.vertical_velocity,
        }
    }
}

pub(crate) struct Sounds {
    audio: Option<Audio>,
    surfaces: [Sound; 7],
    crumble: Sound,
    click: Sound,
    /// The distance walked or swum since the last step
    stride: f64,
    steps: u32,
}

impl Sounds {
    /// Renders all sounds, the game stays silent without an audio device
    pub(crate) fn new(settings: &Settings) -> Self {
        let audio = Audio::new()
            .inspect_err(|error| eprintln!("Audio unavailable: {error}"))
            .ok();
        let sounds = Self {
            audio,
            surfaces: [STONE, DIRT, SAND, WOOD, SOFT, ICE, SPLASH].map(|s| Sound::from_sfxr(&s)),
            crumble: Sound::from_sfxr(&CRUMBLE),
            click: Sound::from_sfxr(&CLICK),
            stride: 0.0,
            steps: 0,
        };
        sounds.apply(settings);
        sounds
    }

    /// Sets the master volume from the volume setting
    pub(crate) fn apply(&self, settings: &Settings) {
        if let Some(audio) = &self.audio {
            // Loudness is perceived logarithmically, a square curve feels more even
            let volume = f32::from(settings.volume) / f32::from(MAX_VOLUME);
            audio.set_volume(volume.powi(2));
        }
    }

    fn play(&self, sound: &Sound, volume: f32, pan: f32, pitch: f32) {
        if let Some(audio) = &self.audio {
            audio.play_with(
                sound,
                Play {
                    volume,
                    pan,
                    pitch,
                    ..Play::default()
                },
            );
        }
    }

    fn surface(&self, material: u8) -> &Sound {
        &self.surfaces[Surface::of(material) as usize]
    }

    /// A menu, hotbar or inventory click
    pub(crate) fn click(&self) {
        self.play(&self.click, 0.4, 0.0, 1.0);
    }

    /// A broken block crumbles, a placed block sounds lower
    pub(crate) fn block(
        &self,
        camera: &camera::Camera,
        position: [i64; 3],
        material: u8,
        broken: bool,
    ) {
        let (volume, pan) = spatial(camera, position);
        if broken {
            self.play(self.surface(material), volume, pan, 1.0);
            self.play(&self.crumble, volume * 0.5, pan, 1.0);
        } else {
            self.play(self.surface(material), volume * 0.8, pan, 0.75);
        }
    }

    /// Footsteps on the ground, splashes in water and a thud on landing
    pub(crate) fn movement(
        &mut self,
        before: Movement,
        camera: &camera::Camera,
        ground: Option<u8>,
    ) {
        if camera.flying {
            self.stride = 0.0;
            return;
        }
        if camera.in_liquid && !before.in_liquid {
            self.play(&self.surfaces[Surface::Liquid as usize], 0.6, 0.0, 1.0);
        }
        // Falls faster than a jump land with a thud that grows with the speed
        if camera.grounded && !before.grounded && before.vertical_velocity < -9.0 {
            let impact = ((-before.vertical_velocity - 9.0) / 15.0).min(1.0) as f32;
            if let Some(ground) = ground {
                self.play(self.surface(ground), 0.4 + impact * 0.6, 0.0, 0.7);
            }
        }

        let [x, _, z] = camera.position;
        let moved = (x - before.position[0]).hypot(z - before.position[2]);
        let (stride, sound) = if camera.in_liquid {
            (STRIDE * 2.0, Some(Surface::Liquid))
        } else if camera.grounded {
            (STRIDE, ground.map(Surface::of))
        } else {
            return;
        };
        self.stride += moved;
        if self.stride < stride {
            return;
        }
        self.stride = 0.0;
        self.steps += 1;
        if let Some(surface) = sound {
            // Alternating feet sound a bit different
            let pitch = if self.steps.is_multiple_of(2) {
                0.95
            } else {
                1.05
            };
            self.play(&self.surfaces[surface as usize], 0.25, 0.0, pitch);
        }
    }
}

/// The block under the feet of the camera
pub(crate) fn ground(camera: &camera::Camera) -> [i64; 3] {
    let [x, y, z] = camera.position;
    [
        x.floor() as i64,
        (y - EYE_HEIGHT - 0.1).floor() as i64,
        z.floor() as i64,
    ]
}

/// The volume and pan of a block, from its distance and direction to the camera
fn spatial(camera: &camera::Camera, position: [i64; 3]) -> (f32, f32) {
    let offset: [f64; 3] =
        std::array::from_fn(|axis| position[axis] as f64 + 0.5 - camera.position[axis]);
    let distance = offset.iter().map(|value| value * value).sum::<f64>().sqrt();
    let horizontal = offset[0].hypot(offset[2]);
    // The camera looks along (sin yaw, -cos yaw), so its right is (cos yaw, sin yaw)
    let (sin, cos) = f64::from(camera.yaw).sin_cos();
    let pan = if horizontal > 0.001 {
        (offset[0] * cos + offset[2] * sin) / horizontal * 0.8
    } else {
        0.0
    };
    ((1.0 / (1.0 + distance * 0.15)) as f32, pan as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materials_have_fitting_surfaces() {
        for (material, surface) in [
            (1, Surface::Dirt),
            (2, Surface::Dirt),
            (3, Surface::Stone),
            (4, Surface::Sand),
            (5, Surface::Sand),
            (6, Surface::Wood),
            (9, Surface::Soft),
            (12, Surface::Soft),
            (28, Surface::Wood),
            (30, Surface::Soft),
            (45, Surface::Ice),
            (world::WATER, Surface::Liquid),
            (19, Surface::Stone),
            (34, Surface::Sand),
            (56, Surface::Stone),
            (61, Surface::Wood),
        ] {
            assert_eq!(Surface::of(material), surface, "material {material}");
        }
    }

    #[test]
    fn blocks_sound_from_their_side_and_fade_with_distance() {
        let camera = camera::Camera {
            position: [0.5, 10.0, 0.5],
            yaw: 0.0,
            ..camera::Camera::default()
        };
        let (near, right) = spatial(&camera, [3, 9, 0]);
        let (far, left) = spatial(&camera, [-30, 9, 0]);
        assert!(right > 0.7 && left < -0.7);
        assert!(near > far);
        let (_, ahead) = spatial(&camera, [0, 9, -5]);
        assert!(ahead.abs() < 0.001);
    }
}
