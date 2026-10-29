/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString};
use std::path::Path;
use std::ptr;

use libsqlite3_sys as sql;
use plaatcraft_engine::protocol::{MAX_BLOB_SIZE, Player};
use plaatcraft_engine::world::{self, ChunkData, Coord, Edit};

fn compress_chunk(data: &[u8]) -> Vec<u8> {
    deflate::zlib_compress(data, deflate::Level::Default)
}

fn decompress_chunk(data: Vec<u8>) -> Result<Vec<u8>> {
    deflate::zlib_decompress(&data, MAX_BLOB_SIZE)
        .map_err(|error| format!("Invalid compressed chunk: {error}"))
}

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
        let mut version = db.prepare(c"PRAGMA user_version")?;
        version.row()?;
        let version_number = version.integer(0);
        drop(version);
        if !matches!(version_number, 0 | 1) {
            return Err(format!(
                "Unsupported world database version {version_number}"
            ));
        }
        db.execute(c"PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-4096; PRAGMA wal_autocheckpoint=1000; PRAGMA journal_size_limit=16777216;")?;
        db.execute(c"BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS metadata (id INTEGER PRIMARY KEY CHECK(id=1), seed INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS chunks (x INTEGER NOT NULL, z INTEGER NOT NULL, data BLOB NOT NULL, PRIMARY KEY(x,z));
            CREATE TABLE IF NOT EXISTS game_state (id INTEGER PRIMARY KEY CHECK(id=1), data BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS interaction (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL);
            INSERT OR IGNORE INTO interaction VALUES(1,0);
            CREATE TABLE IF NOT EXISTS players (name BLOB PRIMARY KEY, data BLOB NOT NULL) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS fluid_updates (x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, PRIMARY KEY(x,y,z)) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS edits (x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, material INTEGER NOT NULL, PRIMARY KEY(x,z,y)) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS dirty_chunks (x INTEGER NOT NULL, z INTEGER NOT NULL, PRIMARY KEY(x,z)) WITHOUT ROWID;
            PRAGMA user_version=1; COMMIT;")?;
        if let Some(seed) = seed {
            let mut stmt = db.prepare(c"INSERT INTO metadata(id,seed) VALUES(1,?)")?;
            stmt.bind_int(1, seed as i64)?;
            stmt.done()?;
        }
        let mut stmt = db.prepare(c"SELECT seed FROM metadata WHERE id=1")?;
        if !stmt.row()? {
            return Err("World seed is missing".into());
        }
        let seed = stmt.integer(0) as u64;
        drop(stmt);
        db.seed = seed;
        Ok(db)
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

    fn prepare(&self, text: &CStr) -> Result<Statement<'_>> {
        let mut raw = ptr::null_mut();
        // SAFETY: All input pointers are valid and `raw` receives the owned statement.
        self.check(unsafe {
            sql::sqlite3_prepare_v2(self.raw, text.as_ptr(), -1, &mut raw, ptr::null_mut())
        })?;
        Ok(Statement { raw, db: self })
    }

    pub(crate) fn load_chunk(&self, (x, z): Coord) -> Result<Option<Vec<u8>>> {
        let mut stmt = self.prepare(c"SELECT data FROM chunks WHERE x=? AND z=? AND NOT EXISTS (SELECT 1 FROM dirty_chunks WHERE dirty_chunks.x=chunks.x AND dirty_chunks.z=chunks.z)")?;
        stmt.bind_int(1, x)?;
        stmt.bind_int(2, z)?;
        if stmt.row()? {
            Ok(Some(decompress_chunk(stmt.blob(0)?)?))
        } else {
            Ok(None)
        }
    }

    fn load_base(&self, (x, z): Coord) -> Result<Option<Vec<u8>>> {
        let mut stmt = self.prepare(c"SELECT data FROM chunks WHERE x=? AND z=?")?;
        stmt.bind_int(1, x)?;
        stmt.bind_int(2, z)?;
        if stmt.row()? {
            Ok(Some(decompress_chunk(stmt.blob(0)?)?))
        } else {
            Ok(None)
        }
    }

    #[cfg(test)]
    pub(crate) fn save_chunk(&self, coord: Coord, data: &[u8]) -> Result<()> {
        self.store_chunk(coord, &compress_chunk(data))
    }

    fn store_chunk(&self, (x, z): Coord, data: &[u8]) -> Result<()> {
        let mut stmt = self.prepare(c"INSERT INTO chunks(x,z,data) VALUES(?,?,?) ON CONFLICT(x,z) DO UPDATE SET data=excluded.data")?;
        stmt.bind_int(1, x)?;
        stmt.bind_int(2, z)?;
        stmt.bind_blob(3, data)?;
        stmt.done()
    }

    pub(crate) fn revision(&self) -> Result<u64> {
        let mut stmt = self.prepare(c"SELECT revision FROM interaction WHERE id=1")?;
        stmt.row()?;
        Ok(stmt.integer(0) as u64)
    }

    fn load_edits(&self, coord: Coord) -> Result<Vec<Edit>> {
        let mut stmt = self.prepare(
            c"SELECT x,y,z,material FROM edits WHERE x BETWEEN ? AND ? AND z BETWEEN ? AND ?",
        )?;
        for (i, value) in [
            coord.0 * 16 - 16,
            coord.0 * 16 + 31,
            coord.1 * 16 - 16,
            coord.1 * 16 + 31,
        ]
        .into_iter()
        .enumerate()
        {
            stmt.bind_int(i as i32 + 1, value)?;
        }
        let mut edits = Vec::new();
        while stmt.row()? {
            let material = u8::try_from(stmt.integer(3)).map_err(|e| e.to_string())?;
            let position = [stmt.integer(0), stmt.integer(1), stmt.integer(2)];
            if material != 0 && !world::valid_material(material)
                || !(3..world::HEIGHT as i64).contains(&position[1])
            {
                return Err("Invalid saved block edit".into());
            }
            edits.push(Edit { position, material });
        }
        Ok(edits)
    }

    /// Saves edits at one new revision.
    pub(crate) fn edits(&self, edits: &[Edit]) -> Result<u64> {
        if edits.iter().any(|edit| {
            (edit.material != 0 && !world::valid_material(edit.material))
                || !(3..world::HEIGHT as i64).contains(&edit.position[1])
        }) {
            return Err("Cannot edit this block".into());
        }
        self.execute(c"BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut stmt = self.prepare(c"INSERT INTO edits(x,y,z,material) VALUES(?,?,?,?) ON CONFLICT(x,z,y) DO UPDATE SET material=excluded.material")?;
            let mut invalidate =
                self.prepare(c"INSERT OR IGNORE INTO dirty_chunks(x,z) SELECT x,z FROM chunks WHERE x BETWEEN ? AND ? AND z BETWEEN ? AND ?")?;
            for edit in edits {
                for (i, value) in edit
                    .position
                    .into_iter()
                    .chain([i64::from(edit.material)])
                    .enumerate()
                {
                    stmt.bind_int(i as i32 + 1, value)?;
                }
                stmt.done()?;
                stmt.reset();
                let x = edit.position[0].div_euclid(16);
                let z = edit.position[2].div_euclid(16);
                for (i, value) in [x - 1, x + 1, z - 1, z + 1].into_iter().enumerate() {
                    invalidate.bind_int(i as i32 + 1, value)?;
                }
                invalidate.done()?;
                invalidate.reset();
            }
            drop((stmt, invalidate));
            self.execute(c"UPDATE interaction SET revision=revision+1 WHERE id=1; COMMIT")?;
            self.revision()
        })();
        if result.is_err() {
            let _ = self.execute(c"ROLLBACK");
        }
        result
    }

    /// The blocks whose fluid still has to flow, saved when the world closed.
    pub(crate) fn fluid_updates(&self) -> Result<Vec<[i64; 3]>> {
        let mut stmt = self.prepare(c"SELECT x,y,z FROM fluid_updates")?;
        let mut positions = Vec::new();
        while stmt.row()? {
            positions.push([stmt.integer(0), stmt.integer(1), stmt.integer(2)]);
        }
        Ok(positions)
    }

    pub(crate) fn save_fluid_updates(&self, positions: &[[i64; 3]]) -> Result<()> {
        self.execute(c"BEGIN IMMEDIATE")?;
        let result = (|| {
            self.execute(c"DELETE FROM fluid_updates")?;
            let mut stmt =
                self.prepare(c"INSERT OR IGNORE INTO fluid_updates(x,y,z) VALUES(?,?,?)")?;
            for position in positions {
                for (i, &value) in position.iter().enumerate() {
                    stmt.bind_int(i as i32 + 1, value)?;
                }
                stmt.done()?;
                stmt.reset();
            }
            drop(stmt);
            self.execute(c"COMMIT")
        })();
        if result.is_err() {
            let _ = self.execute(c"ROLLBACK");
        }
        result
    }

    pub(crate) fn blocks(&self, coord: Coord) -> Result<Option<ChunkData>> {
        // Collision/picking needs current voxels even while lighting is being rebuilt.
        self.execute(c"BEGIN")?;
        let result = (|| {
            let base = self.load_base(coord)?;
            let edits = if base.is_some() {
                self.load_edits(coord)?
            } else {
                Vec::new()
            };
            self.execute(c"COMMIT")?;
            base.map(|data| {
                let mut chunk = ChunkData::decode(&data)?;
                chunk.apply_edits(coord, &edits);
                Ok(chunk)
            })
            .transpose()
        })();
        if result.is_err() {
            let _ = self.execute(c"ROLLBACK");
        }
        result
    }

    pub(crate) fn terrain(&self, coord: Coord) -> Result<(ChunkData, u64)> {
        // Read the cache, edits and revision from one consistent WAL snapshot.
        self.execute(c"BEGIN")?;
        let snapshot = (|| {
            let revision = self.revision()?;
            let saved = self.load_chunk(coord)?;
            let edits = if saved.is_none() {
                self.load_edits(coord)?
            } else {
                Vec::new()
            };
            let mut bases = Vec::new();
            if saved.is_none() {
                for z in coord.1 - 1..=coord.1 + 1 {
                    for x in coord.0 - 1..=coord.0 + 1 {
                        if let Some(data) = self.load_base((x, z))? {
                            bases.push(((x, z), data));
                        }
                    }
                }
            }
            self.execute(c"COMMIT")?;
            Ok((revision, saved, edits, bases))
        })();
        let (revision, saved, edits, bases) = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let _ = self.execute(c"ROLLBACK");
                return Err(error);
            }
        };
        let terrain = if let Some(data) = saved {
            ChunkData::decode(&data)?
        } else {
            let bases = bases
                .into_iter()
                .map(|(coord, data)| Ok((coord, ChunkData::decode(&data)?)))
                .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
            let terrain = ChunkData::rebuild(self.seed, coord, &bases, &edits);
            self.save_generated(coord, &terrain.encode(), revision)?;
            terrain
        };
        Ok((terrain, revision))
    }

    fn save_generated(&self, (x, z): Coord, data: &[u8], revision: u64) -> Result<()> {
        let data = compress_chunk(data);
        // Preserve saved block data while replacing derived lighting. A stale worker
        // cannot clear a newer dirty marker or overwrite a newer edit.
        self.execute(c"BEGIN IMMEDIATE")?;
        let result = (|| {
            if self.revision()? == revision {
                self.store_chunk((x, z), &data)?;
                let mut stmt = self.prepare(c"DELETE FROM dirty_chunks WHERE x=? AND z=?")?;
                stmt.bind_int(1, x)?;
                stmt.bind_int(2, z)?;
                stmt.done()?;
            }
            self.execute(c"COMMIT")
        })();
        if result.is_err() {
            let _ = self.execute(c"ROLLBACK");
        }
        result
    }

    pub(crate) fn player(&self) -> Result<Player> {
        let mut stmt = self.prepare(c"SELECT data FROM game_state WHERE id=1")?;
        if stmt.row()? {
            Player::decode(&stmt.blob(0)?)
        } else {
            Ok(Player::default())
        }
    }

    pub(crate) fn save_player(&self, player: &Player) -> Result<()> {
        let mut stmt = self.prepare(c"INSERT INTO game_state(id,data) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data")?;
        stmt.bind_blob(1, &player.encode())?;
        stmt.done()
    }

    /// A player that joined the world over the network, by name.
    pub(crate) fn guest(&self, name: &str) -> Result<Option<Player>> {
        let mut stmt = self.prepare(c"SELECT data FROM players WHERE name=?")?;
        stmt.bind_blob(1, name.as_bytes())?;
        if stmt.row()? {
            Player::decode(&stmt.blob(0)?).map(Some)
        } else {
            Ok(None)
        }
    }

    pub(crate) fn save_guest(&self, name: &str, player: &Player) -> Result<()> {
        let mut stmt = self.prepare(c"INSERT INTO players(name,data) VALUES(?,?) ON CONFLICT(name) DO UPDATE SET data=excluded.data")?;
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
            sql::sqlite3_close_v2(self.raw);
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
    fn chunk_compression_is_bounded_and_detects_corruption() {
        let data = vec![7; 80_000];
        let compressed = compress_chunk(&data);
        assert!(compressed.len() < data.len() / 10);
        assert_eq!(decompress_chunk(compressed.clone()).unwrap(), data);
        let mut corrupt = compressed.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert!(decompress_chunk(corrupt).is_err());
        assert!(decompress_chunk(compressed[..compressed.len() - 1].to_vec()).is_err());
        assert!(decompress_chunk(compress_chunk(&vec![0; MAX_BLOB_SIZE + 1])).is_err());
        assert!(decompress_chunk(vec![1, 2, 3]).is_err());
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
    fn edits_survive_restart_and_old_workers_cannot_restore_invalidated_chunks() {
        let world = TestWorld::new();
        let edit = Edit {
            position: [-1, 120, 0],
            material: 24,
        };
        {
            let db = Database::open(&world.path).unwrap();
            let flat = ChunkData::from_heights([60; 18 * 18]);
            db.save_chunk((-1, 0), &flat.encode()).unwrap();
            let (old, old_revision) = db.terrain((-1, 0)).unwrap();
            db.terrain((0, 0)).unwrap();
            let revision = db.edits(&[edit]).unwrap();
            assert!(revision > old_revision);
            assert!(db.load_chunk((-1, 0)).unwrap().is_none());
            assert!(db.load_chunk((0, 0)).unwrap().is_none());
            db.save_generated((-1, 0), &old.encode(), old_revision)
                .unwrap();
            assert!(db.load_chunk((-1, 0)).unwrap().is_none());
            assert_eq!(db.terrain((-1, 0)).unwrap().0.voxel([15, 120, 0]), 24);
            assert_eq!(db.terrain((-1, 0)).unwrap().0.voxel([8, 59, 8]), 1);
            assert_eq!(db.terrain((-1, 0)).unwrap().0.voxel([8, 60, 8]), 0);
            let mut hotbar = Hotbar::default();
            hotbar.pick(25);
            db.save_player(&Player {
                hotbar,
                ..Player::default()
            })
            .unwrap();
        }
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.player().unwrap().hotbar.material(), 25);
        assert_eq!(db.terrain((-1, 0)).unwrap().0.voxel([15, 120, 0]), 24);
        db.edits(&[Edit {
            material: 0,
            ..edit
        }])
        .unwrap();
        assert_eq!(db.terrain((-1, 0)).unwrap().0.voxel([15, 120, 0]), 0);
        assert!(
            db.edits(&[Edit {
                position: [0, 0, 0],
                material: 0
            }])
            .is_err()
        );
    }

    #[test]
    fn wal_reopens_chunks_seed_and_player() {
        let world = TestWorld::new();
        let data = ChunkData::generate(world::WORLD_SEED, (-2, 1)).encode();
        let player = Player {
            position: [-192.25, 93.5, 400.125],
            yaw: 2.5,
            pitch: -1.55,
            night: true,
            ..Player::default()
        };
        {
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
            db.save_chunk((-2, 1), &data).unwrap();
            db.save_player(&player).unwrap();
            let reader = Database::open(&world.path).unwrap();
            assert_eq!(reader.load_chunk((-2, 1)).unwrap().unwrap(), data);
            assert!(reader.load_chunk((999, 999)).unwrap().is_none());
        }
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.seed, 12345);
        assert_eq!(db.load_chunk((-2, 1)).unwrap().unwrap(), data);
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
    fn batches_of_edits_share_a_revision_and_fluid_updates_persist() {
        let db = memory();
        let revision = db.revision().unwrap();
        let edits: Vec<_> = (0..5)
            .map(|x| Edit {
                position: [x, 100, 0],
                material: world::Fluid::Water.flowing(7),
            })
            .collect();
        assert_eq!(db.edits(&edits).unwrap(), revision + 1);
        assert_eq!(
            db.terrain((0, 0)).unwrap().0.voxel([4, 100, 0]),
            world::Fluid::Water.flowing(7)
        );
        // One invalid edit rejects the whole batch.
        let invalid = [
            edits[0],
            Edit {
                position: [0, 1, 0],
                material: 0,
            },
        ];
        assert!(db.edits(&invalid).is_err());
        assert_eq!(db.revision().unwrap(), revision + 1);

        assert!(db.fluid_updates().unwrap().is_empty());
        db.save_fluid_updates(&[[1, 2, 3], [-4, 5, -6]]).unwrap();
        db.save_fluid_updates(&[[7, 8, 9]]).unwrap();
        assert_eq!(db.fluid_updates().unwrap(), [[7, 8, 9]]);
    }

    #[test]
    fn concurrent_workers_commit_independent_chunks() {
        let world = TestWorld::new();
        let db = Database::open(&world.path).unwrap();
        std::thread::scope(|scope| {
            for x in 0..4 {
                let path = &world.path;
                scope.spawn(move || {
                    let db = Database::open(path).unwrap();
                    for z in 0..20 {
                        db.save_chunk((x, z), &[x as u8, z as u8]).unwrap();
                    }
                });
            }
        });
        for x in 0..4 {
            for z in 0..20 {
                assert_eq!(db.load_chunk((x, z)).unwrap().unwrap(), [x as u8, z as u8]);
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
