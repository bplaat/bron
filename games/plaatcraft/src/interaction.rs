/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use plaatcraft_engine::raycast::{Hit, raycast};
use plaatcraft_engine::world::{self, ChunkData, Coord};

use crate::database::Database;

// Chunks within this many chunks of the player are kept for collision and aiming.
const NEARBY_RADIUS: i64 = 2;

type Loaded = (Coord, u64, Option<ChunkData>);

// Reads nearby chunks on a background thread with its own connection, so crossing a
// chunk border does not decode chunks during a frame.
struct Prefetch {
    requests: Option<Sender<(Coord, u64)>>,
    results: Receiver<Loaded>,
    pending: HashSet<Coord>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Prefetch {
    fn start(path: &str) -> Self {
        let (requests, jobs) = mpsc::channel::<(Coord, u64)>();
        let (finished, results) = mpsc::channel::<Loaded>();
        let path = path.to_owned();
        let thread = thread::Builder::new()
            .name("nearby-blocks".to_owned())
            .spawn(move || {
                let db = Database::open(&path).expect("Open prefetch database");
                for (coord, edits) in jobs {
                    let chunk = db.blocks(coord).expect("Read nearby blocks");
                    if finished.send((coord, edits, chunk)).is_err() {
                        break;
                    }
                }
            })
            .expect("Start nearby block loader");
        Self {
            requests: Some(requests),
            results,
            pending: HashSet::new(),
            thread: Some(thread),
        }
    }
}

impl Drop for Prefetch {
    fn drop(&mut self) {
        // Closing the request queue ends the loader loop.
        self.requests = None;
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            eprintln!("Nearby block loader failed during shutdown");
        }
    }
}

#[derive(Default)]
pub(crate) struct Interaction {
    pub(crate) target: Option<Hit>,
    chunks: HashMap<Coord, ChunkData>,
    prefetch: Option<Prefetch>,
    // Counts applied edits, so loads that started before an edit are discarded.
    edits: u64,
}

impl Interaction {
    pub(crate) fn with_prefetch(path: &str) -> Self {
        Self {
            prefetch: Some(Prefetch::start(path)),
            ..Self::default()
        }
    }

    pub(crate) fn apply_edit(&mut self, edit: world::Edit) {
        // Keep collision data alive while the worker replaces the rendered mesh.
        for (&coord, chunk) in &mut self.chunks {
            chunk.apply_edits(coord, &[edit]);
        }
        self.edits += 1;
        self.target = None;
    }

    // Keeps the chunks around the player and loads the missing ones in the background.
    pub(crate) fn prefetch(&mut self, center: Coord, loaded: impl Fn(Coord) -> bool) {
        let nearby = |(x, z): Coord| {
            (x - center.0).abs() <= NEARBY_RADIUS && (z - center.1).abs() <= NEARBY_RADIUS
        };
        self.chunks.retain(|&coord, _| nearby(coord));
        let Some(prefetch) = &mut self.prefetch else {
            return;
        };
        while let Ok((coord, edits, chunk)) = prefetch.results.try_recv() {
            prefetch.pending.remove(&coord);
            // A load that missed an edit is requested again below.
            if let Some(chunk) = chunk
                && edits == self.edits
                && nearby(coord)
            {
                self.chunks.entry(coord).or_insert(chunk);
            }
        }
        let Some(requests) = &prefetch.requests else {
            return;
        };
        for z in center.1 - NEARBY_RADIUS..=center.1 + NEARBY_RADIUS {
            for x in center.0 - NEARBY_RADIUS..=center.0 + NEARBY_RADIUS {
                let coord = (x, z);
                if loaded(coord)
                    && !self.chunks.contains_key(&coord)
                    && prefetch.pending.insert(coord)
                {
                    requests
                        .send((coord, self.edits))
                        .expect("Queue nearby block load");
                }
            }
        }
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
        // Prefetching normally loaded the chunk already, reading it here is the fallback.
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
        self.target = raycast(eye, direction, |position| self.block(db, position, &loaded));
    }
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
        let mut interaction = Interaction::default();
        let eye = [0.5, 50.5, 0.5];
        interaction.aim(&db, eye, [0.0, -1.0, 0.0], |_| true);
        let hit = interaction.target.unwrap();
        assert_eq!(hit.position, [0, 47, 0]);
        assert_eq!(hit.material, world::WATER);
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
    fn nearby_chunks_load_in_the_background_and_reload_after_edits() {
        let world = crate::database::tests::TestWorld::new();
        let db = Database::open(&world.path).unwrap();
        let flat = ChunkData::from_heights([60; 18 * 18]);
        db.save_chunk((0, 0), &flat.encode()).unwrap();
        let mut interaction = Interaction::with_prefetch(&world.path);
        let wait = |interaction: &mut Interaction| {
            for _ in 0..1000 {
                interaction.prefetch((0, 0), |coord| coord == (0, 0));
                if interaction.chunks.contains_key(&(0, 0)) {
                    return;
                }
                thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("Nearby chunk did not load");
        };
        wait(&mut interaction);

        // A load that is in flight during an edit is discarded and read again.
        interaction.chunks.clear();
        interaction.prefetch((0, 0), |coord| coord == (0, 0));
        let edit = world::Edit {
            position: [1, 60, 1],
            material: 3,
        };
        db.edit(edit).unwrap();
        interaction.apply_edit(edit);
        wait(&mut interaction);
        assert_eq!(interaction.chunks[&(0, 0)].voxel([1, 60, 1]), 3);

        // Chunks far from the player are dropped.
        interaction.prefetch((10, 10), |_| false);
        assert!(interaction.chunks.is_empty());
    }

    #[test]
    fn editing_while_moving_keeps_collision_and_picking_available() {
        use plaatcraft_engine::camera::Camera;
        use plaatcraft_engine::world::Edit;
        let db = Database::open(":memory:").unwrap();
        let flat = ChunkData::from_heights([60; 18 * 18]);
        db.save_chunk((0, 0), &flat.encode()).unwrap();
        db.save_chunk((1, 0), &flat.encode()).unwrap();
        let mut interaction = Interaction::default();
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
        camera.input.direction = [1.0, 0.0];
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
        let mut cold = Interaction::default();
        assert_eq!(cold.block(&db, edit.position, |_| true), Some(0));
    }
}
