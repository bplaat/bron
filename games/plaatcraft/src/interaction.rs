/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;

use crate::database::Database;
use crate::world::{self, ChunkData, Coord};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) position: [i64; 3],
    pub(crate) adjacent: Option<[i64; 3]>,
    pub(crate) material: u8,
}

// Traverse crossed voxel boundaries exactly, including negative coordinates.
pub(crate) fn raycast(
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
    let mut next = std::array::from_fn::<_, 3, _>(|axis| {
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
            && (material != world::WATER || adjacent.is_some() && previous != world::WATER)
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

pub(crate) struct Interaction {
    pub(crate) selected: u8,
    pub(crate) target: Option<Hit>,
    chunks: HashMap<Coord, ChunkData>,
    pub(crate) scroll: crate::scroll::Scroll,
}

impl Interaction {
    pub(crate) fn new(selected: u8) -> Self {
        Self {
            selected,
            target: None,
            chunks: HashMap::new(),
            scroll: crate::scroll::Scroll::default(),
        }
    }

    pub(crate) fn apply_edit(&mut self, edit: world::Edit) {
        // Keep collision data alive while the worker replaces the rendered mesh.
        for (&coord, chunk) in &mut self.chunks {
            chunk.apply_edits(coord, &[edit]);
        }
        self.target = None;
    }

    pub(crate) fn block(
        &mut self,
        db: &Database,
        position: [i64; 3],
        loaded: impl Fn(Coord) -> bool,
    ) -> Option<u8> {
        if position[1] < 0 || position[1] >= world::HEIGHT as i64 {
            return Some(0);
        }
        let coord = (position[0].div_euclid(16), position[2].div_euclid(16));
        if !loaded(coord) {
            return None;
        }
        if let std::collections::hash_map::Entry::Vacant(entry) = self.chunks.entry(coord) {
            entry.insert(db.blocks(coord).expect("Read nearby blocks")?);
        }
        Some(self.chunks[&coord].voxel([
            position[0].rem_euclid(16) as i32,
            position[1] as i32,
            position[2].rem_euclid(16) as i32,
        ]))
    }

    pub(crate) fn aim(
        &mut self,
        db: &Database,
        eye: [f64; 3],
        direction: [f64; 3],
        loaded: impl Fn(Coord) -> bool,
    ) {
        let center = (
            (eye[0] / 16.0).floor() as i64,
            (eye[2] / 16.0).floor() as i64,
        );
        // An eight-block reach touches at most the surrounding nine chunk columns.
        self.chunks
            .retain(|&(x, z), _| (x - center.0).abs() <= 1 && (z - center.1).abs() <= 1);
        self.target = raycast(eye, direction, |position| self.block(db, position, &loaded));
    }

    pub(crate) fn select(&mut self, db: &Database, material: u8) {
        let Some(material) = world::canonical_material(material) else {
            return;
        };
        if material != self.selected {
            match db.select(material) {
                Ok(()) => self.selected = material,
                Err(error) => eprintln!("Could not save block selection: {error}"),
            }
        }
    }

    pub(crate) fn scroll(&mut self, db: &Database, steps: i64) {
        if steps == 0 {
            return;
        }
        let index = world::MATERIALS
            .iter()
            .position(|&(id, _)| id == self.selected)
            .unwrap_or(0) as i64;
        let next = (index - steps.rem_euclid(world::MATERIALS.len() as i64))
            .rem_euclid(world::MATERIALS.len() as i64) as usize;
        self.select(db, world::MATERIALS[next].0);
    }

    pub(crate) fn name(&self) -> &'static str {
        world::MATERIALS
            .iter()
            .find(|&&(id, _)| id == self.selected)
            .map_or("Stone", |&(_, name)| name)
    }
}

pub(crate) fn overlaps_player(block: [i64; 3], eye: [f64; 3]) -> bool {
    let min = [eye[0] - 0.3, eye[1] - 1.7, eye[2] - 0.3];
    let max = [eye[0] + 0.3, eye[1] + 0.1, eye[2] + 0.3];
    (0..3).all(|axis| (block[axis] as f64) < max[axis] && (block[axis] + 1) as f64 > min[axis])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removing_water_targets_the_surface_and_exposes_the_next_layer() {
        let world = crate::database::tests::TestWorld::new();
        let db = Database::open(&world.path).unwrap();
        let ocean = ChunkData::from_heights([12; 18 * 18]);
        db.save_chunk((0, 0), &ocean.encode()).unwrap();
        let mut interaction = Interaction::new(3);
        let eye = [0.5, 50.5, 0.5];
        interaction.aim(&db, eye, [0.0, -1.0, 0.0], |_| true);
        let hit = interaction.target.unwrap();
        assert_eq!(hit.position, [0, 47, 0]);
        assert_eq!(hit.material, world::WATER);
        interaction.select(&db, hit.material);
        assert_eq!(db.selected().unwrap(), world::WATER);
        let edit = world::Edit {
            position: hit.position,
            material: 0,
        };
        db.edit(edit).unwrap();
        interaction.apply_edit(edit);
        assert_eq!(interaction.block(&db, [0, 47, 0], |_| true), Some(0));
        interaction.aim(&db, eye, [0.0, -1.0, 0.0], |_| true);
        assert_eq!(interaction.target.unwrap().position, [0, 46, 0]);
        drop(db);
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.terrain((0, 0)).unwrap().0.voxel([0, 47, 0]), 0);
        assert_eq!(
            db.blocks((0, 0)).unwrap().unwrap().voxel([0, 46, 0]),
            world::WATER
        );
    }

    #[test]
    fn editing_while_moving_keeps_collision_and_picking_available() {
        use bwindow::KeyCode;

        use crate::camera::Camera;
        use crate::world::Edit;
        let db = Database::open(":memory:").unwrap();
        let flat = ChunkData::from_heights([60; 18 * 18]);
        db.save_chunk((0, 0), &flat.encode()).unwrap();
        db.save_chunk((1, 0), &flat.encode()).unwrap();
        let mut interaction = Interaction::new(3);
        assert_eq!(interaction.block(&db, [8, 60, 8], |_| true), Some(0));
        let edit = Edit {
            position: [15, 60, 1],
            material: 3,
        };
        db.edit(edit).unwrap();
        interaction.apply_edit(edit);
        assert!(db.load_chunk((0, 0)).unwrap().is_none());
        assert!(db.load_chunk((1, 0)).unwrap().is_none());
        assert_eq!(interaction.block(&db, edit.position, |_| true), Some(3));
        // Crossing into an uncached, dirty neighbor must still return actual voxels.
        assert_eq!(interaction.block(&db, [16, 61, 8], |_| true), Some(0));
        let mut camera = Camera {
            position: [8.5, 61.7, 8.5],
            yaw: 0.0,
            ..Camera::default()
        };
        camera.keys.insert(KeyCode::KeyD);
        camera.update(0.05, |p| {
            !matches!(interaction.block(&db, p, |_| true), Some(0))
        });
        assert!(camera.position[0] > 8.7);
        assert!((camera.position[1] - 61.7).abs() < 0.0001);
        let hit = raycast([13.5, 60.5, 1.5], [1.0, 0.0, 0.0], |p| {
            interaction.block(&db, p, |_| true)
        })
        .unwrap();
        assert_eq!(hit.position, edit.position);
        assert_eq!(hit.adjacent, Some([14, 60, 1]));
        let remove = Edit {
            material: 0,
            ..edit
        };
        db.edit(remove).unwrap();
        interaction.apply_edit(remove);
        assert_eq!(interaction.block(&db, edit.position, |_| true), Some(0));
        let mut cold = Interaction::new(3);
        assert_eq!(cold.block(&db, edit.position, |_| true), Some(0));
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
