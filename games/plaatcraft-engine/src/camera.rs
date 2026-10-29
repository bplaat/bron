/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The first-person player: walking, swimming, and flying physics and the view frustum.

use serde::{Deserialize, Serialize};

#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::world::{self, Bounds, CHUNK_SIZE, Coord, HEIGHT};

const SWIM_SPEED: f64 = 3.5;
const SINK_SPEED: f64 = 2.0;
// The player's box around the eye.
const LOWER: [f64; 3] = [-0.3, -1.7, -0.3];
const UPPER: [f64; 3] = [0.3, 0.1, 0.3];
// How far ahead walking looks for a step to jump on.
const STEP_LOOKAHEAD: f64 = 0.3;
const FULL: &[Bounds] = &[[[0.0; 3], [1.0; 3]]];

// A box inside the block at a cell, in world coordinates.
fn offset_bounds(bounds: &Bounds, cell: [i64; 3]) -> Bounds {
    bounds.map(|corner| core::array::from_fn(|a| cell[a] as f64 + corner[a]))
}

// Whether two boxes overlap by more than touching.
fn intersects(a: Bounds, b: Bounds) -> bool {
    (0..3).all(|axis| a[0][axis] < b[1][axis] - 1e-7 && a[1][axis] > b[0][axis] + 1e-7)
}

// Calls `f` with every block cell a box touches.
fn for_cells(bounds: Bounds, mut f: impl FnMut([i64; 3])) {
    let [min, max] = [0, 1].map(|end| {
        let nudge = if end == 0 { 1e-7 } else { -1e-7 };
        bounds[end].map(|value| (value + nudge).floor() as i64)
    });
    for z in min[2]..=max[2] {
        for y in min[1]..=max[1] {
            for x in min[0]..=max[0] {
                f([x, y, z]);
            }
        }
    }
}

/// The held movement controls, set by the platform before each update.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Input {
    /// Digital movement like WASD or a D-pad, x to the right and y forward.
    pub direction: [f32; 2],
    /// An analog joystick, x to the right and y forward, pushing it far sprints.
    pub stick: [f32; 2],
    /// Jumps, swims up, and flies up, like Space.
    pub jump: bool,
    /// Sneaks and flies down, like Shift.
    pub sneak: bool,
    /// Runs and flies faster, like Ctrl.
    pub fast: bool,
}

/// The player's position, orientation, and movement state.
#[derive(Clone, Copy, Debug)]
pub struct Camera {
    /// The eye position in world coordinates.
    pub position: [f64; 3],
    /// The horizontal rotation in radians, 0 looks to negative z.
    pub yaw: f32,
    /// The vertical rotation in radians, positive looks up.
    pub pitch: f32,
    /// The held movement controls.
    pub input: Input,
    /// Whether the body is in water or lava, set before each update.
    pub in_liquid: bool,
    /// Whether the player flies instead of walking, see [`Camera::set_flying`].
    pub flying: bool,
    /// Whether walking into a one block step jumps on it, like in Minecraft.
    pub auto_jump: bool,
    /// The vertical speed while walking or swimming.
    pub vertical_velocity: f64,
    /// Whether the player stands on a block.
    pub grounded: bool,
}

/// The camera basis and frustum for rendering and culling, relative to the
/// player's chunk.
pub struct View {
    /// The eye position relative to the player's chunk.
    pub eye: [f32; 3],
    /// The unit vector to the right of the view.
    pub right: [f32; 3],
    /// The unit vector up from the view.
    pub up: [f32; 3],
    /// The unit vector the view looks along.
    pub forward: [f32; 3],
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
            input: Input::default(),
            in_liquid: false,
            flying: false,
            auto_jump: false,
            vertical_velocity: 0.0,
            grounded: false,
        }
    }
}

impl Camera {
    /// The chunk the player is in.
    pub fn chunk(&self) -> Coord {
        (
            (self.position[0] / CHUNK_SIZE as f64).floor() as i64,
            (self.position[2] / CHUNK_SIZE as f64).floor() as i64,
        )
    }

    /// Turns the view like a mouse moving by `dx` and `dy` pixels.
    pub fn look(&mut self, dx: f64, dy: f64) {
        self.yaw = (self.yaw + dx as f32 * 0.002).rem_euclid(core::f32::consts::TAU);
        self.pitch = (self.pitch - dy as f32 * 0.002).clamp(-1.55, 1.55);
    }

    /// The unit vector the player looks along.
    pub fn direction(&self) -> [f64; 3] {
        let (sin, cos) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        [f64::from(sin * cp), f64::from(sp), f64::from(-cos * cp)]
    }

    /// Starts or stops flying.
    pub const fn set_flying(&mut self, flying: bool) {
        self.flying = flying;
        self.vertical_velocity = 0.0;
        self.grounded = false;
    }

    fn supported(&self, solid: &mut impl FnMut([i64; 3]) -> &'static [Bounds]) -> bool {
        let feet = self.position[1] + LOWER[1];
        let y = (feet - 0.002).floor() as i64;
        let [lower, upper] = self.body([0.0; 3]);
        (lower[2].floor() as i64..=(upper[2] - 1e-7).floor() as i64).any(|z| {
            (lower[0].floor() as i64..=(upper[0] - 1e-7).floor() as i64).any(|x| {
                solid([x, y, z]).iter().any(|bounds| {
                    let world = offset_bounds(bounds, [x, y, z]);
                    (world[1][1] - feet).abs() <= 0.002
                        && [0, 2].into_iter().all(|axis| {
                            lower[axis] < world[1][axis] - 1e-7
                                && upper[axis] > world[0][axis] + 1e-7
                        })
                })
            })
        })
    }

    /// Moves the player by the held input for `dt` seconds through the blocks, which
    /// `block` looks up. Unknown blocks collide, so the player never falls into
    /// unloaded chunks. Clients and servers both move players with it.
    pub fn tick(&mut self, dt: f32, mut block: impl FnMut([i64; 3]) -> Option<u8>) {
        // The feet or waist in water or lava make the player swim.
        let [x, y, z] = self.position;
        self.in_liquid = [0.2, 0.9].into_iter().any(|offset| {
            let position = [
                x.floor() as i64,
                (y - 1.7 + offset).floor() as i64,
                z.floor() as i64,
            ];
            block(position).is_some_and(|material| world::fluid(material).is_some())
        });
        self.update_in(dt, |position| match block(position) {
            Some(material) if position[1] >= 0 => world::boxes(material),
            _ => FULL,
        });
    }

    /// Moves the player by the held input for `dt` seconds, `solid` tells which
    /// blocks collide.
    pub fn update(&mut self, dt: f32, mut solid: impl FnMut([i64; 3]) -> bool) {
        self.update_in(dt, |position| if solid(position) { FULL } else { &[] });
    }

    // Moves the player by the held input for `dt` seconds through the boxes of the blocks.
    fn update_in(&mut self, dt: f32, mut solid: impl FnMut([i64; 3]) -> &'static [Bounds]) {
        let dt = f64::from(dt.clamp(0.0, 0.05));
        let Input {
            direction,
            stick,
            jump: space,
            sneak: shift,
            ..
        } = self.input;
        let forward = (direction[1] + stick[1]).clamp(-1.0, 1.0);
        let right = (direction[0] + stick[0]).clamp(-1.0, 1.0);
        // Pushing the joystick far sprints while walking, like in the Welpen games.
        let fast = self.input.fast || !self.flying && stick[0].hypot(stick[1]) > 0.75;
        let up = if self.flying {
            f32::from(u8::from(space)) - f32::from(u8::from(shift))
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
            13.0
        } else {
            6.5
        };
        let velocity = movement.map(|delta| f64::from(delta / length * speed));
        if self.flying {
            self.move_by(velocity.map(|v| v * dt), &mut solid);
            return;
        }
        self.grounded = self.supported(&mut solid);
        // Half block steps like slabs and stairs are walked up, full ones jumped on.
        let ahead = self.ahead(velocity);
        if self.grounded
            && ahead.is_some_and(|[dx, dz]| {
                self.overlaps([dx, 0.0, dz], &mut solid)
                    && !self.overlaps([0.0, 0.5, 0.0], &mut solid)
                    && !self.overlaps([dx, 0.5, dz], &mut solid)
            })
        {
            self.position[1] += 0.5;
        }
        let climb = self.auto_jump
            && !shift
            && ahead.is_some_and(|[dx, dz]| {
                self.overlaps([dx, 0.0, dz], &mut solid)
                    && !self.overlaps([0.0, 1.0, 0.0], &mut solid)
                    && !self.overlaps([dx, 1.0, dz], &mut solid)
            });
        if (space || climb) && self.grounded && !self.in_liquid {
            self.vertical_velocity = 8.0;
            self.grounded = false;
        }
        let ticks = (dt / 0.01).ceil().max(1.0) as usize;
        let step = dt / ticks as f64;
        for _ in 0..ticks {
            let previous = self.vertical_velocity;
            self.vertical_velocity = if self.in_liquid {
                // Liquids slowly sink the player, holding Space swims up.
                let target = if space { SWIM_SPEED } else { -SINK_SPEED };
                self.vertical_velocity + (target - self.vertical_velocity) * (step * 6.0).min(1.0)
            } else {
                (self.vertical_velocity - 24.0 * step).max(-50.0)
            };
            let dy = (previous + self.vertical_velocity) * 0.5 * step;
            let hit = self.move_by([velocity[0] * step, dy, velocity[2] * step], &mut solid);
            if hit[1] {
                self.vertical_velocity = 0.0;
            }
        }
        self.grounded = self.supported(&mut solid);
    }

    // The player's box shifted by `offset`.
    fn body(&self, offset: [f64; 3]) -> Bounds {
        [LOWER, UPPER]
            .map(|corner| core::array::from_fn(|a| self.position[a] + offset[a] + corner[a]))
    }

    // Whether the box shifted by `offset` overlaps the boxes of the blocks.
    fn overlaps(
        &self,
        offset: [f64; 3],
        solid: &mut impl FnMut([i64; 3]) -> &'static [Bounds],
    ) -> bool {
        let body = self.body(offset);
        let mut found = false;
        for_cells(body, |cell| {
            found |= solid(cell)
                .iter()
                .any(|bounds| intersects(body, offset_bounds(bounds, cell)));
        });
        found
    }

    // Where walking along `velocity` looks for steps, none when standing still.
    fn ahead(&self, velocity: [f64; 3]) -> Option<[f64; 2]> {
        let length = velocity[0].hypot(velocity[2]);
        (length >= 1e-3).then(|| [velocity[0], velocity[2]].map(|v| v / length * STEP_LOOKAHEAD))
    }

    fn move_by(
        &mut self,
        movement: [f64; 3],
        solid: &mut impl FnMut([i64; 3]) -> &'static [Bounds],
    ) -> [bool; 3] {
        let mut hit = [false; 3];
        let steps = (movement.iter().fold(0.0f64, |a, b| a.max(b.abs())) / 0.25)
            .ceil()
            .max(1.0) as usize;
        for _ in 0..steps {
            for axis in [0, 2, 1] {
                let delta = movement[axis] / steps as f64;
                if delta == 0.0 {
                    continue;
                }
                let mut offset = [0.0; 3];
                offset[axis] = delta;
                let body = self.body(offset);
                let mut next = self.position[axis] + delta;
                for_cells(body, |cell| {
                    for bounds in solid(cell) {
                        let world = offset_bounds(bounds, cell);
                        if intersects(body, world) {
                            next = if delta > 0.0 {
                                next.min(world[0][axis] - UPPER[axis])
                            } else {
                                next.max(world[1][axis] - LOWER[axis])
                            };
                        }
                    }
                });
                hit[axis] |= (next - self.position[axis] - delta).abs() > 1e-7;
                self.position[axis] = if delta > 0.0 {
                    next.max(self.position[axis])
                } else {
                    next.min(self.position[axis])
                };
            }
        }
        hit
    }

    /// The view for a `width` by `height` viewport that sees `radius` chunks far.
    pub fn view(&self, width: u32, height: u32, radius: i64) -> View {
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
    /// The camera uniform of the bwebgpu shaders.
    pub fn bytes(&self, time: f32, daylight: f32, eye_material: Option<u8>) -> [u8; 96] {
        // environment.x distinguishes air, water, and lava without changing the uniform layout.
        let medium = match eye_material {
            Some(world::WATER) => 1.0,
            Some(world::LAVA) => 2.0,
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

    /// The position in pixels from the top left of the screen of a point relative to the
    /// player's chunk, when it is in front of the camera.
    pub fn project(&self, point: [f32; 3]) -> Option<[f32; 2]> {
        let relative = [
            point[0] - self.eye[0],
            point[1] - self.eye[1],
            point[2] - self.eye[2],
        ];
        let depth = dot(relative, self.forward);
        if depth < 0.05 {
            return None;
        }
        let x = dot(relative, self.right) * self.horizontal / depth;
        let y = dot(relative, self.up) * self.vertical / depth;
        Some([
            (x + 1.0) * 0.5 * self.size[0],
            (1.0 - y) * 0.5 * self.size[1],
        ])
    }

    /// Whether a chunk at an offset from the player's chunk is in the frustum.
    pub fn visible(&self, offset: [f32; 3]) -> bool {
        self.visible_height(offset, HEIGHT as f32)
    }

    /// Whether a chunk section is in the frustum.
    pub fn section_visible(&self, mut offset: [f32; 3], section: usize) -> bool {
        offset[1] = (section * world::SECTION_SIZE) as f32;
        self.visible_height(offset, world::SECTION_SIZE as f32)
    }

    fn visible_height(&self, offset: [f32; 3], height: f32) -> bool {
        let mut outside = [true; 6];
        for corner in 0..8 {
            let p = core::array::from_fn(|axis| {
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
    fn players_walk_up_stairs_and_stand_on_slabs() {
        let stairs = world::placed_material(90, 0.0, 2.0, false);
        let mut camera = Camera {
            position: [0.5, 2.7, 0.5],
            auto_jump: false,
            ..Camera::default()
        };
        camera.yaw = 0.0;
        camera.pitch = 0.0;
        camera.input.direction = [0.0, 1.0];
        // Stairs rising to the north up a block, a slab on top, and a wall, without auto jump.
        let block = |[_, y, z]: [i64; 3]| {
            Some(match (y, z) {
                (..=0, _) | (1, -4..=-3) | (1..=3, ..=-5) => 3,
                (1, -2) => stairs,
                (2, -4) => 88,
                _ => 0,
            })
        };
        for _ in 0..120 {
            camera.tick(1.0 / 60.0, block);
        }
        assert!(camera.grounded);
        assert!((camera.position[1] - 4.2).abs() < 0.0001);
        assert!((camera.position[2] + 3.7).abs() < 0.0001);
    }

    #[test]
    fn ticks_fall_through_air_swim_in_water_and_stop_at_unknown_blocks() {
        let mut camera = Camera {
            position: [0.5, 70.0, 0.5],
            ..Camera::default()
        };
        // Air above y 60, unknown below.
        let block = |[_, y, _]: [i64; 3]| (y >= 60).then_some(0);
        for _ in 0..40 {
            camera.tick(0.05, block);
        }
        assert!(camera.grounded);
        assert!((camera.position[1] - 61.7).abs() < 0.001);
        camera.tick(0.05, |[_, y, _]| {
            Some(if y < 70 { world::WATER } else { 0 })
        });
        assert!(camera.in_liquid);
    }

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
                (Some(world::WATER), 1.0),
                (Some(world::LAVA), 2.0),
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
        camera.input.jump = true;
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
        camera.input.jump = false;
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
            yaw: core::f32::consts::FRAC_PI_2,
            ..Camera::default()
        };
        camera.input.direction = [0.0, 1.0];
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
        camera.input.jump = true;
        for _ in 0..60 {
            camera.update(1.0 / 60.0, |p| p[1] < 1 || p[1] == 3);
            assert!(camera.position[1] <= 2.9 + 1e-7);
        }
        camera.input.jump = false;
        for _ in 0..60 {
            camera.update(1.0 / 60.0, |p| p[1] < 1 || p[1] == 3);
        }
        assert!((camera.position[1] - 2.7).abs() < 0.0001);
    }

    #[test]
    fn holding_space_and_forward_climbs_one_block_steps() {
        let mut camera = Camera {
            position: [0.5, 2.7, 0.5],
            yaw: core::f32::consts::FRAC_PI_2,
            ..Camera::default()
        };
        camera.input.jump = true;
        camera.input.direction = [0.0, 1.0];
        for _ in 0..240 {
            camera.update(1.0 / 60.0, |p| p[1] < 1 + p[0].div_euclid(2).clamp(0, 4));
        }
        assert!(camera.position[0] > 8.3);
        assert!(camera.position[1] >= 6.7 - 1e-7);
    }

    #[test]
    fn auto_jump_climbs_one_block_steps_but_not_walls_or_while_sneaking() {
        let walk = |auto_jump, sneak, solid: fn([i64; 3]) -> bool| {
            let mut camera = Camera {
                position: [0.5, 2.7, 0.5],
                yaw: core::f32::consts::FRAC_PI_2,
                auto_jump,
                ..Camera::default()
            };
            camera.input.direction = [0.0, 1.0];
            camera.input.sneak = sneak;
            for _ in 0..120 {
                camera.update(1.0 / 60.0, solid);
            }
            camera.position
        };
        let step: fn([i64; 3]) -> bool = |p| p[1] < 1 || p[0] >= 3 && p[1] < 2;
        let wall: fn([i64; 3]) -> bool = |p| p[1] < 1 || p[0] >= 3 && p[1] < 3;
        let climbed = walk(true, false, step);
        assert!(climbed[0] > 4.0);
        assert!((climbed[1] - 3.7).abs() < 0.0001);
        for position in [
            walk(false, false, step),
            walk(true, true, step),
            walk(true, false, wall),
        ] {
            assert!(position[0] < 2.71);
            assert!((position[1] - 2.7).abs() < 0.0001);
        }
    }

    #[test]
    fn flight_toggle_clears_fall_velocity_and_air_jumps_are_blocked() {
        let mut camera = Camera {
            position: [0.5, 10.0, 0.5],
            ..Camera::default()
        };
        camera.update(0.05, |_| false);
        assert!(camera.vertical_velocity < 0.0);
        camera.input.jump = true;
        camera.update(0.05, |_| false);
        assert!(camera.vertical_velocity < -2.0);
        camera.input = Input::default();
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
        camera.input.direction = [0.0, 1.0];
        camera.input.fast = true;
        camera.update(0.05, |p| p[2] == -2);
        assert!((camera.position[2] + 0.7).abs() < 0.0001);
        camera.input = Input {
            sneak: true,
            ..Input::default()
        };
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
    fn movement_is_normalized_and_input_can_be_cleared() {
        let mut straight = Camera {
            flying: true,
            ..Camera::default()
        };
        let mut diagonal = Camera {
            flying: true,
            ..Camera::default()
        };
        straight.input.direction = [0.0, 1.0];
        diagonal.input.direction = [1.0, 1.0];
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
        diagonal.input = Input::default();
        let stopped = diagonal.position;
        diagonal.update(0.05, |_| false);
        assert_eq!(diagonal.position, stopped);
    }

    #[test]
    fn joystick_moves_proportionally_and_sprints_when_pushed_far() {
        let distance = |stick| {
            let mut camera = Camera {
                flying: true,
                input: Input {
                    stick,
                    ..Input::default()
                },
                ..Camera::default()
            };
            let start = camera.position;
            camera.update(0.05, |_| false);
            camera
                .position
                .into_iter()
                .zip(start)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        assert!((distance([0.0, 0.5]) * 2.0 - distance([0.0, 1.0])).abs() < 0.00001);
        assert!((distance([0.6, 0.8]) - distance([0.0, 1.0])).abs() < 0.00001);

        let mut camera = Camera {
            input: Input {
                stick: [0.0, 1.0],
                ..Input::default()
            },
            ..Camera::default()
        };
        let start = camera.position;
        camera.update(0.05, |position| position[1] < 100);
        let sprint = camera.position[2] - start[2];
        camera.input.stick = [0.0, 0.5];
        let start = camera.position;
        camera.update(0.05, |position| position[1] < 100);
        assert!(sprint.abs() > (camera.position[2] - start[2]).abs() * 2.0);
    }

    #[test]
    fn liquids_sink_slowly_and_space_swims_up() {
        let mut camera = Camera {
            position: [0.5, 50.0, 0.5],
            in_liquid: true,
            ..Camera::default()
        };
        for _ in 0..40 {
            camera.update(0.05, |_| false);
        }
        assert!((camera.vertical_velocity + SINK_SPEED).abs() < 0.01);
        let sunk = camera.position[1];
        assert!(sunk < 50.0 && sunk > 50.0 - SINK_SPEED * 2.0);

        camera.input.jump = true;
        for _ in 0..40 {
            camera.update(0.05, |_| false);
        }
        assert!((camera.vertical_velocity - SWIM_SPEED).abs() < 0.01);
        assert!(camera.position[1] > sunk + 4.0);
    }

    #[test]
    fn descending_stops_at_the_seabed() {
        let (x, z) = (-32..32)
            .flat_map(|z| (-32..32).map(move |x| (x * 16, z * 16)))
            .find(|&(x, z)| world::ground_height(x, z) < f64::from(world::SEA_LEVEL) - 4.0)
            .unwrap();
        let ground = world::ground_height(x, z);
        let mut camera = Camera {
            position: [x as f64, 116.0, z as f64],
            flying: true,
            ..Default::default()
        };
        camera.input.sneak = true;
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
        assert!(camera.yaw >= 0.0 && camera.yaw < core::f32::consts::TAU);
    }

    #[test]
    fn points_project_to_screen_pixels() {
        let camera = Camera {
            position: [8.0, 70.0, 8.0],
            yaw: 0.0,
            pitch: 0.0,
            ..Camera::default()
        };
        let view = camera.view(800, 600, 8);
        let [x, y] = view.project([8.0, 70.0, 0.0]).unwrap();
        assert!((x - 400.0).abs() < 0.01 && (y - 300.0).abs() < 0.01);
        let [right, up] = view.project([9.0, 71.0, 0.0]).unwrap();
        assert!(right > 400.0 && up < 300.0);
        assert!(view.project([8.0, 70.0, 16.0]).is_none());
    }
}
