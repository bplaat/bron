/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::{HashMap, HashSet};

use plaatcraft_engine::protocol::Request;
use plaatcraft_engine::raycast::{Hit, raycast};
use plaatcraft_engine::world::{self, ChunkData, Coord};

use crate::server::Connection;

// Chunks within this many chunks of the player are kept for collision and aiming.
const NEARBY_RADIUS: i64 = 2;

// The blocks around the player, loaded from the server so crossing a chunk border does
// not decode chunks during a frame.
#[derive(Default)]
pub(crate) struct Interaction {
    pub(crate) target: Option<Hit>,
    chunks: HashMap<Coord, ChunkData>,
    pending: HashSet<Coord>,
    // Counts applied edits, so loads that started before an edit are discarded.
    edits: u64,
}

impl Interaction {
    pub(crate) fn apply_edit(&mut self, edit: world::Edit) {
        self.apply_edits(&[edit]);
    }

    pub(crate) fn apply_edits(&mut self, edits: &[world::Edit]) {
        // Keep collision data alive while the server rebuilds the rendered mesh.
        for (&coord, chunk) in &mut self.chunks {
            chunk.apply_edits(coord, edits);
        }
        self.edits += 1;
        self.target = None;
    }

    // Forgets the nearby blocks, after an edit the server didn't save.
    pub(crate) fn reload(&mut self) {
        self.chunks.clear();
        self.edits += 1;
        self.target = None;
    }

    // Keeps the chunks around the player and requests the missing ones.
    pub(crate) fn prefetch(
        &mut self,
        server: &Connection,
        center: Coord,
        loaded: impl Fn(Coord) -> bool,
    ) {
        self.chunks.retain(|&coord, _| nearby(coord, center));
        for z in center.1 - NEARBY_RADIUS..=center.1 + NEARBY_RADIUS {
            for x in center.0 - NEARBY_RADIUS..=center.0 + NEARBY_RADIUS {
                let coord = (x, z);
                if loaded(coord) && !self.chunks.contains_key(&coord) && self.pending.insert(coord)
                {
                    server.send(Request::Blocks(coord, self.edits));
                }
            }
        }
    }

    pub(crate) fn receive(
        &mut self,
        center: Coord,
        coord: Coord,
        edits: u64,
        chunk: Option<ChunkData>,
    ) {
        self.pending.remove(&coord);
        // A load that missed an edit is requested again.
        if let Some(chunk) = chunk
            && edits == self.edits
            && nearby(coord, center)
        {
            self.chunks.entry(coord).or_insert(chunk);
        }
    }

    /// The block at a position, when its chunk is loaded.
    pub(crate) fn block(&self, position: [i64; 3]) -> Option<u8> {
        if position[1] < 0 || position[1] >= world::HEIGHT as i64 {
            return Some(0);
        }
        let coord = (position[0].div_euclid(16), position[2].div_euclid(16));
        Some(self.chunks.get(&coord)?.voxel([
            position[0].rem_euclid(16) as i32,
            position[1] as i32,
            position[2].rem_euclid(16) as i32,
        ]))
    }

    pub(crate) fn aim(&mut self, eye: [f64; 3], direction: [f64; 3]) {
        self.target = raycast(eye, direction, |position| self.block(position));
    }
}

const fn nearby((x, z): Coord, center: Coord) -> bool {
    (x - center.0).abs() <= NEARBY_RADIUS && (z - center.1).abs() <= NEARBY_RADIUS
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use plaatcraft_engine::camera::Camera;
    use plaatcraft_engine::protocol::Response;
    use plaatcraft_engine::world::Edit;

    use super::*;
    use crate::database::Database;
    use crate::database::tests::TestWorld;

    fn flat(heights: u8) -> ChunkData {
        ChunkData::from_heights([heights; 18 * 18])
    }

    #[test]
    fn removing_water_targets_the_surface_and_exposes_the_next_layer() {
        let mut interaction = Interaction::default();
        interaction.receive((0, 0), (0, 0), 0, Some(flat(12)));
        let eye = [0.5, 50.5, 0.5];
        interaction.aim(eye, [0.0, -1.0, 0.0]);
        let hit = interaction.target.unwrap();
        assert_eq!(hit.position, [0, 47, 0]);
        assert_eq!(hit.material, world::WATER);
        interaction.apply_edit(Edit {
            position: hit.position,
            material: 0,
        });
        assert_eq!(interaction.block([0, 47, 0]), Some(0));
        interaction.aim(eye, [0.0, -1.0, 0.0]);
        assert_eq!(interaction.target.unwrap().position, [0, 46, 0]);
    }

    #[test]
    fn nearby_chunks_load_from_the_server_and_reload_after_edits() {
        let world = TestWorld::new();
        Database::open(&world.path)
            .unwrap()
            .save_chunk((0, 0), &flat(60).encode())
            .unwrap();
        let (mut server, _) = crate::server::tests::host(&world);
        let mut interaction = Interaction::default();
        let wait = |interaction: &mut Interaction, server: &mut Connection| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !interaction.chunks.contains_key(&(0, 0)) {
                interaction.prefetch(server, (0, 0), |coord| coord == (0, 0));
                while let Some(response) = server.receive() {
                    if let Response::Blocks { coord, tag, chunk } = response {
                        interaction.receive((0, 0), coord, tag, chunk);
                    }
                }
                assert!(Instant::now() < deadline, "Nearby chunk did not load");
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        wait(&mut interaction, &mut server);

        // A load that is in flight during an edit is discarded and read again.
        interaction.chunks.clear();
        interaction.prefetch(&server, (0, 0), |coord| coord == (0, 0));
        let edit = Edit {
            position: [1, 60, 1],
            material: 3,
        };
        server.send(Request::Edit(edit));
        interaction.apply_edit(edit);
        wait(&mut interaction, &mut server);
        assert_eq!(interaction.chunks[&(0, 0)].voxel([1, 60, 1]), 3);

        // Chunks far from the player are dropped.
        interaction.prefetch(&server, (10, 10), |_| false);
        assert!(interaction.chunks.is_empty());
    }

    #[test]
    fn editing_while_moving_keeps_collision_and_picking_available() {
        let mut interaction = Interaction::default();
        assert_eq!(interaction.block([8, 60, 8]), None);
        for coord in [(0, 0), (1, 0)] {
            interaction.receive((0, 0), coord, 0, Some(flat(60)));
        }
        assert_eq!(interaction.block([8, 60, 8]), Some(0));
        let edit = Edit {
            position: [15, 60, 1],
            material: 3,
        };
        interaction.apply_edit(edit);
        assert_eq!(interaction.block(edit.position), Some(3));
        // The neighbor's halo got the edit too.
        assert_eq!(interaction.block([16, 61, 8]), Some(0));
        let mut camera = Camera {
            position: [8.5, 61.7, 8.5],
            yaw: 0.0,
            ..Camera::default()
        };
        camera.input.direction = [1.0, 0.0];
        camera.update(0.05, |p| !matches!(interaction.block(p), Some(0)));
        assert!(camera.position[0] > 8.7);
        assert!((camera.position[1] - 61.7).abs() < 0.0001);
        let hit = raycast([13.5, 60.5, 1.5], [1.0, 0.0, 0.0], |p| interaction.block(p)).unwrap();
        assert_eq!(hit.position, edit.position);
        assert_eq!(hit.adjacent, Some([14, 60, 1]));
        interaction.apply_edit(Edit {
            material: 0,
            ..edit
        });
        assert_eq!(interaction.block(edit.position), Some(0));
        // Loads from before the edits and rejected edits are not kept.
        interaction.receive((0, 0), (0, 1), 0, Some(flat(60)));
        assert_eq!(interaction.block([8, 60, 24]), None);
        interaction.reload();
        assert_eq!(interaction.block(edit.position), None);
    }
}
