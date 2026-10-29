/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Generates and meshes the chunks around the player on a worker thread. Loaded chunks
//! keep their display list and their compressed blocks, the chunks next to the player
//! are decoded for collision and picking. Edits only live as long as the game runs.
//!
//! Lighting a chunk needs its neighbors, so the worker keeps the unlit chunks next to the
//! chunks it still has to build, instead of generating every chunk again for each neighbor.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use plaatcraft_engine::world::{CHUNK_SIZE, ChunkData, Coord, Edit, HEIGHT, WORLD_SEED};

use crate::mesh::{self, Mesh};
use crate::sync::{Condvar, Mutex};

pub const RENDER_DISTANCE: i64 = 6;
// Lower than the main thread, so generation uses the time the main thread waits for vsync
const WORKER_PRIORITY: u8 = 40;
const WORKER_STACK: usize = 256 * 1024;

pub const fn in_range(coord: Coord, center: Coord, radius: i64) -> bool {
    let x = coord.0 - center.0;
    let z = coord.1 - center.1;
    x * x + z * z <= radius * radius
}

pub struct Chunk {
    pub mesh: Mesh,
    blocks: Vec<u8>,
}

struct Built {
    coord: Coord,
    revision: u32,
    blocks: Vec<u8>,
    mesh: Mesh,
}

#[derive(Default)]
struct Shared {
    center: Coord,
    // Edited chunks are rebuilt first, the others nearest first
    urgent: Vec<Coord>,
    queue: Vec<Coord>,
    building: Option<Coord>,
    edits: Vec<Edit>,
    // Counts the edits, so chunks built before an edit are built again
    revision: u32,
    built: Vec<Built>,
}

impl Shared {
    fn loading(&self, coord: Coord) -> bool {
        self.building == Some(coord) || self.queue.contains(&coord) || self.urgent.contains(&coord)
    }
}

struct Worker {
    shared: Mutex<Shared>,
    wake: Condvar,
}

pub struct Stream {
    worker: &'static Worker,
    pub chunks: BTreeMap<Coord, Chunk>,
    // The revision each chunk needs to be built with, set when an edit touches it
    needed: BTreeMap<Coord, u32>,
    nearby: BTreeMap<Coord, ChunkData>,
    edits: Vec<Edit>,
    center: Option<Coord>,
}

impl Stream {
    pub fn start() -> Self {
        let worker: &'static Worker = Box::leak(Box::new(Worker {
            shared: Mutex::new(Shared::default()),
            wake: Condvar::new(),
        }));
        crate::sync::spawn(WORKER_STACK, WORKER_PRIORITY, move || work(worker));
        Self {
            worker,
            chunks: BTreeMap::new(),
            needed: BTreeMap::new(),
            nearby: BTreeMap::new(),
            edits: Vec::new(),
            center: None,
        }
    }

    /// Keeps the chunks around the player and takes the chunks the worker finished.
    pub fn update(&mut self, center: Coord) {
        let mut shared = self.worker.shared.lock();
        for built in core::mem::take(&mut shared.built) {
            match self.needed.get(&built.coord) {
                Some(&needed) if built.revision < needed => continue,
                Some(_) => _ = self.needed.remove(&built.coord),
                None => {}
            }
            if in_range(built.coord, center, RENDER_DISTANCE) {
                self.nearby.remove(&built.coord);
                self.chunks.insert(
                    built.coord,
                    Chunk {
                        mesh: built.mesh,
                        blocks: built.blocks,
                    },
                );
            }
        }
        if self.center == Some(center) {
            return;
        }
        self.center = Some(center);
        shared.center = center;
        self.chunks
            .retain(|&coord, _| in_range(coord, center, RENDER_DISTANCE));
        self.nearby
            .retain(|&coord, _| (coord.0 - center.0).abs() <= 1 && (coord.1 - center.1).abs() <= 1);
        shared
            .queue
            .retain(|&coord| in_range(coord, center, RENDER_DISTANCE));
        for z in -RENDER_DISTANCE..=RENDER_DISTANCE {
            for x in -RENDER_DISTANCE..=RENDER_DISTANCE {
                let coord = (center.0 + x, center.1 + z);
                if in_range(coord, center, RENDER_DISTANCE)
                    && !self.chunks.contains_key(&coord)
                    && !shared.loading(coord)
                {
                    shared.queue.push(coord);
                }
            }
        }
        self.worker.wake.signal();
    }

    /// The block at a world position, `None` while its chunk is not loaded.
    pub fn block(&mut self, position: [i64; 3]) -> Option<u8> {
        if position[1] < 0 || position[1] >= HEIGHT as i64 {
            return Some(0);
        }
        let size = CHUNK_SIZE as i64;
        let coord = (position[0].div_euclid(size), position[2].div_euclid(size));
        if !self.nearby.contains_key(&coord) {
            let chunk = self.chunks.get(&coord)?;
            let mut data = ChunkData::decode(&chunk.blocks).ok()?;
            // The compressed blocks miss the edits that are still being rebuilt
            data.apply_edits(coord, &self.edits);
            self.nearby.insert(coord, data);
        }
        Some(self.nearby[&coord].voxel([
            position[0].rem_euclid(size) as i32,
            position[1] as i32,
            position[2].rem_euclid(size) as i32,
        ]))
    }

    /// Places or removes a block and rebuilds the chunks it touches, the light of an edit
    /// reaches into the neighboring chunks.
    pub fn edit(&mut self, edit: Edit) {
        let size = CHUNK_SIZE as i64;
        let center = (
            edit.position[0].div_euclid(size),
            edit.position[2].div_euclid(size),
        );
        self.edits.push(edit);
        for (&coord, chunk) in &mut self.nearby {
            chunk.apply_edits(coord, &[edit]);
        }
        let mut shared = self.worker.shared.lock();
        shared.edits.push(edit);
        shared.revision += 1;
        let revision = shared.revision;
        // The edited chunk first, then its neighbors
        let mut affected = Vec::new();
        affected.push(center);
        for z in -1..=1 {
            for x in -1..=1 {
                if (x, z) != (0, 0) {
                    affected.push((center.0 + x, center.1 + z));
                }
            }
        }
        for coord in affected {
            if !self.chunks.contains_key(&coord) && !shared.loading(coord) {
                continue;
            }
            self.needed.insert(coord, revision);
            shared.queue.retain(|&queued| queued != coord);
            shared.urgent.retain(|&queued| queued != coord);
            shared.urgent.push(coord);
        }
        self.worker.wake.signal();
    }
}

/// Generates lit chunks and keeps the unlit chunks next to the chunks still to build.
#[derive(Default)]
pub struct Generator {
    bases: BTreeMap<Coord, ChunkData>,
}

impl Generator {
    /// Generates a chunk with the edits, `pending` are the chunks that are built later.
    pub fn generate(&mut self, coord: Coord, edits: &[Edit], pending: &[Coord]) -> ChunkData {
        let near = |(x, z): Coord, base: Coord| (x - base.0).abs() <= 1 && (z - base.1).abs() <= 1;
        self.bases
            .retain(|&base, _| near(coord, base) || pending.iter().any(|&coord| near(coord, base)));
        for z in -1..=1 {
            for x in -1..=1 {
                let neighbor = (coord.0 + x, coord.1 + z);
                self.bases
                    .entry(neighbor)
                    .or_insert_with(|| ChunkData::generate_base(WORLD_SEED, neighbor));
            }
        }
        ChunkData::rebuild(WORLD_SEED, coord, &self.bases, edits)
    }
}

fn work(worker: &Worker) {
    let mut generator = Generator::default();
    loop {
        let (coord, edits, revision, pending) = {
            let mut shared = worker.shared.lock();
            while shared.urgent.is_empty() && shared.queue.is_empty() {
                shared = worker.wake.wait(shared);
            }
            let coord = if shared.urgent.is_empty() {
                let center = shared.center;
                let distance = |(x, z): Coord| (x - center.0).pow(2) + (z - center.1).pow(2);
                let nearest = (0..shared.queue.len())
                    .min_by_key(|&index| distance(shared.queue[index]))
                    .expect("Queue has chunks");
                shared.queue.swap_remove(nearest)
            } else {
                shared.urgent.remove(0)
            };
            shared.building = Some(coord);
            // Edits rebuild the chunks around the player, so their bases are kept too
            let center = shared.center;
            let around = (-1..=1).flat_map(|z| (-1..=1).map(move |x| (center.0 + x, center.1 + z)));
            let pending: Vec<Coord> = shared
                .urgent
                .iter()
                .chain(&shared.queue)
                .copied()
                .chain(around)
                .collect();
            (coord, shared.edits.clone(), shared.revision, pending)
        };
        let chunk = generator.generate(coord, &edits, &pending);
        let built = Built {
            coord,
            revision,
            mesh: mesh::build(&chunk.mesh()),
            blocks: chunk.encode(),
        };
        let mut shared = worker.shared.lock();
        shared.building = None;
        shared.built.push(built);
    }
}
