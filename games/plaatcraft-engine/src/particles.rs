/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The bits of broken blocks, which fly apart, fall, and land on the blocks below, like
//! in Minecraft. The platforms draw them as small boxes with the texture of the block.

use alloc::vec::Vec;

#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::world::{self, CHUNK_SIZE, Coord};

/// The most bits that fly at once, the oldest make room for new ones.
pub const MAX_PARTICLES: usize = 256;
/// The width of a bit in blocks.
pub const SIZE: f32 = 0.11;
// The bits of one broken block, along each axis.
const SPLIT: usize = 3;
const GRAVITY: f32 = 16.0;

/// A flying bit of a broken block.
pub struct Particle {
    /// The center of the bit.
    pub position: [f64; 3],
    /// The material of the broken block.
    pub material: u8,
    /// The turn around the vertical axis.
    pub spin: f32,
    velocity: [f32; 3],
    // How long the bit still lives, in seconds.
    life: f32,
}

/// The flying bits.
#[derive(Default)]
pub struct Particles {
    /// The bits, the oldest first.
    pub particles: Vec<Particle>,
    random: u32,
}

impl Particles {
    // A pseudo random number from 0 to 1.
    fn random(&mut self) -> f32 {
        self.random = self
            .random
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.random >> 8) as f32 / (1 << 24) as f32
    }

    /// Breaks a block of a material into flying bits.
    pub fn burst(&mut self, position: [i64; 3], material: u8) {
        for index in 0..SPLIT * SPLIT * SPLIT {
            let cell = [
                index % SPLIT,
                index / SPLIT % SPLIT,
                index / (SPLIT * SPLIT),
            ]
            .map(|cell| (cell as f32 + 0.5) / SPLIT as f32);
            let velocity = core::array::from_fn(|axis| {
                (cell[axis] - 0.5) * 3.0 + (self.random() - 0.5) * 1.5 + [0.0, 2.5, 0.0][axis]
            });
            let life = 0.5 + self.random() * 0.6;
            let spin = self.random() * core::f32::consts::TAU;
            if self.particles.len() == MAX_PARTICLES {
                self.particles.remove(0);
            }
            self.particles.push(Particle {
                position: core::array::from_fn(|axis| {
                    position[axis] as f64 + f64::from(cell[axis])
                }),
                material,
                spin,
                velocity,
                life,
            });
        }
    }

    /// Moves the bits, which stop on solid blocks, and forgets the old ones.
    pub fn update(&mut self, dt: f32, mut block: impl FnMut([i64; 3]) -> Option<u8>) {
        let mut solid = |position: [f64; 3]| {
            block(position.map(|value| value.floor() as i64))
                .is_some_and(|material| !world::is_passable(material))
        };
        for particle in &mut self.particles {
            particle.life -= dt;
            particle.velocity[1] -= GRAVITY * dt;
            for axis in 0..3 {
                let mut next = particle.position;
                next[axis] += f64::from(particle.velocity[axis] * dt);
                if solid(next) {
                    // Bits land, and slide less along the ground.
                    particle.velocity[axis] = 0.0;
                    if axis == 1 {
                        particle.velocity[0] *= 0.5;
                        particle.velocity[2] *= 0.5;
                    }
                } else {
                    particle.position = next;
                }
            }
        }
        self.particles.retain(|particle| particle.life > 0.0);
    }

    /// The box instances relative to the center chunk, like the player boxes of
    /// [`crate::avatars`], with the texture layer of each material.
    pub fn instances(&self, center: Coord, layer: impl Fn(u8) -> u32) -> Vec<u8> {
        let mut bytes =
            Vec::with_capacity(self.particles.len() * crate::avatars::INSTANCE_BYTES as usize);
        for particle in &self.particles {
            let pivot = [
                (particle.position[0] - (center.0 * CHUNK_SIZE as i64) as f64) as f32,
                particle.position[1] as f32 - SIZE / 2.0,
                (particle.position[2] - (center.1 * CHUNK_SIZE as i64) as f64) as f32,
            ];
            let values = pivot
                .into_iter()
                .chain([particle.spin, 0.0, layer(particle.material) as f32])
                .chain([SIZE; 3])
                .chain([-SIZE / 2.0, 0.0, -SIZE / 2.0]);
            for value in values {
                bytes.extend_from_slice(&value.to_ne_bytes());
            }
        }
        bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_fly_land_on_the_ground_and_disappear() {
        let mut particles = Particles::default();
        particles.burst([0, 10, 0], 3);
        assert_eq!(particles.particles.len(), SPLIT * SPLIT * SPLIT);
        let ground = |position: [i64; 3]| Some(if position[1] < 10 { 3 } else { 0 });
        for _ in 0..20 {
            particles.update(0.02, ground);
        }
        assert!(particles.particles.iter().all(|p| p.position[1] >= 10.0));
        assert_eq!(
            particles.instances((0, 0), |_| 0).len(),
            27 * crate::avatars::INSTANCE_BYTES as usize
        );
        particles.update(2.0, ground);
        assert!(particles.particles.is_empty());
        for _ in 0..20 {
            particles.burst([0, 10, 0], 3);
        }
        assert_eq!(particles.particles.len(), MAX_PARTICLES);
    }
}
