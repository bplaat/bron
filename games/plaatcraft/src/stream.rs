/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ops::Range;

use crate::world::{self, ChunkData, Coord};

pub(crate) type SectionRanges = [[Range<u32>; 2]; world::SECTION_COUNT];
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub(crate) const RENDER_DISTANCE: i64 = 48;
pub(crate) const RESIDENT_CAPACITY: usize = resident_capacity(RENDER_DISTANCE);
const _: () = assert!(RESIDENT_CAPACITY <= 1 << 14);
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

struct Job {
    coord: Coord,
    epoch: u64,
}
type ChunkResult = (Coord, u64, Option<Arc<CachedChunk>>);

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
    pending: HashSet<Coord>,
    jobs: Sender<Job>,
    results: Receiver<ChunkResult>,
    epoch: Arc<AtomicU64>,
    pub(crate) worker_count: usize,
    uploaded: HashSet<Coord>,
    cache_order: VecDeque<Coord>,
    cache_bytes: usize,
    workers: Vec<thread::JoinHandle<()>>,
    revision: u64,
}

pub(crate) fn nearby(center: Coord) -> Vec<Coord> {
    let radius = RENDER_DISTANCE;
    let mut coords = Vec::with_capacity(RESIDENT_CAPACITY);
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

impl Stream {
    pub(crate) fn open(path: &str) -> Self {
        let available = thread::available_parallelism().map_or(1, usize::from);
        Self::start(
            available.saturating_sub(1).clamp(1, 4),
            Some(path.to_owned()),
        )
    }

    #[cfg(test)]
    pub(crate) fn with_workers(worker_count: usize) -> Self {
        Self::start(worker_count, None)
    }

    fn start(worker_count: usize, path: Option<String>) -> Self {
        assert!((1..=4).contains(&worker_count));
        let revision = path.as_ref().map_or(0, |path| {
            crate::database::Database::open(path)
                .expect("Open world database")
                .revision()
                .expect("Read edit revision")
        });
        let (jobs, requests) = mpsc::channel::<Job>();
        let requests = Arc::new(Mutex::new(requests));
        let epoch = Arc::new(AtomicU64::new(0));
        let (finished, results) = mpsc::sync_channel(worker_count * JOBS_PER_WORKER);
        let mut workers = Vec::new();
        for index in 0..worker_count {
            let path = path.clone();
            let requests = requests.clone();
            let finished = finished.clone();
            let epoch = epoch.clone();
            workers.push(
                thread::Builder::new()
                    .name(format!("terrain-{index}"))
                    .spawn(move || {
                        let database = path.map(|path| {
                            crate::database::Database::open(&path).expect("Open worker database")
                        });
                        loop {
                            // Release the receiver lock before doing expensive work.
                            let job = { requests.lock().expect("Terrain queue poisoned").recv() };
                            let Ok(job) = job else { break };
                            let mut generated_revision = 0;
                            let chunk = if job.epoch == epoch.load(Ordering::Relaxed) {
                                let terrain = if let Some(db) = &database {
                                    let (terrain, revision) =
                                        db.terrain(job.coord).expect("Load or generate chunk");
                                    generated_revision = revision;
                                    terrain
                                } else {
                                    ChunkData::generate(job.coord)
                                };
                                let mesh = terrain.mesh();
                                let mut sections: SectionRanges =
                                    std::array::from_fn(|_| [0..0, 0..0]);
                                for (index, quad) in mesh.as_chunks::<8>().0.iter().enumerate() {
                                    let packed = u32::from_ne_bytes(
                                        quad[..4].try_into().expect("Packed quad word"),
                                    );
                                    let section = ((packed >> 27) & 15) as usize;
                                    let translucent =
                                        usize::from(world::is_translucent((packed >> 20) & 63));
                                    let range = &mut sections[section][translucent];
                                    if range.start == range.end {
                                        range.start = index as u32;
                                    }
                                    range.end = index as u32 + 1;
                                }
                                Some(Arc::new(CachedChunk { mesh, sections }))
                            } else {
                                None
                            };
                            if finished
                                .send((job.coord, generated_revision, chunk))
                                .is_err()
                            {
                                break;
                            }
                        }
                    })
                    .expect("Start terrain worker"),
            );
        }
        // Workers exit when Stream drops: job/result channel disconnection unblocks them.
        Self {
            cache: HashMap::new(),
            desired: Vec::new(),
            loading: Vec::new(),
            last_priority: Instant::now(),
            center: (0, 0),
            pending: HashSet::new(),
            jobs,
            results,
            epoch,
            worker_count,
            uploaded: HashSet::new(),
            cache_order: VecDeque::new(),
            cache_bytes: 0,
            workers,
            revision,
        }
    }

    pub(crate) fn invalidate(&mut self, center: Coord, revision: u64) -> Vec<Coord> {
        self.revision = revision;
        self.epoch.fetch_add(1, Ordering::Relaxed);
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

    pub(crate) fn update(&mut self, center: Coord, visible: impl Fn(Coord) -> bool) {
        let moved = self.desired.is_empty() || self.center != center;
        if moved {
            self.center = center;
            self.desired = nearby(center);
            self.uploaded
                .retain(|&coord| in_range(coord, center, RENDER_DISTANCE));
        }
        if moved || self.last_priority.elapsed() >= PRIORITY_INTERVAL {
            let loading = load_order(&self.desired, center, visible);
            if loading != self.loading {
                // Queued stale priorities are canceled; already generated chunks stay cached.
                self.epoch.fetch_add(1, Ordering::Relaxed);
                self.loading = loading;
            }
            self.last_priority = Instant::now();
        }
        loop {
            match self.results.try_recv() {
                Ok((coord, revision, chunk)) => {
                    self.pending.remove(&coord);
                    if let Some(chunk) = chunk
                        && revision == self.revision
                    {
                        self.cache_chunk(coord, chunk);
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => panic!("Terrain worker stopped unexpectedly"),
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
                self.jobs
                    .send(Job {
                        coord,
                        epoch: self.epoch.load(Ordering::Relaxed),
                    })
                    .expect("Queue chunk generation");
            }
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
        // Disconnect both queues before joining, including workers blocked on sending.
        self.jobs = mpsc::channel().0;
        self.results = mpsc::channel().1;
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                eprintln!("Terrain worker failed during shutdown");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_for_center(stream: &mut Stream, coord: Coord) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !stream.cache.contains_key(&coord) {
            stream.update(coord, |_| true);
            assert!(Instant::now() < deadline, "Terrain worker timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn edited_chunks_replace_the_old_cached_mesh() {
        let world = crate::database::tests::TestWorld::new();
        let db = crate::database::Database::open(&world.path).unwrap();
        let mut stream = Stream::start(1, Some(world.path.clone()));
        wait_for_center(&mut stream, (0, 0));
        let original = stream.cache[&(0, 0)].clone();
        stream.mark_uploaded((0, 0));
        let revision = db
            .edit(world::Edit {
                position: [0, 120, 0],
                material: 3,
            })
            .unwrap();
        stream.invalidate((0, 0), revision);
        assert!(!stream.uploaded.contains(&(0, 0)));
        wait_for_center(&mut stream, (0, 0));
        assert_ne!(stream.cache[&(0, 0)].mesh, original.mesh);
        assert_eq!(db.terrain((0, 0)).unwrap().0.voxel([0, 120, 0]), 3);
    }

    #[test]
    fn cache_caps_evict_uploaded_meshes_without_requesting_them_again() {
        let mut stream = Stream::with_workers(1);
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
        stream.update((100, 0), |_| true);
        assert!(!stream.pending.contains(&(100, 0)));
    }

    #[test]
    fn evicted_chunks_reload_from_sqlite() {
        let world = crate::database::tests::TestWorld::new();
        let db = crate::database::Database::open(&world.path).unwrap();
        // Store a distinctive flat chunk to prove loading bypasses world generation.
        let terrain = ChunkData::from_heights([60; (world::CHUNK_SIZE + 2).pow(2)]);
        let mesh = terrain.mesh();
        db.save_chunk((0, 0), &terrain.encode()).unwrap();
        let mut stream = Stream::start(1, Some(world.path.clone()));
        wait_for_center(&mut stream, (0, 0));
        assert_eq!(stream.cache[&(0, 0)].mesh, mesh);
        stream.trim_cache(0, 0);
        assert!(stream.cache.is_empty());
        wait_for_center(&mut stream, (0, 0));
        assert_eq!(stream.cache[&(0, 0)].mesh, mesh);
    }

    #[test]
    fn residency_is_circular_and_translation_invariant() {
        assert_eq!(resident_capacity(24), 1793);
        assert_eq!(resident_capacity(48), 7213);
        let radius = RENDER_DISTANCE;
        for center in [(0, 0), (-40, 73)] {
            let coords = nearby(center);
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
        let coords = nearby((0, 0));
        let ordered = load_order(&coords, (0, 0), |(_, z)| z < 0);
        let index = |coord| ordered.iter().position(|&c| c == coord).unwrap();
        assert_eq!(ordered[0], (0, 0));
        assert!(index((0, 2)) < index((0, -3)));
        assert!(index((0, -12)) < index((0, 3)));
        assert_eq!(ordered.len(), coords.len());
    }

    #[test]
    fn turning_reprioritizes_without_unloading_or_reordering_draws() {
        let mut stream = Stream::with_workers(1);
        stream.update((0, 0), |(_, z)| z < 0);
        let draws = stream.desired.clone();
        let epoch = stream.epoch.load(Ordering::Relaxed);
        stream.last_priority = Instant::now() - PRIORITY_INTERVAL;
        stream.update((0, 0), |(_, z)| z > 0);
        let index = |coord| stream.loading.iter().position(|&c| c == coord).unwrap();
        assert!(index((0, 12)) < index((0, -3)));
        assert!(stream.epoch.load(Ordering::Relaxed) > epoch);
        assert_eq!(stream.desired, draws);
        assert!(stream.pending.len() <= stream.worker_count * JOBS_PER_WORKER);
    }

    #[test]
    fn rapid_moves_cancel_stale_work_without_losing_the_new_center() {
        let mut stream = Stream::with_workers(2);
        for step in 0..20 {
            stream.update((step * 100, -step * 100), |_| true);
            assert!(stream.pending.len() <= 2 * JOBS_PER_WORKER);
        }
        wait_for_center(&mut stream, (-1000, 1000));
        assert_eq!(stream.center, (-1000, 1000));
    }

    #[test]
    fn streaming_reuses_visited_chunks_and_bounds_pending_work() {
        let mut stream = Stream::with_workers(2);
        wait_for_center(&mut stream, (0, 0));
        let original = stream.cache[&(0, 0)].clone();
        let mut covered = 0;
        for (section, ranges) in original.sections.iter().enumerate() {
            for (translucent, range) in ranges.iter().enumerate() {
                for index in range.clone() {
                    let start = index as usize * world::QUAD_BYTES;
                    let a = u32::from_ne_bytes(original.mesh[start..start + 4].try_into().unwrap());
                    assert_eq!(((a >> 27) & 15) as usize, section);
                    assert_eq!(
                        usize::from(world::is_translucent((a >> 20) & 63)),
                        translucent
                    );
                    covered += 1;
                }
            }
        }
        assert_eq!(covered * world::QUAD_BYTES, original.mesh.len());
        wait_for_center(&mut stream, (-32, 12));
        assert!(stream.pending.len() <= stream.worker_count * JOBS_PER_WORKER);
        stream.update((0, 0), |_| true);
        assert!(Arc::ptr_eq(&original, &stream.cache[&(0, 0)]));
        assert!(stream.cache.contains_key(&(-32, 12)));
        assert_eq!(stream.desired.len(), RESIDENT_CAPACITY);
        assert_eq!(stream.desired[0], (0, 0));
    }
}
