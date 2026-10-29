/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The other players in the world, drawn as boxes with the side view sprites of the Kenney
//! voxel pack's male character. The texture coordinates of a box follow its side, so its front
//! and back stretch the sprite's face and back edges.

use std::collections::BTreeMap;
use std::f32::consts::{PI, TAU};

use plaatcraft_engine::camera::View;
use plaatcraft_engine::world::{CHUNK_SIZE, Coord};

use crate::protocol::{PlayerInfo, Pose};
use crate::server::ClientId;

pub(crate) const PARTS: [&str; 4] = ["head", "body", "arm", "leg"];
// The width along the facing direction and the height of each part's sprite in pixels.
const SPRITES: [[f32; 2]; PARTS.len()] = [[64.0, 64.0], [44.0, 59.0], [28.0, 66.0], [28.0, 56.0]];
// The world size of a sprite pixel, which makes the man as tall as the player's box.
const PIXEL: f32 = 0.01;
// The eye above the feet, as in the engine's physics.
const EYE_HEIGHT: f64 = 1.7;
const BOXES: usize = 6;
/// The size of a box instance: pivot, yaw, pitch, texture layer, size, and offset.
pub(crate) const INSTANCE_BYTES: u64 = 12 * 4;
pub(crate) const MAX_INSTANCES: usize = crate::protocol::MAX_PLAYERS * BOXES;
// Farther moves than this jump instead of gliding.
const TELEPORT_DISTANCE: f64 = 8.0;

pub(crate) struct Other {
    name: String,
    pose: Pose,
    target: Pose,
    // The walk cycle and how far the limbs swing.
    phase: f32,
    stride: f32,
}

#[derive(Default)]
pub(crate) struct Others {
    players: BTreeMap<ClientId, Other>,
}

// The difference between two angles, from -PI to PI.
fn angle_difference(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

impl Others {
    pub(crate) fn update(&mut self, players: Vec<PlayerInfo>) {
        self.players
            .retain(|id, _| players.iter().any(|player| player.id == *id));
        for player in players {
            let other = self.players.entry(player.id).or_insert_with(|| Other {
                name: String::new(),
                pose: player.pose,
                target: player.pose,
                phase: 0.0,
                stride: 0.0,
            });
            other.name = player.name;
            other.target = player.pose;
        }
    }

    // Glides towards the received poses, which arrive twenty times a second.
    pub(crate) fn animate(&mut self, dt: f32) {
        let blend = 1.0 - (-dt * 15.0).exp();
        for other in self.players.values_mut() {
            let [x, y, z] = other.pose.position;
            let [tx, ty, tz] = other.target.position;
            let distance = ((tx - x).powi(2) + (ty - y).powi(2) + (tz - z).powi(2)).sqrt();
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
    pub(crate) fn instances(&self, center: Coord, first_layer: u32) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.players.len() * BOXES * INSTANCE_BYTES as usize);
        for other in self.players.values().take(crate::protocol::MAX_PLAYERS) {
            for part in other.boxes() {
                let layer = first_layer + part.sprite as u32;
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
                    .chain([other.pose.yaw, part.pitch, layer as f32])
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
    pub(crate) fn name_tags(&self, view: &View, center: Coord) -> Vec<(&str, [f32; 2])> {
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

struct Part {
    sprite: usize,
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
    fn boxes(&self) -> [Part; BOXES] {
        let [head, body, arm, leg] = sizes();
        let width = leg[0] * 2.0;
        let swing = self.phase.sin() * 0.8 * self.stride;
        let hip = leg[1];
        let neck = leg[1] + body[1];
        let shoulder = neck - arm[0] / 2.0;
        let hanging =
            |sprite: usize, [depth, height]: [f32; 2], side: f32, y: f32, pitch: f32| Part {
                sprite,
                pivot: [side, y],
                pitch,
                size: [depth, height, depth],
                offset: [-depth / 2.0, -height, -depth / 2.0],
            };
        [
            Part {
                sprite: 0,
                pivot: [0.0, neck],
                pitch: self.pose.pitch.clamp(-1.2, 1.2),
                size: [head[0].min(head[1]), head[1], head[0]],
                offset: [-head[0].min(head[1]) / 2.0, 0.0, -head[0] / 2.0],
            },
            Part {
                sprite: 1,
                pivot: [0.0, hip],
                pitch: 0.0,
                size: [width, body[1], body[0]],
                offset: [-width / 2.0, 0.0, -body[0] / 2.0],
            },
            hanging(2, arm, -(width + arm[0]) / 2.0, shoulder, -swing),
            hanging(2, arm, (width + arm[0]) / 2.0, shoulder, swing),
            hanging(3, leg, -leg[0] / 2.0, hip, swing),
            hanging(3, leg, leg[0] / 2.0, hip, -swing),
        ]
    }
}

#[cfg(test)]
mod tests {
    use plaatcraft_engine::camera::Camera;

    use super::*;

    fn info(id: ClientId, x: f64) -> PlayerInfo {
        PlayerInfo {
            id,
            name: format!("Player {id}"),
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
            let path = format!("assets/characters/male_{part}.png");
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
        others.update(vec![info(1, 8.0), info(2, 0.0)]);
        others.update(vec![info(1, 9.0)]);
        assert_eq!(others.players.len(), 1);
        for _ in 0..10 {
            others.animate(1.0 / 60.0);
        }
        let other = &others.players[&1];
        assert!(other.pose.position[0] > 8.5 && other.pose.position[0] < 9.0);
        assert!(other.stride > 0.0 && other.phase > 0.0);
        others.update(vec![info(1, 100.0)]);
        others.animate(1.0 / 60.0);
        assert_eq!(others.players[&1].pose.position[0], 100.0);
        assert!((angle_difference(0.1, TAU - 0.1) + 0.2).abs() < 0.0001);
    }

    #[test]
    fn boxes_stand_on_the_feet_and_tags_float_above() {
        let mut others = Others::default();
        others.update(vec![info(0, 8.0)]);
        let bytes = others.instances((0, 0), 100);
        assert_eq!(bytes.len(), BOXES * INSTANCE_BYTES as usize);
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
