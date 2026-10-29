/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Generates and meshes the chunks around the player on a worker thread. Loaded chunks
//! keep their display list and their compressed blocks, the chunks next to the player
//! are decoded for collision, picking and fluid ticks.
//!
//! Lighting a chunk needs its neighbors, so the worker keeps the unlit chunks next to the
//! chunks it still has to build, instead of generating every chunk again for each neighbor.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;

use plaatcraft_engine::world::{CHUNK_SIZE, ChunkData, Coord, Edit, HEIGHT, WORLD_SEED};

use crate::mesh::{self, Mesh};
use crate::sync::{Condvar, Mutex};

pub const RENDER_DISTANCE: i64 = 8;
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
    epoch: u64,
    blocks: Vec<u8>,
    mesh: Mesh,
}

#[derive(Default)]
struct Shared {
    center: Coord,
    seed: u64,
    epoch: u64,
    // Edited chunks are rebuilt first, the others nearest first
    urgent: Vec<Coord>,
    queue: Vec<Coord>,
    building: Option<Coord>,
    edits: Vec<Edit>,
    // Counts the edits, so chunks built before an edit are built again
    revision: u32,
    world_revision: u64,
    built: Vec<Built>,
    supplied: BTreeMap<Coord, ChunkData>,
    jobs: VecDeque<(u32, Coord, Option<u64>)>,
    results: Vec<JobResult>,
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

pub struct JobResult {
    pub id: u32,
    pub coord: Coord,
    pub tag: Option<u64>,
    pub chunk: ChunkData,
    pub revision: u64,
}

pub struct Stream {
    worker: &'static Worker,
    pub chunks: BTreeMap<Coord, Chunk>,
    // The revision each chunk needs to be built with, set when an edit touches it
    needed: BTreeMap<Coord, u32>,
    nearby: BTreeMap<Coord, ChunkData>,
    edits: Vec<Edit>,
    center: Option<Coord>,
    pub seed: u64,
    pub revision: u64,
    epoch: u64,
    pub remote: bool,
    requests: Vec<Coord>,
    requested: BTreeSet<Coord>,
    in_flight: BTreeSet<Coord>,
    pub centers: Vec<Coord>,
    required: BTreeMap<Coord, u64>,
    remote_blocks: BTreeMap<Coord, Vec<u8>>,
}

impl Stream {
    pub fn start() -> Self {
        let worker: &'static Worker = Box::leak(Box::new(Worker {
            shared: Mutex::new(Shared {
                seed: WORLD_SEED,
                ..Shared::default()
            }),
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
            seed: WORLD_SEED,
            revision: 0,
            epoch: 0,
            remote: false,
            requests: Vec::new(),
            requested: BTreeSet::new(),
            in_flight: BTreeSet::new(),
            centers: Vec::new(),
            required: BTreeMap::new(),
            remote_blocks: BTreeMap::new(),
        }
    }

    pub fn reset(&mut self, seed: u64, edits: Vec<Edit>) {
        self.remote = false;
        self.requests.clear();
        self.requested.clear();
        self.in_flight.clear();
        self.required.clear();
        self.remote_blocks.clear();
        self.epoch += 1;
        self.seed = seed;
        self.revision = 0;
        self.centers.clear();
        self.chunks.clear();
        self.nearby.clear();
        self.needed.clear();
        self.center = None;
        self.edits = edits;
        let mut shared = self.worker.shared.lock();
        *shared = Shared {
            seed,
            epoch: self.epoch,
            edits: self.edits.clone(),
            ..Shared::default()
        };
    }

    pub fn cancel_jobs(&mut self, id: u32) {
        self.worker
            .shared
            .lock()
            .jobs
            .retain(|&(peer, _, tag)| peer != id || tag.is_some());
    }

    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    /// Keeps the chunks around the player and takes the chunks the worker finished.
    pub fn update(&mut self, center: Coord) {
        let mut shared = self.worker.shared.lock();
        for built in core::mem::take(&mut shared.built) {
            if built.epoch != self.epoch {
                continue;
            }
            self.requested.remove(&built.coord);
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
        let moved = self.center != Some(center);
        if !moved && !self.remote {
            return;
        }
        self.center = Some(center);
        shared.center = center;
        self.chunks
            .retain(|&coord, _| in_range(coord, center, RENDER_DISTANCE));
        if moved {
            self.requests
                .retain(|&c| in_range(c, center, RENDER_DISTANCE));
            self.requested
                .retain(|&c| in_range(c, center, RENDER_DISTANCE));
            self.in_flight
                .retain(|&c| in_range(c, center, RENDER_DISTANCE));
            self.required
                .retain(|&c, _| in_range(c, center, RENDER_DISTANCE));
            self.needed
                .retain(|&c, _| in_range(c, center, RENDER_DISTANCE));
            shared
                .supplied
                .retain(|&c, _| in_range(c, center, RENDER_DISTANCE));
            shared
                .urgent
                .retain(|&c| in_range(c, center, RENDER_DISTANCE));
            self.nearby.retain(|&coord, _| {
                (coord.0 - center.0).abs() <= 2 && (coord.1 - center.1).abs() <= 2
            });
        }
        shared
            .queue
            .retain(|&coord| in_range(coord, center, RENDER_DISTANCE));
        for z in -RENDER_DISTANCE..=RENDER_DISTANCE {
            for x in -RENDER_DISTANCE..=RENDER_DISTANCE {
                let coord = (center.0 + x, center.1 + z);
                if in_range(coord, center, RENDER_DISTANCE)
                    && !self.chunks.contains_key(&coord)
                    && !shared.loading(coord)
                    && !self.requested.contains(&coord)
                {
                    if self.remote {
                        self.requested.insert(coord);
                        self.requests.push(coord);
                    } else {
                        shared.queue.push(coord);
                    }
                }
            }
        }
        self.worker.wake.signal();
    }

    /// The blocks of a loaded chunk with its edits, for the integrated server.
    pub fn chunk(&self, coord: Coord) -> Option<ChunkData> {
        if let Some(chunk) = self.nearby.get(&coord) {
            return Some(chunk.clone());
        }
        let blocks = self
            .chunks
            .get(&coord)
            .map(|chunk| &chunk.blocks)
            .or_else(|| self.remote_blocks.get(&coord))?;
        let mut chunk = ChunkData::decode(blocks).ok()?;
        chunk.apply_edits(coord, &self.edits);
        Some(chunk)
    }

    /// The block at a world position, `None` while its chunk is not loaded.
    pub fn block(&mut self, position: [i64; 3]) -> Option<u8> {
        if position[1] < 0 || position[1] >= HEIGHT as i64 {
            return Some(0);
        }
        let size = CHUNK_SIZE as i64;
        let coord = (position[0].div_euclid(size), position[2].div_euclid(size));
        if !self.nearby.contains_key(&coord) {
            let blocks = self
                .chunks
                .get(&coord)
                .map(|c| &c.blocks)
                .or_else(|| self.remote_blocks.get(&coord))?;
            let mut data = ChunkData::decode(blocks).ok()?;
            if self.nearby.len() >= 32 {
                self.nearby.pop_first();
            }
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

    pub fn requests(&mut self) -> Vec<Coord> {
        let center = self.center.unwrap_or_default();
        // Nearest first, equal coords end up next to each other to dedup them
        self.requests
            .sort_by_key(|&c| ((c.0 - center.0).pow(2) + (c.1 - center.1).pow(2), c));
        self.requests.dedup();
        self.requests
            .retain(|&c| in_range(c, center, RENDER_DISTANCE));
        let mut out = Vec::new();
        while self.in_flight.len() < 8 && !self.requests.is_empty() {
            let coord = self.requests.remove(0);
            if self.in_flight.insert(coord) {
                out.push(coord);
            }
        }
        out
    }

    pub fn receive(&mut self, coord: Coord, revision: u64, chunk: Option<ChunkData>) {
        if !self.remote {
            return;
        }
        self.in_flight.remove(&coord);
        if self
            .required
            .get(&coord)
            .is_some_and(|&needed| revision < needed)
        {
            self.requests.push(coord);
            return;
        }
        let Some(chunk) = chunk else {
            self.requested.remove(&coord);
            return;
        };
        let mut shared = self.worker.shared.lock();
        shared.supplied.insert(coord, chunk);
        if !shared.queue.contains(&coord) {
            shared.queue.push(coord);
        }
        self.worker.wake.signal();
    }

    pub fn invalidate(&mut self, edits: &[Edit], revision: u64) {
        self.apply(edits);
        let size = CHUNK_SIZE as i64;
        for edit in edits {
            let c = (
                edit.position[0].div_euclid(size),
                edit.position[2].div_euclid(size),
            );
            for z in -1..=1 {
                for x in -1..=1 {
                    self.required.insert((c.0 + x, c.1 + z), revision);
                }
            }
        }
    }

    pub fn job(&mut self, id: u32, coord: Coord, tag: Option<u64>) -> bool {
        let mut shared = self.worker.shared.lock();
        if shared.jobs.len() >= 32 {
            return false;
        }
        shared.jobs.push_back((id, coord, tag));
        self.worker.wake.signal();
        true
    }

    pub fn results(&mut self) -> Vec<JobResult> {
        let results = core::mem::take(&mut self.worker.shared.lock().results);
        for result in &results {
            let distance = |c: Coord| {
                self.centers
                    .iter()
                    .map(|center| (c.0 - center.0).pow(2) + (c.1 - center.1).pow(2))
                    .min()
                    .unwrap_or(0)
            };
            if self.remote_blocks.len() >= 96 {
                let farthest = self
                    .remote_blocks
                    .keys()
                    .max_by_key(|&&c| distance(c))
                    .copied();
                if let Some(c) = farthest {
                    if distance(c) < distance(result.coord) {
                        continue;
                    }
                    self.remote_blocks.remove(&c);
                }
            }
            self.remote_blocks
                .insert(result.coord, result.chunk.encode());
        }
        results
    }

    /// Applies a whole edit batch, rebuilding each affected chunk just once.
    pub fn apply(&mut self, edits: &[Edit]) {
        if edits.is_empty() {
            return;
        }
        self.revision += 1;
        let size = CHUNK_SIZE as i64;
        let mut affected = BTreeSet::new();
        let mut combined: BTreeMap<_, _> = self
            .edits
            .iter()
            .map(|e| (e.position, e.material))
            .collect();
        for edit in edits {
            combined.insert(edit.position, edit.material);
            let center = (
                edit.position[0].div_euclid(size),
                edit.position[2].div_euclid(size),
            );
            for z in -1..=1 {
                for x in -1..=1 {
                    affected.insert((center.0 + x, center.1 + z));
                }
            }
        }
        self.edits = combined
            .into_iter()
            .map(|(position, material)| Edit { position, material })
            .collect();
        for (&coord, chunk) in &mut self.nearby {
            chunk.apply_edits(coord, edits);
        }
        let mut shared = self.worker.shared.lock();
        shared.edits = self.edits.clone();
        shared.revision += 1;
        shared.world_revision = self.revision;
        let revision = shared.revision;
        for coord in affected {
            if !self.chunks.contains_key(&coord) && !shared.loading(coord) {
                continue;
            }
            self.needed.insert(coord, revision);
            shared.queue.retain(|&queued| queued != coord);
            shared.urgent.retain(|&queued| queued != coord);
            if self.remote {
                self.requested.insert(coord);
                self.requests.push(coord);
            } else {
                shared.urgent.push(coord);
            }
        }
        self.worker.wake.signal();
    }
}

/// Generates lit chunks and keeps the unlit chunks next to the chunks still to build.
#[derive(Default)]
pub struct Generator {
    bases: BTreeMap<Coord, ChunkData>,
    seed: Option<u64>,
}

impl Generator {
    /// Generates a chunk with the edits, `pending` are the chunks that are built later.
    pub fn generate(
        &mut self,
        seed: u64,
        coord: Coord,
        edits: &[Edit],
        pending: &[Coord],
    ) -> ChunkData {
        if self.seed != Some(seed) {
            self.bases.clear();
            self.seed = Some(seed);
        }
        let near = |(x, z): Coord, base: Coord| (x - base.0).abs() <= 1 && (z - base.1).abs() <= 1;
        self.bases
            .retain(|&base, _| near(coord, base) || pending.iter().any(|&coord| near(coord, base)));
        for z in -1..=1 {
            for x in -1..=1 {
                let neighbor = (coord.0 + x, coord.1 + z);
                self.bases
                    .entry(neighbor)
                    .or_insert_with(|| ChunkData::generate_base(seed, neighbor));
            }
        }
        ChunkData::rebuild(seed, coord, &self.bases, edits)
    }
}

fn work(worker: &Worker) {
    let mut generator = Generator::default();
    let mut serve_guest = false;
    loop {
        let (coord, edits, revision, pending, seed, epoch, supplied, job, world_revision) = {
            let mut shared = worker.shared.lock();
            while shared.urgent.is_empty() && shared.queue.is_empty() && shared.jobs.is_empty() {
                shared = worker.wake.wait(shared);
            }
            let job = if shared.urgent.is_empty() && (shared.queue.is_empty() || serve_guest) {
                shared.jobs.pop_front()
            } else {
                None
            };
            serve_guest = !serve_guest;
            let coord = if let Some((_, coord, _)) = job {
                coord
            } else if !shared.urgent.is_empty() {
                shared.urgent.remove(0)
            } else {
                let center = shared.center;
                let nearest = (0..shared.queue.len())
                    .min_by_key(|&i| {
                        (shared.queue[i].0 - center.0).pow(2)
                            + (shared.queue[i].1 - center.1).pow(2)
                    })
                    .expect("Queue has chunks");
                shared.queue.swap_remove(nearest)
            };
            shared.building = Some(coord);
            // Edits rebuild the chunks around the player, so their bases are kept too
            let center = shared.center;
            let around = (-1..=1).flat_map(|z| (-1..=1).map(move |x| (center.0 + x, center.1 + z)));
            let pending: Vec<_> = shared
                .urgent
                .iter()
                .chain(&shared.queue)
                .copied()
                .chain(around)
                .collect();
            let supplied = shared.supplied.remove(&coord);
            (
                coord,
                shared.edits.clone(),
                shared.revision,
                pending,
                shared.seed,
                shared.epoch,
                supplied,
                job,
                shared.world_revision,
            )
        };
        let chunk = supplied.unwrap_or_else(|| generator.generate(seed, coord, &edits, &pending));
        if let Some((id, coord, tag)) = job {
            let mut shared = worker.shared.lock();
            if shared.epoch == epoch {
                shared.building = None;
                shared.results.push(JobResult {
                    id,
                    coord,
                    tag,
                    chunk,
                    revision: world_revision,
                });
            }
        } else {
            let built = Built {
                coord,
                revision,
                epoch,
                mesh: mesh::build(&chunk.mesh()),
                blocks: chunk.encode(),
            };
            let mut shared = worker.shared.lock();
            if shared.epoch == epoch {
                shared.building = None;
                shared.built.push(built);
            }
        }
    }
}
