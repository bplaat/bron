/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The other players in the world, drawn as boxes with the side view sprites of the Kenney
//! voxel pack's male character. The texture coordinates of a box follow its side, so its front
//! and back stretch the sprite's face and back edges.

use alloc::collections::BTreeMap;
#[cfg(all(test, feature = "std"))]
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::f32::consts::{PI, TAU};

use crate::camera::View;
#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::protocol::{ClientId, MAX_PLAYERS, PlayerInfo, Pose};
use crate::world::{self, Bounds, CHUNK_SIZE, Coord};

/// The body parts, in the order of their sprites.
pub const PARTS: [&str; 4] = ["head", "body", "arm", "leg"];
// The width along the facing direction and the height of each part's sprite in pixels.
const SPRITES: [[f32; 2]; PARTS.len()] = [[64.0, 64.0], [44.0, 59.0], [28.0, 66.0], [28.0, 56.0]];
// The world size of a sprite pixel, which makes the man as tall as the player's box.
const PIXEL: f32 = 0.01;
// The eye above the feet, as in the engine's physics.
const EYE_HEIGHT: f64 = 1.7;
const BODY_BOXES: usize = 6;
const BOXES: usize = BODY_BOXES + 2;
// The held block is smaller than a world block and follows the right arm.
const HELD_SIZE: f32 = 0.36;
const FULL: &[Bounds] = &[[[0.0; 3], [1.0; 3]]];
/// The size of a box instance: pivot, yaw, pitch, texture, size, and offset.
/// Character textures use their array layer. Held blocks use -(1 + material + box_index * 128).
pub const INSTANCE_BYTES: u64 = 12 * 4;
/// The most box instances, of all players.
pub const MAX_INSTANCES: usize = MAX_PLAYERS * BOXES;
// Farther moves than this jump instead of gliding.
const TELEPORT_DISTANCE: f64 = 8.0;

// Another player as it is drawn.
struct Other {
    name: String,
    pose: Pose,
    target: Pose,
    held: u8,
    // The walk cycle and how far the limbs swing.
    phase: f32,
    stride: f32,
}

/// The other players in the world.
#[derive(Default)]
pub struct Others {
    players: BTreeMap<ClientId, Other>,
}

// The difference between two angles, from -PI to PI.
fn angle_difference(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

impl Others {
    /// Adds, moves, and removes the players of a state.
    pub fn update(&mut self, players: &[PlayerInfo]) {
        self.players
            .retain(|id, _| players.iter().any(|player| player.id == *id));
        for player in players {
            let other = self.players.entry(player.id).or_insert_with(|| Other {
                name: String::new(),
                pose: player.pose,
                target: player.pose,
                held: player.held,
                phase: 0.0,
                stride: 0.0,
            });
            other.name.clone_from(&player.name);
            other.target = player.pose;
            other.held = player.held;
        }
    }

    /// Glides towards the received poses, which arrive twenty times a second.
    pub fn animate(&mut self, dt: f32) {
        let blend = 1.0 - (-dt * 15.0).exp();
        for other in self.players.values_mut() {
            let [x, y, z] = other.pose.position;
            let [tx, ty, tz] = other.target.position;
            let distance = ((tx - x) * (tx - x) + (ty - y) * (ty - y) + (tz - z) * (tz - z)).sqrt();
            if distance > TELEPORT_DISTANCE {
                other.pose = other.target;
                continue;
            }
            let walked = (tx - x).hypot(tz - z) as f32 * blend;
            for (value, target) in other.pose.position.iter_mut().zip(other.target.position) {
                *value += (target - *value) * f64::from(blend);
            }
            other.pose.yaw += angle_difference(other.pose.yaw, other.target.yaw) * blend;
            other.pose.pitch += (other.target.pitch - other.pose.pitch) * blend;
            other.phase = (other.phase + walked * 4.0) % TAU;
            let speed = if dt > 0.0 { walked / dt } else { 0.0 };
            other.stride += ((speed / 4.0).min(1.0) - other.stride) * blend;
        }
    }

    /// The box instances relative to the center chunk.
    pub fn instances(&self, center: Coord, first_layer: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.players.len() * BOXES * INSTANCE_BYTES as usize);
        for other in self.players.values().take(MAX_PLAYERS) {
            for part in other.boxes() {
                let texture = match part.texture {
                    Texture::Character(sprite) => (first_layer + sprite as u32) as f32,
                    Texture::Block(material, index) => {
                        -(1.0 + f32::from(material) + index as f32 * 128.0)
                    }
                };
                let feet = [
                    (other.pose.position[0] - (center.0 * CHUNK_SIZE as i64) as f64) as f32,
                    (other.pose.position[1] - EYE_HEIGHT) as f32,
                    (other.pose.position[2] - (center.1 * CHUNK_SIZE as i64) as f64) as f32,
                ];
                let (sin, cos) = other.pose.yaw.sin_cos();
                let pivot = [
                    feet[0] + cos * part.pivot[0],
                    feet[1] + part.pivot[1],
                    feet[2] + sin * part.pivot[0],
                ];
                let values = pivot
                    .into_iter()
                    .chain([other.pose.yaw, part.pitch, texture])
                    .chain(part.size)
                    .chain(part.offset);
                for value in values {
                    bytes.extend_from_slice(&value.to_ne_bytes());
                }
            }
        }
        bytes
    }

    /// Where the name tags go on the screen, in physical pixels.
    pub fn name_tags(&self, view: &View, center: Coord) -> Vec<(&str, [f32; 2])> {
        self.players
            .values()
            .filter_map(|other| {
                let top = height() + 0.35;
                let point = [
                    (other.pose.position[0] - (center.0 * CHUNK_SIZE as i64) as f64) as f32,
                    (other.pose.position[1] - EYE_HEIGHT) as f32 + top,
                    (other.pose.position[2] - (center.1 * CHUNK_SIZE as i64) as f64) as f32,
                ];
                Some((other.name.as_str(), view.project(point)?))
            })
            .collect()
    }
}

enum Texture {
    Character(usize),
    Block(u8, usize),
}

struct Part {
    texture: Texture,
    // The rotation point, sideways and up from the feet.
    pivot: [f32; 2],
    pitch: f32,
    // The box's width sideways, height, and depth along the facing direction, and its
    // corner relative to the pivot.
    size: [f32; 3],
    offset: [f32; 3],
}

// The depth and height of the parts in the world.
fn sizes() -> [[f32; 2]; PARTS.len()] {
    SPRITES.map(|[depth, height]| [depth * PIXEL, height * PIXEL])
}

fn height() -> f32 {
    let [head, body, _, leg] = sizes();
    leg[1] + body[1] + head[1]
}

impl Other {
    fn boxes(&self) -> impl Iterator<Item = Part> + '_ {
        let [head, body, arm, leg] = sizes();
        let width = leg[0] * 2.0;
        let swing = self.phase.sin() * 0.8 * self.stride;
        let hip = leg[1];
        let neck = leg[1] + body[1];
        let shoulder = neck - arm[0] / 2.0;
        let hanging =
            |sprite: usize, [depth, height]: [f32; 2], side: f32, y: f32, pitch: f32| Part {
                texture: Texture::Character(sprite),
                pivot: [side, y],
                pitch,
                size: [depth, height, depth],
                offset: [-depth / 2.0, -height, -depth / 2.0],
            };
        let held_pitch = 0.5 + self.pose.pitch.clamp(-1.2, 1.2) * 0.3 + swing * 0.35;
        let parts = [
            Part {
                texture: Texture::Character(0),
                pivot: [0.0, neck],
                pitch: self.pose.pitch.clamp(-1.2, 1.2),
                size: [head[0].min(head[1]), head[1], head[0]],
                offset: [-head[0].min(head[1]) / 2.0, 0.0, -head[0] / 2.0],
            },
            Part {
                texture: Texture::Character(1),
                pivot: [0.0, hip],
                pitch: 0.0,
                size: [width, body[1], body[0]],
                offset: [-width / 2.0, 0.0, -body[0] / 2.0],
            },
            hanging(2, arm, -(width + arm[0]) / 2.0, shoulder, -swing),
            hanging(2, arm, (width + arm[0]) / 2.0, shoulder, held_pitch),
            hanging(3, leg, -leg[0] / 2.0, hip, swing),
            hanging(3, leg, leg[0] / 2.0, hip, -swing),
        ];
        let boxes = world::boxes(self.held);
        parts.into_iter().chain(
            if boxes.is_empty() { FULL } else { boxes }
                .iter()
                .enumerate()
                .map(move |(index, bounds)| {
                    let mut bounds = bounds.map(|corner| corner.map(|v| v as f32));
                    if world::is_sprite(self.held) {
                        bounds[0][2] = 0.5;
                        bounds[1][2] = 0.5;
                    }
                    Part {
                        texture: Texture::Block(self.held, index),
                        pivot: [(width + arm[0]) / 2.0, shoulder],
                        pitch: held_pitch,
                        size: core::array::from_fn(|a| (bounds[1][a] - bounds[0][a]) * HELD_SIZE),
                        offset: core::array::from_fn(|a| {
                            [-HELD_SIZE / 2.0, -arm[1] - HELD_SIZE / 2.0 + 0.04, 0.06][a]
                                + bounds[0][a] * HELD_SIZE
                        }),
                    }
                }),
        )
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;
    use crate::camera::Camera;

    fn info(id: ClientId, x: f64) -> PlayerInfo {
        PlayerInfo {
            id,
            name: format!("Player {id}"),
            held: 1,
            pose: Pose {
                position: [x, 71.7, 8.0],
                yaw: 0.0,
                pitch: 0.0,
            },
        }
    }

    #[test]
    fn sprite_sizes_match_the_kenney_parts() {
        for (part, [width, height]) in PARTS.iter().zip(SPRITES) {
            let path = format!("../plaatcraft/assets/characters/male_{part}.png");
            let image = image::decode(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(
                [image.width() as f32, image.height() as f32],
                [width, height],
                "{path}"
            );
        }
    }

    #[test]
    fn players_glide_walk_and_leave() {
        let mut others = Others::default();
        others.update(&[info(1, 8.0), info(2, 0.0)]);
        others.update(&[info(1, 9.0)]);
        assert_eq!(others.players.len(), 1);
        for _ in 0..10 {
            others.animate(1.0 / 60.0);
        }
        let other = &others.players[&1];
        assert!(other.pose.position[0] > 8.5 && other.pose.position[0] < 9.0);
        assert!(other.stride > 0.0 && other.phase > 0.0);
        others.update(&[info(1, 100.0)]);
        others.animate(1.0 / 60.0);
        assert_eq!(others.players[&1].pose.position[0], 100.0);
        assert!((angle_difference(0.1, TAU - 0.1) + 0.2).abs() < 0.0001);
    }

    #[test]
    fn selected_blocks_follow_the_right_hand_with_their_material_and_shape() {
        let mut others = Others::default();
        for &(material, _) in world::MATERIALS {
            let mut player = info(1, 8.0);
            player.held = material;
            player.pose.pitch = 0.6;
            others.update(&[player]);
            let other = others.players.get_mut(&1).unwrap();
            other.phase = 1.0;
            other.stride = 0.8;
            let parts: Vec<_> = other.boxes().collect();
            let arm = &parts[3];
            let boxes = world::boxes(material);
            assert_eq!(parts.len(), BODY_BOXES + boxes.len().max(1));
            for (index, held) in parts[BODY_BOXES..].iter().enumerate() {
                assert!(
                    matches!(held.texture, Texture::Block(m, i) if m == material && i == index)
                );
                assert_eq!(held.pivot, arm.pivot);
                assert_eq!(held.pitch, arm.pitch);
                assert!(
                    held.size
                        .iter()
                        .all(|&size| (0.0..=HELD_SIZE).contains(&size))
                );
                assert!((held.offset[1] + sizes()[2][1]).abs() <= HELD_SIZE);
                if world::is_sprite(material) {
                    assert_eq!(held.size[2], 0.0);
                } else if let Some(bounds) = boxes.get(index) {
                    assert_eq!(
                        held.size,
                        core::array::from_fn(|a| {
                            (bounds[1][a] - bounds[0][a]) as f32 * HELD_SIZE
                        })
                    );
                }
            }
            let bytes = others.instances((0, 0), 100);
            assert!(bytes.len() <= BOXES * INSTANCE_BYTES as usize);
            for (index, chunk) in bytes.as_chunks::<48>().0[BODY_BOXES..].iter().enumerate() {
                let texture = f32::from_ne_bytes(chunk[20..24].try_into().unwrap());
                assert_eq!(texture, -(1.0 + f32::from(material) + index as f32 * 128.0));
            }
        }
    }

    #[test]
    fn boxes_stand_on_the_feet_and_tags_float_above() {
        let mut others = Others::default();
        others.update(&[info(0, 8.0)]);
        let bytes = others.instances((0, 0), 100);
        assert_eq!(bytes.len(), (BODY_BOXES + 1) * INSTANCE_BYTES as usize);
        let floats: Vec<_> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_ne_bytes(*b))
            .collect();
        // The legs hang from the hips down to the feet.
        let leg = &floats[5 * 12..];
        assert!((leg[1] + leg[10] - 70.0).abs() < 0.001);
        assert_eq!(leg[5], 103.0);
        assert!((height() - 1.79).abs() < 0.001);

        let camera = Camera {
            position: [8.0, 71.7, 16.0],
            yaw: 0.0,
            pitch: 0.0,
            ..Camera::default()
        };
        let view = camera.view(800, 600, 8);
        let tags = others.name_tags(&view, camera.chunk());
        assert_eq!(tags.len(), 1);
        assert!(tags[0].1[1] < 300.0);
    }
}
