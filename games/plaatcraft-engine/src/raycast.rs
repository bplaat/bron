/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Picking the block the player aims at.

#[cfg(not(feature = "std"))]
use crate::math::Float;
use crate::world::{self, Edit};

/// A block hit by [`raycast`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The position of the hit block.
    pub position: [i64; 3],
    /// The empty block in front of the hit face, where a placed block goes.
    pub adjacent: Option<[i64; 3]>,
    /// The material of the hit block.
    pub material: u8,
}

impl Hit {
    /// Where a placed block goes: sprite decorations are replaced, solid blocks get
    /// the block in front of the hit face.
    pub const fn placement_position(self) -> Option<[i64; 3]> {
        if world::is_sprite(self.material) {
            Some(self.position)
        } else {
            self.adjacent
        }
    }

    /// The edit that breaks the hit block, bedrock can't be broken.
    pub fn break_edit(self) -> Option<Edit> {
        (self.material != world::BEDROCK && buildable(self.position)).then_some(Edit {
            position: self.position,
            material: 0,
        })
    }

    /// The edit that places `item` for the player at `eye`, into air, water or a sprite
    /// decoration that `block` returns for the placement position.
    pub fn place_edit(
        self,
        item: u8,
        eye: [f64; 3],
        block: impl FnOnce([i64; 3]) -> Option<u8>,
    ) -> Option<Edit> {
        let position = self.placement_position()?;
        let replaceable =
            |material| material == 0 || world::is_water(material) || world::is_sprite(material);
        if !buildable(position)
            || overlaps_player(position, eye)
            || !block(position).is_some_and(replaceable)
        {
            return None;
        }
        Some(Edit {
            position,
            material: world::placed_material(
                item,
                eye[0] - (position[0] as f64 + 0.5),
                eye[2] - (position[2] as f64 + 0.5),
                position[1] < self.position[1],
            ),
        })
    }
}

// Nothing is built or broken in the bedrock layers.
fn buildable(position: [i64; 3]) -> bool {
    (3..world::HEIGHT as i64).contains(&position[1])
}

/// Finds the block the player aims at within reach, `block` returns `None` for
/// unloaded positions. Traverses crossed voxel boundaries exactly, including negative
/// coordinates.
pub fn raycast(
    eye: [f64; 3],
    direction: [f64; 3],
    mut block: impl FnMut([i64; 3]) -> Option<u8>,
) -> Option<Hit> {
    let mut position = eye.map(|v| v.floor() as i64);
    let step = direction.map(|v| {
        if v > 0.0 {
            1
        } else if v < 0.0 {
            -1
        } else {
            0
        }
    });
    let delta = direction.map(|v| {
        if v == 0.0 {
            f64::INFINITY
        } else {
            1.0 / v.abs()
        }
    });
    let mut next = core::array::from_fn::<_, 3, _>(|axis| {
        if step[axis] == 0 {
            f64::INFINITY
        } else {
            ((position[axis] + i64::from(step[axis] > 0)) as f64 - eye[axis]) / direction[axis]
        }
    });
    let mut adjacent = None;
    let mut previous = 0;
    loop {
        let material = block(position)?;
        // Water is targetable at an exposed boundary. When swimming, skip the
        // surrounding water volume so the ray can still reach the seabed.
        if material != 0
            && (!world::is_water(material) || adjacent.is_some() && !world::is_water(previous))
        {
            return Some(Hit {
                position,
                adjacent,
                material,
            });
        }
        let axis = (0..3)
            .min_by(|&a, &b| next[a].total_cmp(&next[b]))
            .expect("Ray has three axes");
        if next[axis] > 8.0 || !next[axis].is_finite() {
            return None;
        }
        previous = material;
        let distance = next[axis];
        // Cross simultaneous boundaries together: edge/corner-only neighbors
        // have no volume along this ray and must not become selection targets.
        for a in 0..3 {
            if next[a] == distance {
                position[a] += step[a];
                next[a] += delta[a];
            }
        }
        let mut face_neighbor = position;
        face_neighbor[axis] -= step[axis];
        adjacent = Some(face_neighbor);
    }
}

/// Whether a block intersects the player's collision box, which matches the camera's.
pub fn overlaps_player(block: [i64; 3], eye: [f64; 3]) -> bool {
    const LOWER: [f64; 3] = [-0.3, -1.7, -0.3];
    const UPPER: [f64; 3] = [0.3, 0.1, 0.3];
    (0..3).all(|axis| {
        let block = block[axis] as f64;
        eye[axis] + LOWER[axis] < block + 1.0 && eye[axis] + UPPER[axis] > block
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_replaces_sprite_decorations_and_uses_solid_block_faces() {
        for &(material, _) in world::MATERIALS {
            let hit = Hit {
                position: [0, 60, 0],
                adjacent: Some([0, 61, 0]),
                material,
            };
            if world::is_sprite(material) {
                assert_eq!(hit.placement_position(), Some(hit.position));
                assert_eq!(
                    Hit {
                        adjacent: None,
                        ..hit
                    }
                    .placement_position(),
                    Some(hit.position)
                );
            } else {
                assert_eq!(hit.placement_position(), hit.adjacent);
                assert_eq!(
                    Hit {
                        adjacent: None,
                        ..hit
                    }
                    .placement_position(),
                    None
                );
            }
        }
    }

    #[test]
    fn edits_keep_bedrock_and_turn_ovens_toward_the_player() {
        let hit = |material| Hit {
            position: [0, 60, 0],
            adjacent: Some([0, 61, 0]),
            material,
        };
        assert_eq!(hit(world::BEDROCK).break_edit(), None);
        assert_eq!(hit(3).break_edit().map(|edit| edit.material), Some(0));
        let eye = [0.5, 63.6, 3.0];
        let edit = hit(3).place_edit(36, eye, |_| Some(0)).unwrap();
        assert_eq!((edit.position, edit.material), ([0, 61, 0], 36));
        assert_eq!(
            hit(3)
                .place_edit(36, [3.0, 63.6, 0.5], |_| Some(0))
                .unwrap()
                .material,
            75
        );
        assert_eq!(hit(3).place_edit(3, eye, |_| Some(3)), None);
        assert_eq!(hit(3).place_edit(3, [0.5, 62.6, 0.5], |_| Some(0)), None);
        let deep = Hit {
            position: [0, 2, 0],
            adjacent: Some([0, 2, 0]),
            material: 0,
        };
        assert_eq!(deep.place_edit(3, eye, |_| Some(0)), None);
    }

    #[test]
    fn edge_and_corner_rays_skip_voxels_they_only_touch() {
        for direction in [[1.0, 0.0, 1.0], [-1.0, 0.0, -1.0], [1.0, 1.0, 1.0]] {
            let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt();
            let direction = direction.map(|v| v / length);
            let target = direction.map(|v| {
                if v > 0.0 {
                    1
                } else if v < 0.0 {
                    -1
                } else {
                    0
                }
            });
            let hit = raycast([0.5; 3], direction, |p| {
                Some(if p == [0; 3] {
                    0
                } else if p == target {
                    4
                } else {
                    3
                })
            })
            .unwrap();
            assert_eq!(hit.position, target);
            assert_eq!(hit.material, 4);
            let adjacent = hit.adjacent.unwrap();
            assert_eq!(
                target
                    .into_iter()
                    .zip(adjacent)
                    .map(|(a, b)| (a - b).abs())
                    .sum::<i64>(),
                1
            );
        }
    }

    #[test]
    fn picking_handles_axes_negative_boundaries_reach_and_unloaded_chunks() {
        let hit = raycast([0.5, 10.5, 0.5], [-1.0, 0.0, 0.0], |p| {
            Some(if p == [-2, 10, 0] { 3 } else { 0 })
        })
        .unwrap();
        assert_eq!(hit.position, [-2, 10, 0]);
        assert_eq!(hit.adjacent, Some([-1, 10, 0]));
        let hit = raycast([0.0, 10.5, 0.5], [-1.0, 0.0, 0.0], |p| {
            Some(if p[0] == -1 { 4 } else { 0 })
        })
        .unwrap();
        assert_eq!(hit.adjacent, Some([0, 10, 0]));
        assert!(
            raycast([0.5, 10.5, 0.5], [1.0, 0.0, 0.0], |p| Some(if p[0] == 9 {
                3
            } else {
                0
            }))
            .is_none()
        );
        assert!(
            raycast([0.5, 10.5, 0.5], [1.0, 0.0, 0.0], |p| if p[0] == 2 {
                None
            } else {
                Some(0)
            })
            .is_none()
        );
        let hit = raycast([0.5, 10.5, 0.5], [0.0, -1.0, 0.0], |p| {
            Some(if p[1] == 9 {
                22
            } else if p[1] == 8 {
                4
            } else {
                0
            })
        })
        .unwrap();
        assert_eq!(hit.position, [0, 9, 0]);
        assert_eq!(hit.material, world::WATER);
        assert_eq!(hit.adjacent, Some([0, 10, 0]));
        let submerged = raycast([0.5, 9.5, 0.5], [0.0, -1.0, 0.0], |p| {
            Some(if p[1] >= 8 { world::WATER } else { 4 })
        })
        .unwrap();
        assert_eq!(submerged.position, [0, 7, 0]);
        assert!(overlaps_player([0, 9, 0], [0.5, 10.5, 0.5]));
        assert!(!overlaps_player([1, 9, 0], [0.5, 10.5, 0.5]));
    }
}
