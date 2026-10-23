/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashSet;

use bwindow::{KeyCode, Modifiers};

use crate::world::{CHUNK_SIZE, Coord, HEIGHT};

pub(crate) struct Camera {
    pub(crate) position: [f64; 3],
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) keys: HashSet<KeyCode>,
    pub(crate) modifiers: Modifiers,
    pub(crate) flying: bool,
    pub(crate) vertical_velocity: f64,
    pub(crate) grounded: bool,
}

pub(crate) struct View {
    eye: [f32; 3],
    right: [f32; 3],
    up: [f32; 3],
    forward: [f32; 3],
    horizontal: f32,
    vertical: f32,
    size: [f32; 2],
    far: f32,
    fog_end: f32,
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(x, y)| x * y).sum()
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            position: [8.0, 116.0, 8.0],
            yaw: -0.7,
            pitch: -0.35,
            keys: HashSet::new(),
            modifiers: Modifiers::NONE,
            flying: false,
            vertical_velocity: 0.0,
            grounded: false,
        }
    }
}

impl Camera {
    pub(crate) fn chunk(&self) -> Coord {
        (
            (self.position[0] / CHUNK_SIZE as f64).floor() as i64,
            (self.position[2] / CHUNK_SIZE as f64).floor() as i64,
        )
    }

    pub(crate) fn look(&mut self, dx: f64, dy: f64) {
        self.yaw = (self.yaw + dx as f32 * 0.002).rem_euclid(std::f32::consts::TAU);
        self.pitch = (self.pitch - dy as f32 * 0.002).clamp(-1.55, 1.55);
    }

    pub(crate) const fn set_flying(&mut self, flying: bool) {
        self.flying = flying;
        self.vertical_velocity = 0.0;
        self.grounded = false;
    }

    fn supported(&self, solid: &mut impl FnMut([i64; 3]) -> bool) -> bool {
        let feet = self.position[1] - 1.7;
        let y = (feet - 0.002).floor() as i64;
        if ((y + 1) as f64 - feet).abs() > 0.002 {
            return false;
        }
        let min_x = (self.position[0] - 0.3 + 1e-7).floor() as i64;
        let max_x = (self.position[0] + 0.3 - 1e-7).floor() as i64;
        let min_z = (self.position[2] - 0.3 + 1e-7).floor() as i64;
        let max_z = (self.position[2] + 0.3 - 1e-7).floor() as i64;
        (min_z..=max_z).any(|z| (min_x..=max_x).any(|x| solid([x, y, z])))
    }

    pub(crate) fn update(&mut self, dt: f32, mut solid: impl FnMut([i64; 3]) -> bool) {
        let dt = f64::from(dt.clamp(0.0, 0.05));
        let pressed = |key| f32::from(u8::from(self.keys.contains(&key)));
        let forward = pressed(KeyCode::KeyW) - pressed(KeyCode::KeyS);
        let right = pressed(KeyCode::KeyD) - pressed(KeyCode::KeyA);
        let shift = self.modifiers.contains(Modifiers::SHIFT);
        let fast = self.modifiers.contains(Modifiers::CONTROL);
        let space = self.keys.contains(&KeyCode::Space);
        let up = if self.flying {
            pressed(KeyCode::Space) - f32::from(u8::from(shift))
        } else {
            0.0
        };
        let (sin, cos) = self.yaw.sin_cos();
        let movement = [
            sin * forward + cos * right,
            up,
            -cos * forward + sin * right,
        ];
        let length = dot(movement, movement).sqrt().max(1.0);
        let speed = if self.flying {
            if fast { 240.0 } else { 18.0 }
        } else if shift {
            1.8
        } else if fast {
            6.5
        } else {
            4.3
        };
        let velocity = movement.map(|delta| f64::from(delta / length * speed));
        if self.flying {
            self.move_by(velocity.map(|v| v * dt), &mut solid);
            return;
        }
        self.grounded = self.supported(&mut solid);
        if space && self.grounded {
            self.vertical_velocity = 8.0;
            self.grounded = false;
        }
        let ticks = (dt / 0.01).ceil().max(1.0) as usize;
        let step = dt / ticks as f64;
        for _ in 0..ticks {
            let previous = self.vertical_velocity;
            self.vertical_velocity = (self.vertical_velocity - 24.0 * step).max(-50.0);
            let dy = (previous + self.vertical_velocity) * 0.5 * step;
            let hit = self.move_by([velocity[0] * step, dy, velocity[2] * step], &mut solid);
            if hit[1] {
                self.vertical_velocity = 0.0;
            }
        }
        self.grounded = self.supported(&mut solid);
    }

    fn move_by(
        &mut self,
        movement: [f64; 3],
        solid: &mut impl FnMut([i64; 3]) -> bool,
    ) -> [bool; 3] {
        let mut hit = [false; 3];
        let steps = (movement.iter().fold(0.0f64, |a, b| a.max(b.abs())) / 0.25)
            .ceil()
            .max(1.0) as usize;
        let lower = [-0.3, -1.7, -0.3];
        let upper = [0.3, 0.1, 0.3];
        for _ in 0..steps {
            for axis in [0, 2, 1] {
                let delta = movement[axis] / steps as f64;
                if delta == 0.0 {
                    continue;
                }
                let mut next = self.position;
                next[axis] += delta;
                let min: [i64; 3] =
                    std::array::from_fn(|a| (next[a] + lower[a] + 1e-7).floor() as i64);
                let max: [i64; 3] =
                    std::array::from_fn(|a| (next[a] + upper[a] - 1e-7).floor() as i64);
                for z in min[2]..=max[2] {
                    for y in min[1]..=max[1] {
                        for x in min[0]..=max[0] {
                            let block = [x, y, z];
                            if solid(block) {
                                next[axis] = if delta > 0.0 {
                                    next[axis].min(block[axis] as f64 - upper[axis])
                                } else {
                                    next[axis].max((block[axis] + 1) as f64 - lower[axis])
                                };
                            }
                        }
                    }
                }
                hit[axis] |= (next[axis] - self.position[axis] - delta).abs() > 1e-7;
                self.position[axis] = if delta > 0.0 {
                    next[axis].max(self.position[axis])
                } else {
                    next[axis].min(self.position[axis])
                };
            }
        }
        hit
    }

    pub(crate) fn view(&self, width: u32, height: u32, radius: i64) -> View {
        let (sin, cos) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let center = self.chunk();
        let vertical = 1.0 / (75.0f32.to_radians() * 0.5).tan();
        View {
            eye: [
                (self.position[0] - center.0 as f64 * CHUNK_SIZE as f64) as f32,
                self.position[1] as f32,
                (self.position[2] - center.1 as f64 * CHUNK_SIZE as f64) as f32,
            ],
            right: [cos, 0.0, sin],
            up: [-sin * sp, cp, cos * sp],
            forward: [sin * cp, sp, -cos * cp],
            horizontal: vertical * height as f32 / width as f32,
            vertical,
            size: [width as f32, height as f32],
            far: radius as f32 * CHUNK_SIZE as f32 * 3.0,
            // Hide the circle's stepped edge even at a corner of the player's chunk.
            fog_end: (radius as f32 - 1.5) * CHUNK_SIZE as f32,
        }
    }
}

impl View {
    pub(crate) fn bytes(&self, time: f32, daylight: f32, eye_material: Option<u8>) -> [u8; 96] {
        // environment.x distinguishes air, water, and lava without changing the uniform layout.
        let medium = match eye_material {
            Some(crate::world::WATER) => 1.0,
            Some(crate::world::LAVA) => 2.0,
            _ => 0.0,
        };
        let mut bytes = [0; 96];
        for (row, values) in bytes.as_chunks_mut::<16>().0.iter_mut().zip([
            [self.eye[0], self.eye[1], self.eye[2], 0.05],
            [self.right[0], self.right[1], self.right[2], self.horizontal],
            [self.up[0], self.up[1], self.up[2], self.vertical],
            [self.forward[0], self.forward[1], self.forward[2], self.far],
            [
                self.size[0],
                self.size[1],
                self.fog_end * 0.75,
                self.fog_end,
            ],
            [medium, time, daylight, 0.0],
        ]) {
            for (word, value) in row.as_chunks_mut::<4>().0.iter_mut().zip(values) {
                *word = value.to_ne_bytes();
            }
        }
        bytes
    }

    pub(crate) fn visible(&self, offset: [f32; 3]) -> bool {
        self.visible_height(offset, HEIGHT as f32)
    }

    pub(crate) fn section_visible(&self, mut offset: [f32; 3], section: usize) -> bool {
        offset[1] = (section * crate::world::SECTION_SIZE) as f32;
        self.visible_height(offset, crate::world::SECTION_SIZE as f32)
    }

    fn visible_height(&self, offset: [f32; 3], height: f32) -> bool {
        let mut outside = [true; 6];
        for corner in 0..8 {
            let p = std::array::from_fn(|axis| {
                offset[axis]
                    + if corner & (1 << axis) == 0 {
                        0.0
                    } else if axis == 1 {
                        height
                    } else {
                        CHUNK_SIZE as f32
                    }
                    - self.eye[axis]
            });
            let x = dot(p, self.right) * self.horizontal;
            let y = dot(p, self.up) * self.vertical;
            let z = dot(p, self.forward);
            for (all, is_outside) in
                outside
                    .iter_mut()
                    .zip([x < -z, x > z, y < -z, y > z, z < 0.05, z > self.far])
            {
                *all &= is_outside;
            }
        }
        !outside.into_iter().any(|value| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liquid_fog_uses_the_eye_material_instead_of_sea_level() {
        for height in [20.0, 100.0] {
            let camera = Camera {
                position: [0.5, height, 0.5],
                ..Camera::default()
            };
            let view = camera.view(1280, 800, 24);
            for (material, expected) in [
                (None, 0.0),
                (Some(0), 0.0),
                (Some(3), 0.0),
                (Some(crate::world::WATER), 1.0),
                (Some(crate::world::LAVA), 2.0),
            ] {
                for daylight in [0.035, 1.0] {
                    let bytes = view.bytes(0.0, daylight, material);
                    assert_eq!(
                        f32::from_ne_bytes(bytes[80..84].try_into().unwrap()),
                        expected
                    );
                    assert_eq!(
                        f32::from_ne_bytes(bytes[88..92].try_into().unwrap()),
                        daylight
                    );
                }
            }
        }
    }

    #[test]
    fn walking_falls_lands_and_repeats_jumps_while_space_is_held() {
        let mut camera = Camera {
            position: [0.5, 10.0, 0.5],
            ..Camera::default()
        };
        for _ in 0..120 {
            camera.update(1.0 / 60.0, |p| p[1] < 1);
        }
        assert!((camera.position[1] - 2.7).abs() < 0.0001);
        assert!(camera.grounded);
        assert_eq!(camera.vertical_velocity, 0.0);
        camera.keys.insert(KeyCode::Space);
        let mut peak = camera.position[1];
        let mut jumps = 0;
        for _ in 0..120 {
            let previous = camera.vertical_velocity;
            camera.update(1.0 / 60.0, |p| p[1] < 1);
            if previous <= 0.0 && camera.vertical_velocity > 0.0 {
                jumps += 1;
            }
            peak = peak.max(camera.position[1]);
        }
        assert!((4.0..4.05).contains(&peak));
        assert!(jumps >= 3);
        camera.keys.remove(&KeyCode::Space);
        for _ in 0..120 {
            camera.update(1.0 / 60.0, |p| p[1] < 1);
        }
        assert!((camera.position[1] - 2.7).abs() < 0.0001);
        assert_eq!(camera.vertical_velocity, 0.0);
    }

    #[test]
    fn walking_leaves_ledges_and_ceiling_hits_stop_jumps() {
        let mut camera = Camera {
            position: [0.5, 2.7, 0.5],
            yaw: std::f32::consts::FRAC_PI_2,
            ..Camera::default()
        };
        camera.keys.insert(KeyCode::KeyW);
        for _ in 0..30 {
            camera.update(1.0 / 60.0, |p| p[0] < 1 && p[1] < 1);
        }
        assert!(camera.position[0] > 2.0);
        assert!(camera.position[1] < 2.0);
        assert!(!camera.grounded);
        let mut camera = Camera {
            position: [0.5, 2.7, 0.5],
            ..Camera::default()
        };
        camera.keys.insert(KeyCode::Space);
        for _ in 0..60 {
            camera.update(1.0 / 60.0, |p| p[1] < 1 || p[1] == 3);
            assert!(camera.position[1] <= 2.9 + 1e-7);
        }
        camera.keys.remove(&KeyCode::Space);
        for _ in 0..60 {
            camera.update(1.0 / 60.0, |p| p[1] < 1 || p[1] == 3);
        }
        assert!((camera.position[1] - 2.7).abs() < 0.0001);
    }

    #[test]
    fn holding_space_and_forward_climbs_one_block_steps() {
        let mut camera = Camera {
            position: [0.5, 2.7, 0.5],
            yaw: std::f32::consts::FRAC_PI_2,
            ..Camera::default()
        };
        camera.keys.extend([KeyCode::Space, KeyCode::KeyW]);
        for _ in 0..240 {
            camera.update(1.0 / 60.0, |p| p[1] < 1 + p[0].div_euclid(2).clamp(0, 4));
        }
        assert!(camera.position[0] > 8.3);
        assert!(camera.position[1] >= 6.7 - 1e-7);
    }

    #[test]
    fn flight_toggle_clears_fall_velocity_and_air_jumps_are_blocked() {
        let mut camera = Camera {
            position: [0.5, 10.0, 0.5],
            ..Camera::default()
        };
        camera.update(0.05, |_| false);
        assert!(camera.vertical_velocity < 0.0);
        camera.keys.insert(KeyCode::Space);
        camera.update(0.05, |_| false);
        assert!(camera.vertical_velocity < -2.0);
        camera.keys.clear();
        camera.set_flying(true);
        let position = camera.position;
        camera.update(0.05, |_| false);
        assert_eq!(camera.position, position);
        assert_eq!(camera.vertical_velocity, 0.0);
        camera.set_flying(false);
        camera.update(0.05, |_| false);
        assert!(camera.position[1] < position[1]);
    }

    #[test]
    fn fast_flight_cannot_tunnel_through_placed_blocks_and_removed_floors_allow_descent() {
        let mut camera = Camera {
            position: [0.5, 12.0, 0.5],
            flying: true,
            yaw: 0.0,
            ..Camera::default()
        };
        camera.keys.insert(KeyCode::KeyW);
        camera.modifiers = Modifiers::CONTROL;
        camera.update(0.05, |p| p[2] == -2);
        assert!((camera.position[2] + 0.7).abs() < 0.0001);
        camera.keys.clear();
        camera.modifiers = Modifiers::SHIFT;
        camera.update(0.05, |p| p[1] < 10);
        assert!((camera.position[1] - 11.7).abs() < 0.0001);
        camera.update(0.05, |p| p[1] < 3);
        assert!(camera.position[1] < 11.0);
    }

    #[test]
    fn negative_coordinates_and_rebasing() {
        let camera = Camera {
            position: [-0.1, 52.0, -16.1],
            ..Default::default()
        };
        assert_eq!(camera.chunk(), (-1, -2));
        let view = camera.view(1280, 800, 16);
        assert!((view.eye[0] - 15.9).abs() < 0.001);
        assert!((view.eye[2] - 15.9).abs() < 0.001);
    }

    #[test]
    fn movement_is_normalized_and_keys_can_be_cleared() {
        let mut straight = Camera {
            flying: true,
            ..Camera::default()
        };
        let mut diagonal = Camera {
            flying: true,
            ..Camera::default()
        };
        straight.keys.insert(KeyCode::KeyW);
        diagonal.keys.extend([KeyCode::KeyW, KeyCode::KeyD]);
        let start = straight.position;
        straight.update(0.05, |_| false);
        diagonal.update(0.05, |_| false);
        let distance = |c: &Camera| {
            c.position
                .into_iter()
                .zip(start)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        assert!((distance(&straight) - distance(&diagonal)).abs() < 0.00001);
        diagonal.keys.clear();
        let stopped = diagonal.position;
        diagonal.update(0.05, |_| false);
        assert_eq!(diagonal.position, stopped);
    }

    #[test]
    fn descending_stops_at_the_seabed() {
        let (x, z) = (-32..32)
            .flat_map(|z| (-32..32).map(move |x| (x * 16, z * 16)))
            .find(|&(x, z)| {
                crate::world::ground_height(x, z) < f64::from(crate::world::SEA_LEVEL) - 4.0
            })
            .unwrap();
        let ground = crate::world::ground_height(x, z);
        let mut camera = Camera {
            position: [x as f64, 116.0, z as f64],
            flying: true,
            ..Default::default()
        };
        camera.modifiers = Modifiers::SHIFT;
        for _ in 0..200 {
            camera.update(0.05, |p| (p[1] as f64) < ground);
        }
        assert!((camera.position[1] - ground - 1.7).abs() < 0.0001);
    }

    #[test]
    fn culling_and_pitch_limits() {
        let mut camera = Camera {
            yaw: 0.0,
            pitch: 0.0,
            ..Default::default()
        };
        let view = camera.view(1280, 800, 16);
        assert!(view.visible([0.0, 0.0, -32.0]));
        assert!(!view.visible([0.0, 0.0, 32.0]));
        assert!(!view.visible([2000.0, 0.0, -32.0]));
        camera.look(100000.0, 100000.0);
        assert!(camera.pitch.abs() <= 1.55);
        assert!(camera.yaw >= 0.0 && camera.yaw < std::f32::consts::TAU);
    }
}
