/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The sound effects as sfxr samples, and when they play, how loud and from which side. The
//! games render and mix them. Block sounds depend on the surface of the material and come
//! from the direction of the block.

use crate::camera::Camera;
#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::world;

/// A footstep every this many blocks walked, a swim stroke every two
const STRIDE: f64 = 2.2;

/// The eye height above the feet
const EYE_HEIGHT: f64 = 1.7;

/// A sound effect, the first ones are the surfaces of materials
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Stone, rock and ores
    Stone,
    /// Dirt and grass blocks
    Dirt,
    /// Sand, gravel and snow
    Sand,
    /// Logs, planks and tables
    Wood,
    /// Leaves, wool and plants
    Soft,
    /// Ice
    Ice,
    /// Water and lava
    Splash,
    /// The debris layered over a breaking block
    Crumble,
    /// A menu, hotbar or inventory click
    Click,
    /// A rising blip as the player leaves the ground
    Jump,
    /// A gust of wind when taking off or landing from flight
    Fly,
}

impl Effect {
    /// All effects in order, so `effect as usize` indexes them
    pub const ALL: [Self; 11] = [
        Self::Stone,
        Self::Dirt,
        Self::Sand,
        Self::Wood,
        Self::Soft,
        Self::Ice,
        Self::Splash,
        Self::Crumble,
        Self::Click,
        Self::Jump,
        Self::Fly,
    ];

    /// How a material sounds when it is broken, placed and walked on
    pub fn surface(material: u8) -> Self {
        if matches!(material, world::WATER | world::LAVA) {
            return Self::Splash;
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

    /// The sfxr sample the effect is rendered from
    pub const fn sample(self) -> sfxr::Sample {
        use sfxr::{Sample, WaveType};
        match self {
            Self::Stone => Sample {
                wave_type: WaveType::Noise,
                base_freq: 0.4,
                freq_ramp: -0.08,
                env_attack: 0.025,
                env_sustain: 0.025,
                env_decay: 0.17,
                hpf_freq: 0.08,
                lpf_freq: 0.7,
                ..Sample::new()
            },
            Self::Dirt => Sample {
                wave_type: WaveType::Noise,
                base_freq: 0.18,
                env_attack: 0.025,
                env_sustain: 0.04,
                env_decay: 0.2,
                lpf_freq: 0.35,
                lpf_ramp: -0.08,
                ..Sample::new()
            },
            Self::Sand => Sample {
                wave_type: WaveType::Noise,
                base_freq: 0.55,
                env_attack: 0.025,
                env_sustain: 0.06,
                env_decay: 0.24,
                lpf_freq: 0.65,
                hpf_freq: 0.12,
                ..Sample::new()
            },
            Self::Wood => Sample {
                wave_type: WaveType::Triangle,
                base_freq: 0.23,
                freq_ramp: -0.08,
                env_attack: 0.025,
                env_sustain: 0.025,
                env_decay: 0.19,
                lpf_freq: 0.45,
                ..Sample::new()
            },
            Self::Soft => Sample {
                wave_type: WaveType::Noise,
                base_freq: 0.45,
                env_attack: 0.035,
                env_sustain: 0.05,
                env_decay: 0.19,
                lpf_freq: 0.45,
                hpf_freq: 0.12,
                ..Sample::new()
            },
            Self::Ice => Sample {
                wave_type: WaveType::Sine,
                base_freq: 0.68,
                env_attack: 0.025,
                env_sustain: 0.015,
                env_decay: 0.27,
                pha_offset: 0.06,
                ..Sample::new()
            },
            Self::Splash => Sample {
                wave_type: WaveType::Noise,
                base_freq: 0.32,
                freq_ramp: -0.15,
                env_attack: 0.045,
                env_sustain: 0.1,
                env_decay: 0.4,
                lpf_freq: 0.55,
                lpf_ramp: -0.12,
                ..Sample::new()
            },
            Self::Crumble => Sample {
                wave_type: WaveType::Noise,
                base_freq: 0.28,
                freq_ramp: -0.08,
                env_attack: 0.02,
                env_sustain: 0.07,
                env_decay: 0.3,
                lpf_freq: 0.5,
                lpf_ramp: -0.1,
                ..Sample::new()
            },
            Self::Click => Sample {
                wave_type: WaveType::Sine,
                base_freq: 0.42,
                env_attack: 0.025,
                env_sustain: 0.01,
                env_decay: 0.08,
                hpf_freq: 0.05,
                ..Sample::new()
            },
            Self::Jump => Sample {
                wave_type: WaveType::Triangle,
                base_freq: 0.26,
                freq_ramp: 0.12,
                env_attack: 0.025,
                env_sustain: 0.035,
                env_decay: 0.15,
                lpf_freq: 0.5,
                ..Sample::new()
            },
            Self::Fly => Sample {
                wave_type: WaveType::Noise,
                base_freq: 0.24,
                freq_ramp: 0.06,
                env_attack: 0.16,
                env_sustain: 0.08,
                env_decay: 0.3,
                lpf_freq: 0.4,
                hpf_freq: 0.06,
                ..Sample::new()
            },
        }
    }
}

/// A sound effect to play
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Play {
    /// The sound effect
    pub effect: Effect,
    /// The volume, from 0.0 to 1.0
    pub volume: f32,
    /// From -1.0 for left to 1.0 for right
    pub pan: f32,
    /// The playback speed, lower sounds lower
    pub pitch: f32,
}

impl Play {
    const fn new(effect: Effect, volume: f32) -> Self {
        Self {
            effect,
            volume,
            pan: 0.0,
            pitch: 1.0,
        }
    }

    /// A menu, hotbar or inventory click
    pub const fn click() -> Self {
        Self::new(Effect::Click, 0.4)
    }

    /// Taking off rises, landing from flight sounds lower
    pub const fn fly(flying: bool) -> Self {
        Self {
            pitch: if flying { 1.0 } else { 0.7 },
            ..Self::new(Effect::Fly, 0.5)
        }
    }

    /// A broken block crumbles, a placed block sounds lower
    pub fn block(
        camera: &Camera,
        position: [i64; 3],
        material: u8,
        broken: bool,
        mut play: impl FnMut(Self),
    ) {
        let (volume, pan) = spatial(camera, position);
        let surface = Effect::surface(material);
        if broken {
            play(Self {
                pan,
                ..Self::new(surface, volume)
            });
            play(Self {
                pan,
                ..Self::new(Effect::Crumble, volume * 0.5)
            });
        } else {
            play(Self {
                pan,
                pitch: 0.75,
                ..Self::new(surface, volume * 0.8)
            });
        }
    }
}

/// The movement state of the player before an update
#[derive(Clone, Copy)]
pub struct Movement {
    position: [f64; 3],
    in_liquid: bool,
    grounded: bool,
    vertical_velocity: f64,
}

impl Movement {
    /// The movement state of the camera
    pub const fn of(camera: &Camera) -> Self {
        Self {
            position: camera.position,
            in_liquid: camera.in_liquid,
            grounded: camera.grounded,
            vertical_velocity: camera.vertical_velocity,
        }
    }
}

/// Jumps, footsteps on the ground, splashes in water and a thud on landing
#[derive(Default)]
pub struct Footsteps {
    /// The distance walked or swum since the last step
    stride: f64,
    steps: u32,
}

impl Footsteps {
    /// Plays the sounds of the movement since `before`, `ground` is the block under the feet
    pub fn update(
        &mut self,
        before: Movement,
        camera: &Camera,
        ground: Option<u8>,
        mut play: impl FnMut(Play),
    ) {
        if camera.flying {
            self.stride = 0.0;
            return;
        }
        if before.grounded
            && !camera.grounded
            && camera.vertical_velocity > 0.0
            && !camera.in_liquid
        {
            play(Play::new(Effect::Jump, 0.3));
        }
        if camera.in_liquid && !before.in_liquid {
            play(Play::new(Effect::Splash, 0.6));
        }
        // Falls faster than a jump land with a thud that grows with the speed
        if camera.grounded && !before.grounded && before.vertical_velocity < -9.0 {
            let impact = ((-before.vertical_velocity - 9.0) / 15.0).min(1.0) as f32;
            if let Some(ground) = ground {
                play(Play {
                    pitch: 0.7,
                    ..Play::new(Effect::surface(ground), 0.4 + impact * 0.6)
                });
            }
        }

        let [x, _, z] = camera.position;
        let moved = (x - before.position[0]).hypot(z - before.position[2]);
        let (stride, effect) = if camera.in_liquid {
            (STRIDE * 2.0, Some(Effect::Splash))
        } else if camera.grounded {
            (STRIDE, ground.map(Effect::surface))
        } else {
            return;
        };
        self.stride += moved;
        if self.stride < stride {
            return;
        }
        self.stride = 0.0;
        self.steps += 1;
        if let Some(effect) = effect {
            // Alternating feet sound a bit different
            let pitch = if self.steps.is_multiple_of(2) {
                0.95
            } else {
                1.05
            };
            play(Play {
                pitch,
                ..Play::new(effect, 0.25)
            });
        }
    }
}

/// The block under the feet of the camera
pub fn ground(camera: &Camera) -> [i64; 3] {
    let [x, y, z] = camera.position;
    [
        x.floor() as i64,
        (y - EYE_HEIGHT - 0.1).floor() as i64,
        z.floor() as i64,
    ]
}

/// The volume and pan of a block, from its distance and direction to the camera
fn spatial(camera: &Camera, position: [i64; 3]) -> (f32, f32) {
    let offset: [f64; 3] =
        core::array::from_fn(|axis| position[axis] as f64 + 0.5 - camera.position[axis]);
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
    fn effects_are_in_index_order() {
        for (index, effect) in Effect::ALL.into_iter().enumerate() {
            assert_eq!(effect as usize, index);
        }
    }

    #[test]
    fn presets_render_short_audible_effects_with_bounded_samples() {
        for effect in Effect::ALL {
            let mut generator = sfxr::Generator::new(effect.sample());
            generator.volume = 0.5;
            let mut samples = alloc::vec![0.0; sfxr::SAMPLE_RATE as usize];
            generator.generate(&mut samples);
            assert!(
                generator.finished(),
                "{effect:?} lasts longer than a second"
            );
            assert!(
                samples
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() <= 1.0),
                "{effect:?} contains invalid samples"
            );
            let peak = samples
                .iter()
                .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            assert!(peak > 0.01, "{effect:?} is inaudible");
            assert!(samples[0].abs() < 0.01, "{effect:?} starts abruptly");
            assert!(
                samples[samples.len() - 1024..]
                    .iter()
                    .all(|sample| sample.abs() < 0.001),
                "{effect:?} leaves an audible tail"
            );
        }
    }

    #[test]
    fn materials_have_fitting_surfaces() {
        for (material, effect) in [
            (1, Effect::Dirt),
            (2, Effect::Dirt),
            (3, Effect::Stone),
            (4, Effect::Sand),
            (5, Effect::Sand),
            (6, Effect::Wood),
            (9, Effect::Soft),
            (12, Effect::Soft),
            (28, Effect::Wood),
            (30, Effect::Soft),
            (45, Effect::Ice),
            (world::WATER, Effect::Splash),
            (19, Effect::Stone),
            (34, Effect::Sand),
            (56, Effect::Stone),
            (61, Effect::Wood),
        ] {
            assert_eq!(Effect::surface(material), effect, "material {material}");
        }
    }

    #[test]
    fn blocks_sound_from_their_side_and_fade_with_distance() {
        let camera = Camera {
            position: [0.5, 10.0, 0.5],
            yaw: 0.0,
            ..Camera::default()
        };
        let (near, right) = spatial(&camera, [3, 9, 0]);
        let (far, left) = spatial(&camera, [-30, 9, 0]);
        assert!(right > 0.7 && left < -0.7);
        assert!(near > far);
        let (_, ahead) = spatial(&camera, [0, 9, -5]);
        assert!(ahead.abs() < 0.001);
    }

    #[test]
    fn jumps_play_but_walking_off_ledges_does_not() {
        let mut footsteps = Footsteps::default();
        let mut camera = Camera {
            grounded: true,
            ..Camera::default()
        };
        let before = Movement::of(&camera);
        camera.grounded = false;
        camera.vertical_velocity = 8.0;
        let mut played = None;
        footsteps.update(before, &camera, None, |play| played = Some(play.effect));
        assert_eq!(played, Some(Effect::Jump));

        camera.vertical_velocity = -1.0;
        let mut played = None;
        footsteps.update(before, &camera, None, |play| played = Some(play.effect));
        assert_eq!(played, None);
    }
}
