/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Loads, generates and meshes the chunks around the player on a worker thread. Loaded
//! chunks keep their display list and their compressed blocks, the chunks next to the
//! player are decoded for collision, picking and fluid ticks.
//!
//! The worker owns the blocks of the open world: it reads the stored chunks from the SD
//! card, generates the missing ones, applies the edits and keeps the changed chunks in
//! memory until a save writes them, because SD card writes are slow. Lighting a chunk needs
//! its neighbors, so it keeps the chunks next to the chunks it still has to build.

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;

use plaatcraft_engine::world::{self, CHUNK_SIZE, ChunkData, Coord, Edit, HEIGHT, WORLD_SEED};

use crate::local::Saved;
use crate::mesh::{self, Mesh};
use crate::storage::Sqlite;
use crate::sync::{Condvar, Mutex};

pub const RENDER_DISTANCE: i64 = 8;
// Lower than the main thread, so generation uses the time the main thread waits for vsync
const WORKER_PRIORITY: u8 = 40;
const WORKER_STACK: usize = 256 * 1024;
// The changed chunks the worker keeps before it writes them without waiting for a save,
// about 2 KiB each
const MAX_UNSAVED: usize = 256;

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
    // The edits the worker still has to apply to the world's chunks
    edits: Vec<Edit>,
    // Counts the edits, so chunks built before an edit are built again
    revision: u32,
    world_revision: u64,
    built: Vec<Built>,
    supplied: BTreeMap<Coord, ChunkData>,
    jobs: VecDeque<(u32, Coord, Option<u64>)>,
    results: Vec<JobResult>,
    // The save the worker still has to write, while it writes one, and its failure
    save: Option<Saved>,
    saving: bool,
    error: Option<String>,
}

impl Shared {
    fn loading(&self, coord: Coord) -> bool {
        self.building == Some(coord) || self.queue.contains(&coord) || self.urgent.contains(&coord)
    }
}

struct Worker {
    shared: Mutex<Shared>,
    wake: Condvar,
    saved: Condvar,
    sqlite: &'static Sqlite,
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
    // The last edit of each block this session by chunk. The compressed blocks of chunks that
    // are still rebuilt, and of guests' chunks built before an edit, miss them.
    edits: BTreeMap<Coord, Vec<Edit>>,
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
    pub fn start(sqlite: &'static Sqlite) -> Self {
        let worker: &'static Worker = Box::leak(Box::new(Worker {
            shared: Mutex::new(Shared {
                seed: WORLD_SEED,
                ..Shared::default()
            }),
            wake: Condvar::new(),
            saved: Condvar::new(),
            sqlite,
        }));
        crate::sync::spawn(WORKER_STACK, WORKER_PRIORITY, move || work(worker));
        Self {
            worker,
            chunks: BTreeMap::new(),
            needed: BTreeMap::new(),
            nearby: BTreeMap::new(),
            edits: BTreeMap::new(),
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

    /// Streams another world, the world of this Wii is saved and closed first.
    pub fn reset(&mut self, seed: u64) {
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
        self.edits.clear();
        let mut shared = self.worker.shared.lock();
        *shared = Shared {
            seed,
            epoch: self.epoch,
            ..Shared::default()
        };
    }

    /// Saves the world in the background with the changed chunks.
    pub fn save(&mut self, saved: Saved) {
        self.worker.shared.lock().save = Some(saved);
        self.worker.wake.signal();
    }

    /// Waits until the worker wrote the last save.
    pub fn flush(&mut self) -> Result<(), String> {
        let mut shared = self.worker.shared.lock();
        while shared.save.is_some() || shared.saving {
            shared = self.worker.saved.wait(shared);
        }
        shared.error.take().map_or(Ok(()), Err)
    }

    /// Why the last background save failed.
    pub fn error(&mut self) -> Option<String> {
        self.worker.shared.lock().error.take()
    }

    pub fn cancel_jobs(&mut self, id: u32) {
        self.worker
            .shared
            .lock()
            .jobs
            .retain(|&(peer, _, tag)| peer != id || tag.is_some());
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
        chunk.apply_edits(coord, self.edits_of(coord));
        Some(chunk)
    }

    fn edits_of(&self, coord: Coord) -> &[Edit] {
        self.edits.get(&coord).map_or(&[], Vec::as_slice)
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
            data.apply_edits(coord, self.edits_of(coord));
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
        for edit in edits {
            let coord = world::chunk_of(edit.position);
            for index in 0..9 {
                self.required
                    .insert(world::neighbor(coord, index), revision);
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
        let edited: BTreeSet<_> = edits
            .iter()
            .map(|edit| world::chunk_of(edit.position))
            .collect();
        let affected: BTreeSet<_> = edited
            .iter()
            .flat_map(|&coord| (0..9).map(move |index| world::neighbor(coord, index)))
            .collect();
        for &edit in edits {
            let chunk = self
                .edits
                .entry(world::chunk_of(edit.position))
                .or_default();
            chunk.retain(|old| old.position != edit.position);
            chunk.push(edit);
        }
        for (&coord, chunk) in &mut self.nearby {
            chunk.apply_edits(coord, edits);
        }
        let mut shared = self.worker.shared.lock();
        // The server's chunks of a LAN world have its edits
        if !self.remote {
            shared.edits.extend_from_slice(edits);
        }
        shared.revision += 1;
        shared.world_revision = self.revision;
        let revision = shared.revision;
        // The edited chunks go before their neighbors, whose light only changes a little, and
        // before older rebuilds. The worker gets only the time the main thread leaves per frame,
        // so building all nine chunks before the edited one shows the change half a second late.
        let rebuild: Vec<_> = edited
            .iter()
            .chain(affected.difference(&edited))
            .copied()
            .filter(|&coord| self.chunks.contains_key(&coord) || shared.loading(coord))
            .collect();
        shared.queue.retain(|queued| !rebuild.contains(queued));
        shared.urgent.retain(|queued| !rebuild.contains(queued));
        for &coord in &rebuild {
            self.needed.insert(coord, revision);
        }
        if self.remote {
            self.requested.extend(&rebuild);
            self.requests.extend(&rebuild);
        } else {
            shared.urgent.splice(0..0, rebuild);
        }
        self.worker.wake.signal();
    }
}

/// The blocks of the world the worker streams. With a world of this Wii open, the chunks are
/// read from it, the generated ones are stored once they are built and the changed ones are
/// kept until they are saved, otherwise they are only generated, like the title screen's.
#[derive(Default)]
pub struct Store {
    seed: u64,
    // The decoded blocks of the chunks near the ones still to build
    chunks: BTreeMap<Coord, ChunkData>,
    // The chunks that are generated but not stored, the neighbors of built chunks
    generated: BTreeSet<Coord>,
    // The decoded chunks that changed, and the encoded ones that are no longer near
    changed: BTreeSet<Coord>,
    unsaved: BTreeMap<Coord, Vec<u8>>,
    // What the last save wrote besides the chunks
    saved: Option<Saved>,
}

impl Store {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            ..Self::default()
        }
    }

    // The blocks of a chunk, from memory, the open world or the generator.
    fn load(&mut self, sqlite: &Sqlite, coord: Coord) -> &mut ChunkData {
        if !self.chunks.contains_key(&coord) {
            let unsaved = self.unsaved.remove(&coord);
            let chunk = match unsaved {
                Some(data) => {
                    self.changed.insert(coord);
                    ChunkData::decode_blocks(&data).ok()
                }
                None => sqlite.lock().as_ref().and_then(|db| match db.chunk(coord) {
                    Ok(None) => {
                        self.generated.insert(coord);
                        None
                    }
                    // An unreadable chunk is generated again, but stays intact on the card
                    stored => stored.ok().flatten(),
                }),
            };
            let chunk = chunk.unwrap_or_else(|| ChunkData::generate_blocks(self.seed, coord));
            self.chunks.insert(coord, chunk);
        }
        self.chunks.get_mut(&coord).expect("Loaded chunk")
    }

    /// Lights a chunk, `pending` are the chunks that are built later, so their neighbors stay.
    pub fn build(&mut self, sqlite: &Sqlite, coord: Coord, pending: &[Coord]) -> ChunkData {
        let near = |(x, z): Coord, base: Coord| (x - base.0).abs() <= 1 && (z - base.1).abs() <= 1;
        let far: Vec<_> = self
            .chunks
            .keys()
            .copied()
            .filter(|&base| !near(coord, base) && !pending.iter().any(|&c| near(c, base)))
            .collect();
        for base in far {
            let chunk = self.chunks.remove(&base).expect("Kept chunk");
            self.generated.remove(&base);
            if self.changed.remove(&base) {
                self.unsaved.insert(base, chunk.encode_blocks());
            }
        }
        for index in 0..9 {
            self.load(sqlite, world::neighbor(coord, index));
        }
        // A built chunk is stored, like Minecraft keeps every chunk it loaded
        if self.generated.remove(&coord) {
            self.changed.insert(coord);
        }
        let chunks: [&ChunkData; 9] =
            core::array::from_fn(|index| &self.chunks[&world::neighbor(coord, index)]);
        ChunkData::lit(chunks)
    }

    fn edit(&mut self, sqlite: &Sqlite, edits: &[Edit]) {
        let coords: BTreeSet<_> = edits
            .iter()
            .map(|edit| world::chunk_of(edit.position))
            .collect();
        for coord in coords {
            self.load(sqlite, coord).apply_edits(coord, edits);
            self.generated.remove(&coord);
            self.changed.insert(coord);
        }
    }

    // Writes the changed chunks and the saved state when it changed, the chunks alone
    // without one.
    fn save(&mut self, sqlite: &Sqlite, saved: Option<Saved>) -> Result<(), String> {
        let saved = saved.filter(|saved| self.saved.as_ref() != Some(saved));
        if saved.is_none() && self.changed.is_empty() && self.unsaved.is_empty() {
            return Ok(());
        }
        for coord in core::mem::take(&mut self.changed) {
            self.unsaved
                .insert(coord, self.chunks[&coord].encode_blocks());
        }
        let sqlite = sqlite.lock();
        let db = sqlite.as_ref().ok_or("The world is closed")?;
        db.save(&self.unsaved, saved.as_ref())?;
        self.unsaved.clear();
        if saved.is_some() {
            self.saved = saved;
        }
        Ok(())
    }
}

fn work(worker: &Worker) {
    let mut store = Store::new(WORLD_SEED);
    let mut store_epoch = 0;
    // A failed save waits for more changed chunks before the next try
    let mut limit = MAX_UNSAVED;
    let mut serve_guest = false;
    loop {
        let unsaved = |store: &Store| store.changed.len() + store.unsaved.len();
        let full = unsaved(&store) > limit;
        let mut shared = worker.shared.lock();
        while !full
            && shared.urgent.is_empty()
            && shared.queue.is_empty()
            && shared.jobs.is_empty()
            && shared.edits.is_empty()
            && shared.save.is_none()
        {
            shared = worker.wake.wait(shared);
        }
        if shared.epoch != store_epoch {
            store = Store::new(shared.seed);
            store_epoch = shared.epoch;
            limit = MAX_UNSAVED;
        }
        let edits = core::mem::take(&mut shared.edits);
        let save = shared.save.take();
        shared.saving = save.is_some();
        // The edits and saves go before the chunks, which are built with the edits
        if !edits.is_empty() || save.is_some() || full {
            drop(shared);
            store.edit(worker.sqlite, &edits);
            let mut result = Ok(());
            if save.is_some() || unsaved(&store) > limit {
                result = store.save(worker.sqlite, save);
                limit = unsaved(&store) + MAX_UNSAVED;
            }
            let mut shared = worker.shared.lock();
            shared.saving = false;
            if let Err(error) = result {
                shared.error = Some(error);
            }
            worker.saved.signal();
            continue;
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
                    (shared.queue[i].0 - center.0).pow(2) + (shared.queue[i].1 - center.1).pow(2)
                })
                .expect("Queue has chunks");
            shared.queue.swap_remove(nearest)
        };
        shared.building = Some(coord);
        // Edits rebuild the chunks around the player, so their neighbors are kept too
        let center = shared.center;
        let around = (0..9).map(move |index| world::neighbor(center, index));
        let jobs = shared.jobs.iter().map(|&(_, coord, _)| coord);
        let pending: Vec<_> = shared
            .urgent
            .iter()
            .chain(&shared.queue)
            .copied()
            .chain(jobs)
            .chain(around)
            .collect();
        let supplied = shared.supplied.remove(&coord);
        let (revision, epoch, world_revision) =
            (shared.revision, shared.epoch, shared.world_revision);
        drop(shared);
        let chunk = supplied.unwrap_or_else(|| store.build(worker.sqlite, coord, &pending));
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
