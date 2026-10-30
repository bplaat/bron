/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The world database of the engine's schema, in WAL mode while the world is open. Edits
//! rewrite the stored chunks they fall in, generated chunks are stored when first loaded, so
//! a world never changes with the world generation.

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use libsqlite3_sys as sql;
use plaatcraft_engine::database::{self as schema, APPLICATION_ID, VERSION};
use plaatcraft_engine::protocol::{MAX_BLOB_SIZE, Player};
use plaatcraft_engine::world::{self, ChunkData, Coord, Edit};

type Result<T> = std::result::Result<T, String>;

// Connections stay on their owning thread; statements borrow the connection.
pub(crate) struct Database {
    raw: *mut sql::sqlite3,
    seed: u64,
}
struct Statement<'a> {
    raw: *mut sql::sqlite3_stmt,
    db: &'a Database,
}

impl Database {
    /// Opens an existing world.
    pub(crate) fn open(path: &Path) -> Result<Self> {
        Self::connect(path, None)
    }

    /// Creates a new world that is generated from the seed.
    pub(crate) fn create(path: &Path, seed: u64) -> Result<Self> {
        Self::connect(path, Some(seed))
    }

    fn connect(path: &Path, seed: Option<u64>) -> Result<Self> {
        let path =
            CString::new(path.to_str().ok_or("Invalid world path")?).map_err(|e| e.to_string())?;
        let mut raw = ptr::null_mut();
        let create = if seed.is_some() {
            sql::SQLITE_OPEN_CREATE
        } else {
            0
        };
        // SAFETY: The path is NUL-terminated and `raw` is a valid output pointer.
        let code = unsafe {
            sql::sqlite3_open_v2(
                path.as_ptr(),
                &mut raw,
                sql::SQLITE_OPEN_READWRITE | sql::SQLITE_OPEN_NOMUTEX | create,
                ptr::null(),
            )
        };
        if raw.is_null() {
            return Err("SQLite could not allocate a connection".into());
        }
        let mut db = Self { raw, seed: 0 };
        db.check(code)?;
        // SAFETY: `db` owns a live SQLite connection.
        db.check(unsafe { sql::sqlite3_busy_timeout(db.raw, 5000) })?;
        // Apple's SQLite keeps the WAL files after closing, a closed world is one file.
        // In-memory databases have no files and don't know the operation.
        let mut persist_wal: i32 = 0;
        // SAFETY: The connection is live and SQLite only reads the integer during the call.
        unsafe {
            sql::sqlite3_file_control(
                db.raw,
                c"main".as_ptr(),
                sql::SQLITE_FCNTL_PERSIST_WAL,
                (&raw mut persist_wal).cast(),
            );
        }
        let identity = {
            let mut stmt = db.prepare(schema::IDENTITY)?;
            stmt.row()?;
            [stmt.integer(0), stmt.integer(1)]
        };
        if identity != [0, 0] && identity != [APPLICATION_ID, VERSION] {
            return Err("Unsupported world database".into());
        }
        // Edits commit to the WAL without waiting for the disk, which stays consistent and
        // only loses the last edits on a power loss, so slow disks don't stall the game.
        db.execute(c"PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-4096; PRAGMA wal_autocheckpoint=1000; PRAGMA journal_size_limit=16777216;")?;
        db.transaction(|| {
            db.execute(schema::SCHEMA)?;
            if let Some(seed) = seed {
                let mut stmt = db.prepare(schema::SET_SEED)?;
                stmt.bind_int(1, seed as i64)?;
                stmt.done()?;
            }
            Ok(())
        })?;
        let mut stmt = db.prepare(schema::SEED)?;
        if !stmt.row()? {
            return Err("World seed is missing".into());
        }
        let seed = stmt.integer(0) as u64;
        drop(stmt);
        db.seed = seed;
        Ok(db)
    }

    /// Closes the last connection of the world and leaves a rollback journal database,
    /// because the Wii's SQLite has no WAL.
    pub(crate) fn close(self) -> Result<()> {
        self.execute(c"PRAGMA journal_mode=DELETE")
    }

    fn check(&self, code: i32) -> Result<()> {
        if code == sql::SQLITE_OK {
            Ok(())
        } else {
            // SAFETY: SQLite keeps its connection error string valid until the next API call.
            Err(unsafe { CStr::from_ptr(sql::sqlite3_errmsg(self.raw)) }
                .to_string_lossy()
                .into_owned())
        }
    }

    fn execute(&self, text: &CStr) -> Result<()> {
        // SAFETY: The connection and SQL string remain valid for the duration of the call.
        self.check(unsafe {
            sql::sqlite3_exec(
                self.raw,
                text.as_ptr(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        })
    }

    // Runs the writes in one transaction, which is rolled back when they fail.
    fn transaction<T>(&self, writes: impl FnOnce() -> Result<T>) -> Result<T> {
        self.execute(c"BEGIN IMMEDIATE")?;
        let result = writes().and_then(|value| self.execute(c"COMMIT").map(|()| value));
        if result.is_err() {
            let _ = self.execute(c"ROLLBACK");
        }
        result
    }

    fn prepare(&self, text: &CStr) -> Result<Statement<'_>> {
        let mut raw = ptr::null_mut();
        // SAFETY: All input pointers are valid and `raw` receives the owned statement.
        self.check(unsafe {
            sql::sqlite3_prepare_v2(self.raw, text.as_ptr(), -1, &mut raw, ptr::null_mut())
        })?;
        Ok(Statement { raw, db: self })
    }

    /// The stored blocks of a chunk, `None` while it was never loaded.
    pub(crate) fn chunk(&self, (x, z): Coord) -> Result<Option<ChunkData>> {
        let mut stmt = self.prepare(schema::CHUNK)?;
        stmt.bind_int(1, x)?;
        stmt.bind_int(2, z)?;
        if stmt.row()? {
            ChunkData::decode_blocks(&stmt.blob(0)?).map(Some)
        } else {
            Ok(None)
        }
    }

    fn save_chunk(&self, (x, z): Coord, chunk: &ChunkData, statement: &CStr) -> Result<()> {
        let mut stmt = self.prepare(statement)?;
        stmt.bind_int(1, x)?;
        stmt.bind_int(2, z)?;
        stmt.bind_blob(3, &chunk.encode_blocks())?;
        stmt.done()
    }

    /// The lit chunk from the stored chunks around it, a chunk that was never loaded is
    /// generated and stored, its neighbors only light it.
    pub(crate) fn terrain(&self, coord: Coord) -> Result<ChunkData> {
        // One read transaction, so the chunks come from one consistent WAL snapshot.
        self.execute(c"BEGIN")?;
        let stored: Result<Vec<_>> = (0..9)
            .map(|index| self.chunk(world::neighbor(coord, index)))
            .collect();
        self.execute(c"COMMIT")?;
        let mut stored = stored?;
        if stored[4].is_none() {
            let chunk = ChunkData::generate_blocks(self.seed, coord);
            // An edit may have stored the chunk meanwhile, the client loads it again then.
            self.save_chunk(coord, &chunk, schema::ADD_CHUNK)?;
            stored[4] = Some(chunk);
        }
        let chunks: Vec<_> = stored
            .into_iter()
            .enumerate()
            .map(|(index, chunk)| {
                chunk.unwrap_or_else(|| {
                    ChunkData::generate_blocks(self.seed, world::neighbor(coord, index))
                })
            })
            .collect();
        Ok(ChunkData::lit(std::array::from_fn(|index| &chunks[index])))
    }

    /// Stores the edits in the chunks they fall in, at once.
    pub(crate) fn edit(&self, edits: &[Edit]) -> Result<()> {
        if edits.iter().any(|edit| {
            (edit.material != 0 && !world::valid_material(edit.material))
                || !(3..world::HEIGHT as i64).contains(&edit.position[1])
        }) {
            return Err("Cannot edit this block".into());
        }
        let mut coords: Vec<_> = edits
            .iter()
            .map(|edit| world::chunk_of(edit.position))
            .collect();
        coords.sort_unstable();
        coords.dedup();
        self.transaction(|| {
            for coord in coords {
                let mut chunk = match self.chunk(coord)? {
                    Some(chunk) => chunk,
                    None => ChunkData::generate_blocks(self.seed, coord),
                };
                chunk.apply_edits(coord, edits);
                self.save_chunk(coord, &chunk, schema::SAVE_CHUNK)?;
            }
            Ok(())
        })
    }

    /// The blocks whose fluid still has to flow, saved when the world closed.
    pub(crate) fn fluid_updates(&self) -> Result<Vec<[i64; 3]>> {
        let mut stmt = self.prepare(schema::FLUIDS)?;
        let mut positions = Vec::new();
        while stmt.row()? {
            positions.push([stmt.integer(0), stmt.integer(1), stmt.integer(2)]);
        }
        Ok(positions)
    }

    pub(crate) fn save_fluid_updates(&self, positions: &[[i64; 3]]) -> Result<()> {
        self.transaction(|| {
            self.execute(schema::CLEAR_FLUIDS)?;
            let mut stmt = self.prepare(schema::ADD_FLUID)?;
            for position in positions {
                for (i, &value) in position.iter().enumerate() {
                    stmt.bind_int(i as i32 + 1, value)?;
                }
                stmt.done()?;
                stmt.reset();
            }
            Ok(())
        })
    }

    pub(crate) fn player(&self) -> Result<Player> {
        let mut stmt = self.prepare(schema::PLAYER)?;
        if stmt.row()? {
            Player::decode(&stmt.blob(0)?)
        } else {
            Ok(Player::default())
        }
    }

    pub(crate) fn save_player(&self, player: &Player) -> Result<()> {
        let mut stmt = self.prepare(schema::SAVE_PLAYER)?;
        stmt.bind_blob(1, &player.encode())?;
        stmt.done()
    }

    /// A player that joined the world over the network, by name.
    pub(crate) fn guest(&self, name: &str) -> Result<Option<Player>> {
        let mut stmt = self.prepare(schema::GUEST)?;
        stmt.bind_blob(1, name.as_bytes())?;
        if stmt.row()? {
            Player::decode(&stmt.blob(0)?).map(Some)
        } else {
            Ok(None)
        }
    }

    pub(crate) fn save_guest(&self, name: &str, player: &Player) -> Result<()> {
        let mut stmt = self.prepare(schema::SAVE_GUEST)?;
        stmt.bind_blob(1, name.as_bytes())?;
        stmt.bind_blob(2, &player.encode())?;
        stmt.done()
    }
}

impl Statement<'_> {
    fn bind_int(&mut self, index: i32, value: i64) -> Result<()> {
        // SAFETY: The statement is live and exclusively borrowed.
        self.db
            .check(unsafe { sql::sqlite3_bind_int64(self.raw, index, value) })
    }
    fn bind_blob(&mut self, index: i32, value: &[u8]) -> Result<()> {
        let length = i32::try_from(value.len()).map_err(|e| e.to_string())?;
        // SAFETY: SQLite copies `value` because SQLITE_TRANSIENT is supplied.
        self.db.check(unsafe {
            sql::sqlite3_bind_blob(
                self.raw,
                index,
                value.as_ptr().cast(),
                length,
                sql::SQLITE_TRANSIENT(),
            )
        })
    }
    fn row(&mut self) -> Result<bool> {
        // SAFETY: The statement is live and exclusively borrowed while it advances.
        match unsafe { sql::sqlite3_step(self.raw) } {
            sql::SQLITE_ROW => Ok(true),
            sql::SQLITE_DONE => Ok(false),
            code => {
                self.db.check(code)?;
                unreachable!()
            }
        }
    }
    // Makes the statement ready to run again with new bindings.
    fn reset(&mut self) {
        // SAFETY: The statement is live and exclusively borrowed.
        unsafe {
            sql::sqlite3_reset(self.raw);
        }
    }

    fn done(&mut self) -> Result<()> {
        if self.row()? {
            Err("Unexpected SQLite row".into())
        } else {
            Ok(())
        }
    }
    fn integer(&self, column: i32) -> i64 {
        // SAFETY: Callers read columns only while the statement points at a row.
        unsafe { sql::sqlite3_column_int64(self.raw, column) }
    }
    fn blob(&self, column: i32) -> Result<Vec<u8>> {
        // SAFETY: Callers read columns only while the statement points at a row.
        let size = unsafe { sql::sqlite3_column_bytes(self.raw, column) };
        if !(1..=MAX_BLOB_SIZE as i32).contains(&size) {
            return Err("Invalid saved blob size".into());
        }
        // SAFETY: The statement remains on the same row while the blob is copied.
        let data = unsafe { sql::sqlite3_column_blob(self.raw, column) };
        if data.is_null() {
            return Err("Missing saved blob".into());
        }
        // SAFETY: SQLite reported this non-null blob pointer and its validated size.
        Ok(unsafe { std::slice::from_raw_parts(data.cast(), size as usize) }.to_vec())
    }
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        // SAFETY: This statement is owned and finalized exactly once.
        unsafe {
            sql::sqlite3_finalize(self.raw);
        }
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        // SAFETY: This connection is owned and closed exactly once.
        unsafe {
            sql::sqlite3_close(self.raw);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use plaatcraft_engine::camera::Camera;

    use super::*;
    use crate::inventory::Hotbar;

    pub(crate) struct TestWorld {
        directory: PathBuf,
        pub(crate) path: PathBuf,
    }
    impl TestWorld {
        pub(crate) fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir()
                .join(format!("plaatcraft-world-test-{}-{id}", std::process::id()));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("World.db");
            Database::create(&path, world::WORLD_SEED).unwrap();
            Self { directory, path }
        }
    }
    impl Drop for TestWorld {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    pub(crate) fn memory() -> Database {
        Database::create(Path::new(":memory:"), world::WORLD_SEED).unwrap()
    }

    impl Database {
        /// Stores the blocks of a chunk, like a flat one to tell it from generated terrain.
        pub(crate) fn store(&self, coord: Coord, chunk: &ChunkData) -> Result<()> {
            self.save_chunk(coord, chunk, schema::SAVE_CHUNK)
        }
    }

    #[test]
    fn chunk_blobs_use_a_rowid_table_to_avoid_overflow_page_waste() {
        let db = memory();
        let mut table = db
            .prepare(c"SELECT wr FROM pragma_table_list WHERE name='chunks'")
            .unwrap();
        assert!(table.row().unwrap());
        assert_eq!(table.integer(0), 0);
    }

    #[test]
    fn worlds_are_only_created_once_and_never_opened_when_missing() {
        let world = TestWorld::new();
        assert!(Database::create(&world.path, 7).is_err());
        assert_eq!(Database::open(&world.path).unwrap().seed, world::WORLD_SEED);
        let missing = world.path.with_file_name("Missing.db");
        assert!(Database::open(&missing).is_err());
        assert!(!missing.exists());
        let created = world.path.with_file_name("Other.db");
        assert_eq!(Database::create(&created, 7).unwrap().seed, 7);
        assert_eq!(Database::open(&created).unwrap().seed, 7);
    }

    #[test]
    fn movement_mode_roundtrips() {
        let db = memory();
        assert!(!db.player().unwrap().flying);
        assert!(!Player::default().flying);
        db.save_player(&Player::default()).unwrap();
        assert!(!db.player().unwrap().flying);
        let flying = Player {
            flying: true,
            ..Player::default()
        };
        db.save_player(&flying).unwrap();
        assert!(db.player().unwrap().flying);
    }

    #[test]
    fn loaded_and_edited_chunks_are_stored_and_survive_restart() {
        let world = TestWorld::new();
        let edit = Edit {
            position: [-1, 120, 0],
            material: 24,
        };
        {
            let db = Database::open(&world.path).unwrap();
            assert!(db.chunk((0, 0)).unwrap().is_none());
            let generated = db.terrain((0, 0)).unwrap();
            assert_eq!(
                generated.mesh(),
                ChunkData::generate(world::WORLD_SEED, (0, 0)).mesh()
            );
            // Only the loaded chunk is stored, its neighbors only light it.
            assert!(db.chunk((0, 0)).unwrap().is_some());
            assert!(db.chunk((-1, 0)).unwrap().is_none());
            db.edit(&[edit]).unwrap();
            assert_eq!(db.chunk((-1, 0)).unwrap().unwrap().voxel([15, 120, 0]), 24);
            // The neighbor's halo shows the edit.
            assert_eq!(db.terrain((0, 0)).unwrap().voxel([0, 120, 0]), 0);
            let mut hotbar = Hotbar::default();
            hotbar.pick(25);
            db.save_player(&Player {
                hotbar,
                ..Player::default()
            })
            .unwrap();
            db.close().unwrap();
        }
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.player().unwrap().hotbar.material(), 25);
        assert_eq!(db.terrain((-1, 0)).unwrap().voxel([15, 120, 0]), 24);
        db.edit(&[Edit {
            material: 0,
            ..edit
        }])
        .unwrap();
        assert_eq!(db.terrain((-1, 0)).unwrap().voxel([15, 120, 0]), 0);
        assert!(
            db.edit(&[Edit {
                position: [0, 0, 0],
                material: 0
            }])
            .is_err()
        );
    }

    #[test]
    fn stored_chunks_dont_change_with_the_world_generation() {
        let world = TestWorld::new();
        {
            let db = Database::open(&world.path).unwrap();
            db.store((0, 0), &ChunkData::from_heights([60; 18 * 18]))
                .unwrap();
            db.terrain((5, 5)).unwrap();
            db.execute(c"UPDATE metadata SET seed=12345 WHERE id=1")
                .unwrap();
        }
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.terrain((0, 0)).unwrap().voxel([8, 60, 8]), 0);
        assert_eq!(db.terrain((0, 0)).unwrap().voxel([8, 59, 8]), 1);
        assert_eq!(
            db.chunk((5, 5)).unwrap().unwrap().encode_blocks(),
            ChunkData::generate_blocks(world::WORLD_SEED, (5, 5)).encode_blocks()
        );
    }

    #[test]
    fn closed_worlds_are_rollback_journal_files_for_the_wii() {
        let world = TestWorld::new();
        let db = Database::open(&world.path).unwrap();
        db.terrain((0, 0)).unwrap();
        db.close().unwrap();
        let header = std::fs::read(&world.path).unwrap();
        // The file format versions are 2 in WAL mode.
        assert_eq!(header[18..20], [1, 1]);
        assert_eq!(header[60..64], (VERSION as u32).to_be_bytes());
        assert_eq!(header[68..72], (APPLICATION_ID as u32).to_be_bytes());
        assert!(!world.path.with_extension("db-wal").exists());
        let db = Database::open(&world.path).unwrap();
        assert!(db.chunk((0, 0)).unwrap().is_some());
    }

    #[test]
    fn wal_reopens_chunks_seed_and_player() {
        let world = TestWorld::new();
        let player = Player {
            position: [-192.25, 93.5, 400.125],
            yaw: 2.5,
            pitch: -1.55,
            night: true,
            ..Player::default()
        };
        let blocks = {
            let db = Database::open(&world.path).unwrap();
            let mut mode = db.prepare(c"PRAGMA journal_mode").unwrap();
            assert!(mode.row().unwrap());
            assert_eq!(
                // SAFETY: `mode` is positioned on a row containing a text column.
                unsafe { CStr::from_ptr(sql::sqlite3_column_text(mode.raw, 0).cast()) },
                c"wal"
            );
            drop(mode);
            db.execute(c"UPDATE metadata SET seed=12345 WHERE id=1")
                .unwrap();
            let blocks = db.terrain((-2, 1)).unwrap().encode_blocks();
            db.save_player(&player).unwrap();
            let reader = Database::open(&world.path).unwrap();
            assert_eq!(
                reader.chunk((-2, 1)).unwrap().unwrap().encode_blocks(),
                blocks
            );
            assert!(reader.chunk((999, 999)).unwrap().is_none());
            blocks
        };
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.seed, 12345);
        assert_eq!(db.terrain((-2, 1)).unwrap().encode_blocks(), blocks);
        let mut camera = Camera::default();
        db.player().unwrap().restore(&mut camera);
        assert_eq!(Player::new(&camera, true, Hotbar::default()), player);
    }

    #[test]
    fn guests_are_saved_by_name_and_players_roundtrip_as_bytes() {
        let db = memory();
        assert_eq!(db.guest("Bob").unwrap(), None);
        let mut hotbar = Hotbar::default();
        hotbar.pick(25);
        let bob = Player {
            position: [1.5, 80.0, -3.0],
            yaw: 1.0,
            flying: true,
            hotbar,
            ..Player::default()
        };
        assert_eq!(Player::decode(&bob.encode()).unwrap(), bob);
        assert!(Player::decode(&bob.encode()[1..]).is_err());
        db.save_guest("Bob", &bob).unwrap();
        db.save_guest("Alice", &Player::default()).unwrap();
        assert_eq!(db.guest("Bob").unwrap(), Some(bob));
        assert_eq!(db.guest("Alice").unwrap(), Some(Player::default()));
        assert_ne!(db.player().unwrap(), bob);
    }

    #[test]
    fn batches_of_edits_apply_at_once_and_fluid_updates_persist() {
        let db = memory();
        let edits: Vec<_> = (0..5)
            .map(|x| Edit {
                position: [x * 8 - 8, 100, 0],
                material: world::Fluid::Water.flowing(7),
            })
            .collect();
        db.edit(&edits).unwrap();
        assert_eq!(
            db.terrain((1, 0)).unwrap().voxel([8, 100, 0]),
            world::Fluid::Water.flowing(7)
        );
        // One invalid edit rejects the whole batch.
        let invalid = [
            Edit {
                material: 0,
                ..edits[0]
            },
            Edit {
                position: [0, 1, 0],
                material: 0,
            },
        ];
        assert!(db.edit(&invalid).is_err());
        assert_eq!(
            db.chunk((-1, 0)).unwrap().unwrap().voxel([8, 100, 0]),
            world::Fluid::Water.flowing(7)
        );

        assert!(db.fluid_updates().unwrap().is_empty());
        db.save_fluid_updates(&[[1, 2, 3], [-4, 5, -6]]).unwrap();
        db.save_fluid_updates(&[[7, 8, 9]]).unwrap();
        assert_eq!(db.fluid_updates().unwrap(), [[7, 8, 9]]);
    }

    #[test]
    fn concurrent_workers_store_independent_chunks() {
        let world = TestWorld::new();
        let db = Database::open(&world.path).unwrap();
        std::thread::scope(|scope| {
            for x in 0..4 {
                let path = &world.path;
                scope.spawn(move || {
                    let db = Database::open(path).unwrap();
                    for z in 0..5 {
                        db.terrain((x, z)).unwrap();
                    }
                });
            }
        });
        for x in 0..4 {
            for z in 0..5 {
                assert!(db.chunk((x, z)).unwrap().is_some());
            }
        }
    }

    #[test]
    fn future_database_versions_are_rejected_without_overwriting() {
        let world = TestWorld::new();
        let db = Database::open(&world.path).unwrap();
        db.execute(c"PRAGMA user_version=99").unwrap();
        assert!(Database::open(&world.path).is_err());
        let mut version = db.prepare(c"PRAGMA user_version").unwrap();
        assert!(version.row().unwrap());
        assert_eq!(version.integer(0), 99);
    }
}
