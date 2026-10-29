/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The bits of broken blocks, which fly apart, fall, and land on the blocks below, like
//! in Minecraft. They are small boxes with the texture of the block, drawn by the player
//! box pipeline, see players.rs.

use plaatcraft_engine::world::{self, CHUNK_SIZE, Coord};

use crate::players::INSTANCE_BYTES;
use crate::textures;

/// The most bits that fly at once, the oldest make room for new ones.
pub(crate) const MAX_PARTICLES: usize = 256;
// The bits of one broken block, along each axis.
const SPLIT: usize = 3;
const GRAVITY: f32 = 16.0;
const SIZE: f32 = 0.11;

struct Particle {
    position: [f64; 3],
    velocity: [f32; 3],
    spin: f32,
    layer: u32,
    // How long the bit still lives, in seconds.
    life: f32,
}

#[derive(Default)]
pub(crate) struct Particles {
    particles: Vec<Particle>,
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
    pub(crate) fn burst(&mut self, position: [i64; 3], material: u8) {
        // Plants show their plane, blocks their side.
        let face = if world::is_sprite(material) { 6 } else { 0 };
        let layer = textures::layer(material, face);
        for index in 0..SPLIT * SPLIT * SPLIT {
            let cell = [
                index % SPLIT,
                index / SPLIT % SPLIT,
                index / (SPLIT * SPLIT),
            ]
            .map(|cell| (cell as f32 + 0.5) / SPLIT as f32);
            let velocity = std::array::from_fn(|axis| {
                (cell[axis] - 0.5) * 3.0 + (self.random() - 0.5) * 1.5 + [0.0, 2.5, 0.0][axis]
            });
            let life = 0.5 + self.random() * 0.6;
            let spin = self.random() * std::f32::consts::TAU;
            if self.particles.len() == MAX_PARTICLES {
                self.particles.remove(0);
            }
            self.particles.push(Particle {
                position: std::array::from_fn(|axis| position[axis] as f64 + f64::from(cell[axis])),
                velocity,
                spin,
                layer,
                life,
            });
        }
    }

    /// Moves the bits, which stop on solid blocks, and forgets the old ones.
    pub(crate) fn update(&mut self, dt: f32, block: impl Fn([i64; 3]) -> Option<u8>) {
        let solid = |position: [f64; 3]| {
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

    /// The box instances relative to the center chunk, see players.rs.
    pub(crate) fn instances(&self, center: Coord) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.particles.len() * INSTANCE_BYTES as usize);
        for particle in &self.particles {
            let pivot = [
                (particle.position[0] - (center.0 * CHUNK_SIZE as i64) as f64) as f32,
                particle.position[1] as f32 - SIZE / 2.0,
                (particle.position[2] - (center.1 * CHUNK_SIZE as i64) as f64) as f32,
            ];
            let values = pivot
                .into_iter()
                .chain([particle.spin, 0.0, particle.layer as f32])
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
            particles.instances((0, 0)).len(),
            27 * INSTANCE_BYTES as usize
        );
        particles.update(2.0, ground);
        assert!(particles.particles.is_empty());
        for _ in 0..20 {
            particles.burst([0, 10, 0], 3);
        }
        assert_eq!(particles.particles.len(), MAX_PARTICLES);
    }
}
