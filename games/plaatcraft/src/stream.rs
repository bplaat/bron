/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! Requests the terrain around the player from the server, or generates it for the title
//! screen background, and meshes it for the GPU.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use plaatcraft_engine::protocol::Request;
use plaatcraft_engine::world::{self, ChunkData, Coord};

use crate::server::Connection;

pub(crate) type SectionRanges = [[Range<u32>; 2]; world::SECTION_COUNT];

pub(crate) const RESIDENT_CAPACITY: usize = resident_capacity(crate::settings::MAX_DISTANCE as i64);
const _: () = assert!(RESIDENT_CAPACITY <= 1 << 16);
const PRIORITY_INTERVAL: Duration = Duration::from_millis(100);

pub(crate) const fn in_range(coord: Coord, center: Coord, radius: i64) -> bool {
    let x = coord.0 - center.0;
    let z = coord.1 - center.1;
    x >= -radius && x <= radius && z >= -radius && z <= radius && x * x + z * z <= radius * radius
}

pub(crate) const fn resident_capacity(radius: i64) -> usize {
    let mut count = 0;
    let mut z = -radius;
    while z <= radius {
        let mut x = -radius;
        while x <= radius {
            if in_range((x, z), (0, 0), radius) {
                count += 1;
            }
            x += 1;
        }
        z += 1;
    }
    count
}
const JOBS_PER_WORKER: usize = 32;
const CACHE_CHUNKS: usize = 512;
const CACHE_BYTES: usize = 128 * 1024 * 1024;

/// Where a stream gets its terrain.
#[derive(Clone, Copy)]
pub(crate) enum Source<'a> {
    Server(&'a Connection),
    /// Generates an unsaved world from a seed, for the title screen background.
    Generate(u64),
}

enum Terrain {
    Loaded(Box<ChunkData>),
    Generate(u64),
}

struct Job {
    coord: Coord,
    revision: u64,
    terrain: Terrain,
}
type Meshed = (Coord, u64, Arc<CachedChunk>);

pub(crate) struct CachedChunk {
    // Terrain is persisted on disk; only reusable meshes remain in RAM.
    pub(crate) mesh: Vec<u8>,
    pub(crate) sections: SectionRanges,
}

pub(crate) struct Stream {
    pub(crate) cache: HashMap<Coord, Arc<CachedChunk>>,
    // Keep draw order distance-sorted, independently of load priority.
    pub(crate) desired: Vec<Coord>,
    pub(crate) loading: Vec<Coord>,
    last_priority: Instant,
    pub(crate) center: Coord,
    // Chunks requested from the server or being meshed.
    pending: HashSet<Coord>,
    // Whether the terrain requests the server still has queued are stale.
    cancel: bool,
    jobs: Sender<Job>,
    results: Receiver<Meshed>,
    pub(crate) worker_count: usize,
    uploaded: HashSet<Coord>,
    cache_order: VecDeque<Coord>,
    cache_bytes: usize,
    workers: Vec<thread::JoinHandle<()>>,
    revision: u64,
    pub(crate) radius: i64,
    desired_radius: i64,
}

pub(crate) fn nearby(center: Coord, radius: i64) -> Vec<Coord> {
    let mut coords = Vec::with_capacity(resident_capacity(radius));
    for z in -radius..=radius {
        for x in -radius..=radius {
            let coord = (center.0 + x, center.1 + z);
            if in_range(coord, center, radius) {
                coords.push(coord);
            }
        }
    }
    coords.sort_unstable_by_key(|&(x, z)| ((x - center.0).pow(2) + (z - center.1).pow(2), x, z));
    coords
}

fn load_order(coords: &[Coord], center: Coord, visible: impl Fn(Coord) -> bool) -> Vec<Coord> {
    let mut priorities: Vec<_> = coords
        .iter()
        .map(|&coord| {
            let distance = (coord.0 - center.0).pow(2) + (coord.1 - center.1).pow(2);
            let tier = if distance <= 4 {
                0
            } else if visible(coord) {
                1
            } else {
                2
            };
            (tier, distance, coord)
        })
        .collect();
    priorities.sort_unstable();
    priorities.into_iter().map(|(_, _, coord)| coord).collect()
}

// Terrain work yields to the main thread, which renders and handles input, so loading
// chunks does not drop frames.
pub(crate) fn lower_thread_priority() {
    cfg_select! {
        any(target_os = "linux", target_os = "android") => {
            unsafe extern "C" {
                fn setpriority(which: i32, who: u32, priority: i32) -> i32;
            }
            // SAFETY: On Linux, PRIO_PROCESS with id 0 only changes the calling thread's nice value.
            unsafe { setpriority(0, 0, 10) };
        }
        target_vendor = "apple" => {
            unsafe extern "C" {
                fn pthread_set_qos_class_self_np(class: u32, relative_priority: i32) -> i32;
            }
            const QOS_CLASS_UTILITY: u32 = 0x11;
            // SAFETY: This only changes the scheduling class of the calling thread.
            unsafe { pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0) };
        }
        windows => {
            #[link(name = "kernel32")]
            unsafe extern "system" {
                fn GetCurrentThread() -> *mut std::ffi::c_void;
                fn SetThreadPriority(thread: *mut std::ffi::c_void, priority: i32) -> i32;
            }
            const THREAD_PRIORITY_BELOW_NORMAL: i32 = -1;
            // SAFETY: The pseudo handle of the current thread is always valid.
            unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
        }
        _ => {}
    }
}

fn mesh(terrain: &ChunkData) -> CachedChunk {
    let mesh = terrain.mesh();
    let mut sections: SectionRanges = std::array::from_fn(|_| [0..0, 0..0]);
    for (index, quad) in mesh
        .as_chunks::<{ world::QUAD_BYTES }>()
        .0
        .iter()
        .enumerate()
    {
        let packed = u32::from_ne_bytes(quad[..4].try_into().expect("Packed quad word"));
        let section = ((packed >> 27) & 15) as usize;
        let translucent = usize::from(world::is_translucent(world::quad_material(packed)));
        let range = &mut sections[section][translucent];
        if range.start == range.end {
            range.start = index as u32;
        }
        range.end = index as u32 + 1;
    }
    CachedChunk { mesh, sections }
}

impl Stream {
    /// Streams a world whose edits are at a revision.
    pub(crate) fn new(revision: u64) -> Self {
        // Leave one core for the main thread, which renders and runs the physics.
        let available = thread::available_parallelism().map_or(1, usize::from);
        Self::with_workers(available.saturating_sub(1).max(1), revision)
    }

    fn with_workers(worker_count: usize, revision: u64) -> Self {
        assert!(worker_count > 0);
        let (jobs, queue) = mpsc::channel::<Job>();
        let queue = Arc::new(Mutex::new(queue));
        let (finished, results) = mpsc::channel();
        let workers = (0..worker_count)
            .map(|index| {
                let (queue, finished) = (queue.clone(), finished.clone());
                thread::Builder::new()
                    .name(format!("mesh-{index}"))
                    .spawn(move || {
                        lower_thread_priority();
                        loop {
                            // Release the queue lock before doing expensive work.
                            let job = { queue.lock().expect("Mesh queue poisoned").recv() };
                            let Ok(job) = job else { break };
                            let terrain = match job.terrain {
                                Terrain::Loaded(terrain) => *terrain,
                                Terrain::Generate(seed) => ChunkData::generate(seed, job.coord),
                            };
                            let chunk = Arc::new(mesh(&terrain));
                            if finished.send((job.coord, job.revision, chunk)).is_err() {
                                break;
                            }
                        }
                    })
                    .expect("Start mesh worker")
            })
            .collect();
        // Workers exit when Stream drops: job/result channel disconnection unblocks them.
        Self {
            cache: HashMap::new(),
            desired: Vec::new(),
            loading: Vec::new(),
            last_priority: Instant::now(),
            center: (0, 0),
            pending: HashSet::new(),
            cancel: false,
            jobs,
            results,
            worker_count,
            uploaded: HashSet::new(),
            cache_order: VecDeque::new(),
            cache_bytes: 0,
            workers,
            revision,
            radius: i64::from(crate::settings::DEFAULT_DISTANCE),
            desired_radius: i64::from(crate::settings::DEFAULT_DISTANCE),
        }
    }

    pub(crate) fn invalidate(&mut self, center: Coord, revision: u64) -> Vec<Coord> {
        self.revision = revision;
        self.cancel = true;
        let mut affected = Vec::new();
        for z in center.1 - 1..=center.1 + 1 {
            for x in center.0 - 1..=center.0 + 1 {
                let coord = (x, z);
                if let Some(old) = self.cache.remove(&coord) {
                    self.cache_bytes -= old.mesh.capacity();
                }
                self.cache_order.retain(|&c| c != coord);
                self.uploaded.remove(&coord);
                affected.push(coord);
            }
        }
        affected
    }

    /// Meshes terrain from the server, unless it was canceled or is from an old revision.
    pub(crate) fn receive(&mut self, coord: Coord, revision: u64, terrain: Option<ChunkData>) {
        match terrain {
            Some(terrain) if revision == self.revision => self
                .jobs
                .send(Job {
                    coord,
                    revision,
                    terrain: Terrain::Loaded(Box::new(terrain)),
                })
                .expect("Queue chunk meshing"),
            _ => {
                self.pending.remove(&coord);
            }
        }
    }

    pub(crate) fn mark_uploaded(&mut self, coord: Coord) {
        self.uploaded.insert(coord);
    }

    fn cache_chunk(&mut self, coord: Coord, chunk: Arc<CachedChunk>) {
        if let Some(old) = self.cache.remove(&coord) {
            self.cache_bytes -= old.mesh.capacity();
            self.cache_order.retain(|&c| c != coord);
        }
        self.cache_bytes += chunk.mesh.capacity();
        self.cache.insert(coord, chunk);
        self.cache_order.push_back(coord);
        self.trim_cache(CACHE_CHUNKS, CACHE_BYTES);
    }

    fn trim_cache(&mut self, chunks: usize, bytes: usize) {
        while self.cache.len() > chunks || self.cache_bytes > bytes {
            // Prefer evicting meshes already uploaded; never reload them just to fill RAM.
            let index = self
                .cache_order
                .iter()
                .position(|c| self.uploaded.contains(c))
                .unwrap_or(0);
            let oldest = self
                .cache_order
                .remove(index)
                .expect("Nonempty cache order");
            let removed = self.cache.remove(&oldest).expect("Cached chunk");
            self.cache_bytes -= removed.mesh.capacity();
        }
    }

    pub(crate) fn update(
        &mut self,
        source: Source<'_>,
        center: Coord,
        visible: impl Fn(Coord) -> bool,
    ) {
        let moved =
            self.desired.is_empty() || self.center != center || self.radius != self.desired_radius;
        if moved {
            self.center = center;
            self.desired_radius = self.radius;
            self.desired = nearby(center, self.radius);
            self.uploaded
                .retain(|&coord| in_range(coord, center, self.radius));
        }
        if moved || self.last_priority.elapsed() >= PRIORITY_INTERVAL {
            let loading = load_order(&self.desired, center, visible);
            if loading != self.loading {
                // Queued stale priorities are canceled; already generated chunks stay cached.
                self.cancel = true;
                self.loading = loading;
            }
            self.last_priority = Instant::now();
        }
        if std::mem::take(&mut self.cancel)
            && let Source::Server(server) = source
        {
            server.send(Request::CancelTerrain);
        }
        loop {
            match self.results.try_recv() {
                Ok((coord, revision, chunk)) => {
                    self.pending.remove(&coord);
                    if revision == self.revision {
                        self.cache_chunk(coord, chunk);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => panic!("Mesh worker stopped unexpectedly"),
            }
        }
        for &coord in &self.loading {
            if self.pending.len() >= self.worker_count * JOBS_PER_WORKER {
                break;
            }
            if !self.uploaded.contains(&coord)
                && !self.cache.contains_key(&coord)
                && self.pending.insert(coord)
            {
                match source {
                    Source::Server(server) => server.send(Request::Terrain(coord)),
                    Source::Generate(seed) => self
                        .jobs
                        .send(Job {
                            coord,
                            revision: self.revision,
                            terrain: Terrain::Generate(seed),
                        })
                        .expect("Queue chunk generation"),
                }
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        // Disconnect both queues before joining, including workers blocked on sending.
        self.jobs = mpsc::channel().0;
        self.results = mpsc::channel().1;
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                eprintln!("Mesh worker failed during shutdown");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use plaatcraft_engine::protocol::Response;

    use super::*;
    use crate::database::Database;
    use crate::database::tests::TestWorld;

    fn connect() -> (TestWorld, Connection, Stream) {
        let world = TestWorld::new();
        let (server, welcome) = crate::server::tests::host(&world);
        (world, server, Stream::with_workers(1, welcome.revision))
    }

    // Hands the terrain to the stream and returns the revision of the last saved edit.
    fn pump(stream: &mut Stream, server: &mut Connection) -> Option<u64> {
        let mut edited = None;
        while let Some(response) = server.receive() {
            match response {
                Response::Terrain {
                    coord,
                    revision,
                    chunk,
                } => stream.receive(coord, revision, chunk),
                Response::Edited { revision, .. } => edited = revision,
                _ => {}
            }
        }
        edited
    }

    fn wait_for_center(stream: &mut Stream, server: &mut Connection, coord: Coord) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !stream.cache.contains_key(&coord) {
            pump(stream, server);
            stream.update(Source::Server(server), coord, |_| true);
            assert!(Instant::now() < deadline, "Terrain timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn changing_distance_rebuilds_residency_without_moving() {
        let (_world, server, mut stream) = connect();
        let center = (-40, 73);
        stream.radius = 4;
        stream.update(Source::Server(&server), center, |_| true);
        assert_eq!(stream.desired.len(), resident_capacity(4));
        stream.radius = i64::from(crate::settings::MAX_DISTANCE);
        stream.update(Source::Server(&server), center, |_| true);
        assert_eq!(stream.desired.len(), RESIDENT_CAPACITY);
        let distant = (center.0 + stream.radius, center.1);
        assert!(stream.desired.contains(&distant));
        stream.mark_uploaded(distant);
        stream.radius = 4;
        stream.update(Source::Server(&server), center, |_| true);
        assert!(!stream.uploaded.contains(&distant));
        assert!(!stream.loading.contains(&distant));
        assert_eq!(stream.desired.len(), resident_capacity(4));
    }

    #[test]
    fn edited_chunks_replace_the_old_cached_mesh() {
        let (_world, mut server, mut stream) = connect();
        stream.radius = 4;
        wait_for_center(&mut stream, &mut server, (0, 0));
        let original = stream.cache[&(0, 0)].clone();
        stream.mark_uploaded((0, 0));
        server.send(Request::Edit(world::Edit {
            position: [0, 120, 0],
            material: 3,
        }));
        let revision = loop {
            if let Some(revision) = pump(&mut stream, &mut server) {
                break revision;
            }
            thread::sleep(Duration::from_millis(1));
        };
        stream.invalidate((0, 0), revision);
        assert!(!stream.uploaded.contains(&(0, 0)));
        wait_for_center(&mut stream, &mut server, (0, 0));
        assert_ne!(stream.cache[&(0, 0)].mesh, original.mesh);
    }

    #[test]
    fn generated_streams_need_no_server() {
        let mut stream = Stream::with_workers(1, 0);
        stream.radius = 2;
        let deadline = Instant::now() + Duration::from_secs(10);
        while stream.cache.len() < resident_capacity(2) {
            stream.update(Source::Generate(world::WORLD_SEED), (0, 0), |_| true);
            assert!(Instant::now() < deadline, "Terrain timed out");
            thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            stream.cache[&(0, 0)].mesh,
            ChunkData::generate(world::WORLD_SEED, (0, 0)).mesh()
        );
    }

    #[test]
    fn terrain_of_old_revisions_is_not_meshed() {
        let mut stream = Stream::with_workers(1, 3);
        stream.pending.insert((0, 0));
        stream.receive((0, 0), 2, Some(ChunkData::from_heights([60; 18 * 18])));
        assert!(stream.pending.is_empty());
        stream.pending.insert((0, 0));
        stream.receive((0, 0), 3, None);
        assert!(stream.pending.is_empty());
    }

    #[test]
    fn cache_caps_evict_uploaded_meshes_without_requesting_them_again() {
        let (_world, server, mut stream) = connect();
        let chunk = Arc::new(CachedChunk {
            mesh: vec![0; 64],
            sections: std::array::from_fn(|_| [0..0, 0..0]),
        });
        for x in 0..CACHE_CHUNKS as i64 + 10 {
            stream.cache_chunk((x, 0), chunk.clone());
            assert!(stream.cache.len() <= CACHE_CHUNKS);
        }
        assert_eq!(stream.cache_order.len(), stream.cache.len());
        stream.mark_uploaded((100, 0));
        stream.trim_cache(CACHE_CHUNKS - 1, CACHE_BYTES);
        assert!(!stream.cache.contains_key(&(100, 0)));
        assert!(stream.uploaded.contains(&(100, 0)));
        stream.trim_cache(CACHE_CHUNKS, 128);
        assert!(stream.cache_bytes <= 128);
        assert_eq!(
            stream.cache_bytes,
            stream
                .cache
                .values()
                .map(|c| c.mesh.capacity())
                .sum::<usize>()
        );
        stream.update(Source::Server(&server), (100, 0), |_| true);
        assert!(!stream.pending.contains(&(100, 0)));
    }

    #[test]
    fn evicted_chunks_reload_from_sqlite() {
        let world = TestWorld::new();
        // Store a distinctive flat chunk to prove loading bypasses world generation.
        let terrain = ChunkData::from_heights([60; (world::CHUNK_SIZE + 2).pow(2)]);
        Database::open(&world.path)
            .unwrap()
            .store((0, 0), &terrain)
            .unwrap();
        let (mut server, welcome) = crate::server::tests::host(&world);
        let mut stream = Stream::with_workers(1, welcome.revision);
        // Limit residency so unrelated generation cannot queue ahead of the reload.
        stream.desired = vec![(0, 0)];
        stream.loading = stream.desired.clone();
        wait_for_center(&mut stream, &mut server, (0, 0));
        let mesh = stream.cache[&(0, 0)].mesh.clone();
        assert_ne!(mesh, ChunkData::generate(world::WORLD_SEED, (0, 0)).mesh());
        stream.trim_cache(0, 0);
        assert!(stream.cache.is_empty());
        wait_for_center(&mut stream, &mut server, (0, 0));
        assert_eq!(stream.cache[&(0, 0)].mesh, mesh);
    }

    #[test]
    fn residency_is_circular_and_translation_invariant() {
        assert_eq!(resident_capacity(24), 1793);
        assert_eq!(resident_capacity(48), 7213);
        let radius = i64::from(crate::settings::DEFAULT_DISTANCE);
        for center in [(0, 0), (-40, 73)] {
            let coords = nearby(center, radius);
            assert_eq!(coords.len(), resident_capacity(radius));
            assert!(coords.iter().all(|&coord| in_range(coord, center, radius)));
            assert!(coords.contains(&(center.0 + radius, center.1)));
            assert!(!coords.contains(&(center.0 + radius, center.1 + radius)));
            assert!(!in_range((center.0 + radius + 1, center.1), center, radius));
            assert_eq!(
                coords.iter().copied().collect::<HashSet<_>>().len(),
                coords.len()
            );
        }
    }

    #[test]
    fn nearby_chunks_come_first_then_visible_chunks() {
        let coords = nearby((0, 0), 32);
        let ordered = load_order(&coords, (0, 0), |(_, z)| z < 0);
        let index = |coord| ordered.iter().position(|&c| c == coord).unwrap();
        assert_eq!(ordered[0], (0, 0));
        assert!(index((0, 2)) < index((0, -3)));
        assert!(index((0, -12)) < index((0, 3)));
        assert_eq!(ordered.len(), coords.len());
    }

    #[test]
    fn turning_reprioritizes_without_unloading_or_reordering_draws() {
        let (_world, server, mut stream) = connect();
        stream.update(Source::Server(&server), (0, 0), |(_, z)| z < 0);
        let draws = stream.desired.clone();
        stream.last_priority = Instant::now() - PRIORITY_INTERVAL;
        stream.update(Source::Server(&server), (0, 0), |(_, z)| z > 0);
        let index = |coord| stream.loading.iter().position(|&c| c == coord).unwrap();
        assert!(index((0, 12)) < index((0, -3)));
        assert!(!stream.cancel);
        assert_eq!(stream.desired, draws);
        assert!(stream.pending.len() <= stream.worker_count * JOBS_PER_WORKER);
    }

    #[test]
    fn rapid_moves_cancel_stale_work_without_losing_the_new_center() {
        let (_world, mut server, mut stream) = connect();
        for step in 0..20 {
            stream.update(Source::Server(&server), (step * 100, -step * 100), |_| true);
            assert!(stream.pending.len() <= JOBS_PER_WORKER);
        }
        wait_for_center(&mut stream, &mut server, (-1000, 1000));
        assert_eq!(stream.center, (-1000, 1000));
    }

    #[test]
    fn streaming_reuses_visited_chunks_and_bounds_pending_work() {
        let (_world, mut server, mut stream) = connect();
        stream.radius = 8;
        wait_for_center(&mut stream, &mut server, (0, 0));
        let original = stream.cache[&(0, 0)].clone();
        let mut covered = 0;
        for (section, ranges) in original.sections.iter().enumerate() {
            for (translucent, range) in ranges.iter().enumerate() {
                for index in range.clone() {
                    let start = index as usize * world::QUAD_BYTES;
                    let a = u32::from_ne_bytes(original.mesh[start..start + 4].try_into().unwrap());
                    assert_eq!(((a >> 27) & 15) as usize, section);
                    assert_eq!(
                        usize::from(world::is_translucent(world::quad_material(a))),
                        translucent
                    );
                    covered += 1;
                }
            }
        }
        assert_eq!(covered * world::QUAD_BYTES, original.mesh.len());
        wait_for_center(&mut stream, &mut server, (-32, 12));
        assert!(stream.pending.len() <= stream.worker_count * JOBS_PER_WORKER);
        stream.update(Source::Server(&server), (0, 0), |_| true);
        assert!(Arc::ptr_eq(&original, &stream.cache[&(0, 0)]));
        assert!(stream.cache.contains_key(&(-32, 12)));
        assert_eq!(stream.desired.len(), resident_capacity(stream.radius));
        assert_eq!(stream.desired[0], (0, 0));
    }
}
