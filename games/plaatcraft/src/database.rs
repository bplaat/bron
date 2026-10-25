/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::OnceLock;

use libsqlite3_sys as sql;

use crate::camera::Camera;
use crate::inventory::{self, Hotbar};
use crate::world::{self, ChunkData, Coord, Edit};

// App sandboxes have no writable working directory, so the world lives in the data directory.
pub(crate) fn path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let dir = directories::ProjectDirs::from("nl", "bplaat", "PlaatCraft")
            .expect("Find data directory")
            .data_dir();
        std::fs::create_dir_all(&dir).expect("Create data directory");
        dir.join("world.db").to_string_lossy().into_owned()
    })
}
const MAX_BLOB_SIZE: usize = 450_000;

fn compress_chunk(data: &[u8]) -> Vec<u8> {
    deflate::zlib_compress(data, deflate::Level::Default)
}

fn decompress_chunk(data: Vec<u8>) -> Result<Vec<u8>> {
    deflate::zlib_decompress(&data, MAX_BLOB_SIZE)
        .map_err(|error| format!("Invalid compressed chunk: {error}"))
}

type Result<T> = std::result::Result<T, String>;

// Connections stay on their owning thread; statements borrow the connection.
pub(crate) struct Database(*mut sql::sqlite3);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WindowState {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) maximized: bool,
}

struct Statement<'a> {
    raw: *mut sql::sqlite3_stmt,
    db: &'a Database,
}

impl Database {
    pub(crate) fn open(path: &str) -> Result<Self> {
        let path = CString::new(path).map_err(|e| e.to_string())?;
        let mut raw = ptr::null_mut();
        // SAFETY: The path is NUL-terminated and `raw` is a valid output pointer.
        let code = unsafe {
            sql::sqlite3_open_v2(
                path.as_ptr(),
                &mut raw,
                sql::SQLITE_OPEN_READWRITE | sql::SQLITE_OPEN_CREATE | sql::SQLITE_OPEN_NOMUTEX,
                ptr::null(),
            )
        };
        if raw.is_null() {
            return Err("SQLite could not allocate a connection".into());
        }
        let db = Self(raw);
        db.check(code)?;
        // SAFETY: `db` owns a live SQLite connection.
        db.check(unsafe { sql::sqlite3_busy_timeout(db.0, 5000) })?;
        let mut version = db.prepare(c"PRAGMA user_version")?;
        version.row()?;
        let version_number = version.integer(0);
        drop(version);
        if !matches!(version_number, 0 | 3) {
            return Err(format!(
                "Unsupported world database version {version_number}"
            ));
        }
        db.execute(c"PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA cache_size=-4096; PRAGMA wal_autocheckpoint=1000; PRAGMA journal_size_limit=16777216;")?;
        db.execute(c"BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS metadata (id INTEGER PRIMARY KEY CHECK(id=1), seed INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS chunks (x INTEGER NOT NULL, z INTEGER NOT NULL, data BLOB NOT NULL, PRIMARY KEY(x,z));
            CREATE TABLE IF NOT EXISTS game_state (id INTEGER PRIMARY KEY CHECK(id=1), data BLOB NOT NULL);
            CREATE TABLE IF NOT EXISTS window_state (id INTEGER PRIMARY KEY CHECK(id=1), x INTEGER NOT NULL, y INTEGER NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL, maximized INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS interaction (id INTEGER PRIMARY KEY CHECK(id=1), selected INTEGER NOT NULL, revision INTEGER NOT NULL);
            INSERT OR IGNORE INTO interaction VALUES(1,3,0);
            CREATE TABLE IF NOT EXISTS hotbar (id INTEGER PRIMARY KEY CHECK(id=1), slots BLOB NOT NULL, selected INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS edits (x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, material INTEGER NOT NULL, PRIMARY KEY(x,z,y)) WITHOUT ROWID;
            CREATE TABLE IF NOT EXISTS dirty_chunks (x INTEGER NOT NULL, z INTEGER NOT NULL, PRIMARY KEY(x,z)) WITHOUT ROWID;
            PRAGMA user_version=3; COMMIT;")?;
        let mut seed = db.prepare(c"INSERT OR IGNORE INTO metadata(id,seed) VALUES(1,?)")?;
        seed.bind_int(1, world::WORLD_SEED as i64)?;
        seed.done()?;
        drop(seed);
        Ok(db)
    }

    fn check(&self, code: i32) -> Result<()> {
        if code == sql::SQLITE_OK {
            Ok(())
        } else {
            // SAFETY: SQLite keeps its connection error string valid until the next API call.
            Err(unsafe { CStr::from_ptr(sql::sqlite3_errmsg(self.0)) }
                .to_string_lossy()
                .into_owned())
        }
    }

    fn execute(&self, text: &CStr) -> Result<()> {
        // SAFETY: The connection and SQL string remain valid for the duration of the call.
        self.check(unsafe {
            sql::sqlite3_exec(
                self.0,
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
            sql::sqlite3_prepare_v2(self.0, text.as_ptr(), -1, &mut raw, ptr::null_mut())
        })?;
        Ok(Statement { raw, db: self })
    }

    pub(crate) fn seed(&self) -> Result<u64> {
        let mut stmt = self.prepare(c"SELECT seed FROM metadata WHERE id=1")?;
        if !stmt.row()? {
            return Err("World seed is missing".into());
        }
        Ok(stmt.integer(0) as u64)
    }

    pub(crate) fn load_chunk(&self, (x, z): Coord) -> Result<Option<Vec<u8>>> {
        let mut stmt = self.prepare(c"SELECT data FROM chunks WHERE x=? AND z=? AND NOT EXISTS (SELECT 1 FROM dirty_chunks WHERE dirty_chunks.x=chunks.x AND dirty_chunks.z=chunks.z)")?;
        stmt.bind_int(1, x)?;
        stmt.bind_int(2, z)?;
        if stmt.row()? {
            Ok(Some(decompress_chunk(stmt.blob()?)?))
        } else {
            Ok(None)
        }
    }

    fn load_base(&self, (x, z): Coord) -> Result<Option<Vec<u8>>> {
        let mut stmt = self.prepare(c"SELECT data FROM chunks WHERE x=? AND z=?")?;
        stmt.bind_int(1, x)?;
        stmt.bind_int(2, z)?;
        if stmt.row()? {
            Ok(Some(decompress_chunk(stmt.blob()?)?))
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

    // The block selected before the hotbar existed.
    fn selected(&self) -> Result<u8> {
        let mut stmt = self.prepare(c"SELECT selected FROM interaction WHERE id=1")?;
        stmt.row()?;
        let material = u8::try_from(stmt.integer(0)).map_err(|e| e.to_string())?;
        if matches!(material, 64 | 65) {
            return Ok(Hotbar::default().material());
        }
        world::canonical_material(material).ok_or_else(|| "Invalid selected block".into())
    }

    pub(crate) fn hotbar(&self) -> Result<Hotbar> {
        let mut stmt = self.prepare(c"SELECT slots,selected FROM hotbar WHERE id=1")?;
        let mut hotbar = Hotbar::default();
        if !stmt.row()? {
            // Worlds from before the hotbar start with their selected block in it.
            drop(stmt);
            hotbar.pick(self.selected()?);
            return Ok(hotbar);
        }
        let slots = stmt.blob()?;
        if slots.len() != inventory::SLOTS {
            return Err("Invalid hotbar size".into());
        }
        for (slot, &material) in hotbar.slots.iter_mut().zip(&slots) {
            if !matches!(material, 64 | 65) {
                *slot = world::canonical_material(material).ok_or("Invalid hotbar block")?;
            }
        }
        hotbar.selected = usize::try_from(stmt.integer(1))
            .ok()
            .filter(|&selected| selected < inventory::SLOTS)
            .ok_or("Invalid selected hotbar slot")?;
        Ok(hotbar)
    }

    pub(crate) fn save_hotbar(&self, hotbar: &Hotbar) -> Result<()> {
        if !hotbar
            .slots
            .iter()
            .all(|&material| world::valid_material(material))
        {
            return Err("Invalid hotbar block".into());
        }
        let mut stmt =
            self.prepare(c"INSERT OR REPLACE INTO hotbar(id,slots,selected) VALUES(1,?,?)")?;
        stmt.bind_blob(1, &hotbar.slots)?;
        stmt.bind_int(2, hotbar.selected as i64)?;
        stmt.done()
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
            let material = if matches!(material, 0 | 64 | 65) {
                0
            } else if let Some(material) = world::canonical_material(material) {
                material
            } else {
                return Err("Invalid saved block edit".into());
            };
            if !(3..world::HEIGHT as i64).contains(&position[1]) {
                return Err("Invalid saved block edit".into());
            }
            edits.push(Edit { position, material });
        }
        Ok(edits)
    }

    pub(crate) fn edit(&self, edit: Edit) -> Result<u64> {
        if (edit.material != 0 && !world::valid_material(edit.material))
            || !(3..world::HEIGHT as i64).contains(&edit.position[1])
        {
            return Err("Cannot edit this block".into());
        }
        self.execute(c"BEGIN IMMEDIATE")?;
        let result = (|| {
            let mut stmt = self.prepare(c"INSERT INTO edits(x,y,z,material) VALUES(?,?,?,?) ON CONFLICT(x,z,y) DO UPDATE SET material=excluded.material")?;
            for (i, value) in edit
                .position
                .into_iter()
                .chain([i64::from(edit.material)])
                .enumerate()
            {
                stmt.bind_int(i as i32 + 1, value)?;
            }
            stmt.done()?;
            let mut invalidate =
                self.prepare(c"INSERT OR IGNORE INTO dirty_chunks(x,z) SELECT x,z FROM chunks WHERE x BETWEEN ? AND ? AND z BETWEEN ? AND ?")?;
            let x = edit.position[0].div_euclid(16);
            let z = edit.position[2].div_euclid(16);
            for (i, value) in [x - 1, x + 1, z - 1, z + 1].into_iter().enumerate() {
                invalidate.bind_int(i as i32 + 1, value)?;
            }
            invalidate.done()?;
            self.execute(c"UPDATE interaction SET revision=revision+1 WHERE id=1; COMMIT")?;
            self.revision()
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
                .collect::<Result<std::collections::HashMap<_, _>>>()?;
            let terrain = ChunkData::rebuild(coord, bases, &edits);
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

    pub(crate) fn load_window(&self) -> Result<Option<WindowState>> {
        let mut stmt =
            self.prepare(c"SELECT x,y,width,height,maximized FROM window_state WHERE id=1")?;
        if !stmt.row()? {
            return Ok(None);
        }
        let width = u32::try_from(stmt.integer(2)).map_err(|e| e.to_string())?;
        let height = u32::try_from(stmt.integer(3)).map_err(|e| e.to_string())?;
        if !(100..=16384).contains(&width) || !(100..=16384).contains(&height) {
            return Err("Invalid saved window size".into());
        }
        Ok(Some(WindowState {
            x: i32::try_from(stmt.integer(0)).map_err(|e| e.to_string())?,
            y: i32::try_from(stmt.integer(1)).map_err(|e| e.to_string())?,
            width,
            height,
            maximized: stmt.integer(4) != 0,
        }))
    }

    pub(crate) fn save_session(
        &self,
        camera: &Camera,
        night: bool,
        window: WindowState,
    ) -> Result<()> {
        self.execute(c"BEGIN IMMEDIATE")?;
        let result = (|| {
            self.save_state(camera, night)?;
            let mut stmt = self.prepare(c"INSERT INTO window_state(id,x,y,width,height,maximized) VALUES(1,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET x=excluded.x,y=excluded.y,width=excluded.width,height=excluded.height,maximized=excluded.maximized")?;
            for (i, value) in [
                i64::from(window.x),
                i64::from(window.y),
                i64::from(window.width),
                i64::from(window.height),
                i64::from(window.maximized),
            ]
            .into_iter()
            .enumerate()
            {
                stmt.bind_int(i as i32 + 1, value)?;
            }
            stmt.done()?;
            self.execute(c"COMMIT")
        })();
        if result.is_err() {
            let _ = self.execute(c"ROLLBACK");
        }
        result
    }

    pub(crate) fn restore_state(&self, camera: &mut Camera) -> Result<bool> {
        let mut stmt = self.prepare(c"SELECT data FROM game_state WHERE id=1")?;
        if !stmt.row()? {
            return Ok(false);
        }
        let data = stmt.blob()?;
        if !matches!(data.len(), 41 | 42) || data[40] > 1 || (data.len() == 42 && data[41] > 1) {
            return Err("Invalid saved game state".into());
        }
        let values: Vec<_> = data[..40]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|v| f64::from_le_bytes(*v))
            .collect();
        if values.iter().any(|v| !v.is_finite())
            || values[..3].iter().any(|v| v.abs() > 1e12)
            || values[3].abs() > f64::from(f32::MAX)
            || values[4].abs() > f64::from(1.55f32)
        {
            return Err("Invalid saved camera coordinates".into());
        }
        camera.position.copy_from_slice(&values[..3]);
        camera.yaw = values[3] as f32;
        camera.pitch = values[4] as f32;
        // Older saves predate walking and retain their original flying behavior.
        camera.set_flying(data.get(41).is_none_or(|&mode| mode != 0));
        Ok(data[40] != 0)
    }

    pub(crate) fn save_state(&self, camera: &Camera, night: bool) -> Result<()> {
        let mut data = Vec::with_capacity(42);
        for value in camera
            .position
            .into_iter()
            .chain([f64::from(camera.yaw), f64::from(camera.pitch)])
        {
            data.extend_from_slice(&value.to_le_bytes());
        }
        data.push(u8::from(night));
        data.push(u8::from(camera.flying));
        let mut stmt = self.prepare(c"INSERT INTO game_state(id,data) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data")?;
        stmt.bind_blob(1, &data)?;
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
    fn blob(&self) -> Result<Vec<u8>> {
        // SAFETY: Callers read columns only while the statement points at a row.
        let size = unsafe { sql::sqlite3_column_bytes(self.raw, 0) };
        if !(1..=MAX_BLOB_SIZE as i32).contains(&size) {
            return Err("Invalid saved blob size".into());
        }
        // SAFETY: The statement remains on the same row while the blob is copied.
        let data = unsafe { sql::sqlite3_column_blob(self.raw, 0) };
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
            sql::sqlite3_close_v2(self.0);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    pub(crate) struct TestWorld {
        directory: std::path::PathBuf,
        pub(crate) path: String,
    }
    impl TestWorld {
        pub(crate) fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir()
                .join(format!("plaatcraft-world-test-{}-{id}", std::process::id()));
            std::fs::create_dir(&directory).unwrap();
            let path = directory.join("world.db").to_str().unwrap().to_owned();
            Self { directory, path }
        }
    }
    impl Drop for TestWorld {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.directory).unwrap();
        }
    }

    #[test]
    fn chunk_blobs_use_a_rowid_table_to_avoid_overflow_page_waste() {
        let db = Database::open(":memory:").unwrap();
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
    fn movement_mode_roundtrips_and_legacy_saves_remain_flying() {
        let db = Database::open(":memory:").unwrap();
        let mut camera = Camera::default();
        assert!(!camera.flying);
        db.save_state(&camera, false).unwrap();
        let mut restored = Camera::default();
        restored.set_flying(true);
        db.restore_state(&mut restored).unwrap();
        assert!(!restored.flying);
        camera.set_flying(true);
        db.save_state(&camera, false).unwrap();
        db.restore_state(&mut restored).unwrap();
        assert!(restored.flying);
        db.execute(c"UPDATE game_state SET data=substr(data,1,41)")
            .unwrap();
        restored.set_flying(false);
        db.restore_state(&mut restored).unwrap();
        assert!(restored.flying);
    }

    #[test]
    fn retired_materials_are_removed_from_saved_edits_and_hotbars() {
        let db = Database::open(":memory:").unwrap();
        for material in [64, 65] {
            let mut selected = db
                .prepare(c"UPDATE interaction SET selected=? WHERE id=1")
                .unwrap();
            selected.bind_int(1, i64::from(material)).unwrap();
            selected.done().unwrap();
            assert_eq!(db.selected().unwrap(), Hotbar::default().material());
            db.save_hotbar(&Hotbar::default()).unwrap();
            let mut hotbar = Hotbar::default();
            hotbar.slots[0] = material;
            let mut slots = db.prepare(c"UPDATE hotbar SET slots=? WHERE id=1").unwrap();
            slots.bind_blob(1, &hotbar.slots).unwrap();
            slots.done().unwrap();
            assert_eq!(db.hotbar().unwrap(), Hotbar::default());

            let mut edit = db
                .prepare(c"INSERT OR REPLACE INTO edits(x,y,z,material) VALUES(1,120,1,?)")
                .unwrap();
            edit.bind_int(1, i64::from(material)).unwrap();
            edit.done().unwrap();
            assert_eq!(db.load_edits((0, 0)).unwrap()[0].material, 0);

            assert_eq!(db.terrain((0, 0)).unwrap().0.voxel([1, 120, 1]), 0);
        }
    }

    #[test]
    fn legacy_iron_selections_and_edits_use_canonical_materials() {
        let db = Database::open(":memory:").unwrap();
        db.execute(
            c"UPDATE interaction SET selected=17 WHERE id=1;
            INSERT INTO edits(x,y,z,material) VALUES(1,60,1,57);",
        )
        .unwrap();

        assert_eq!(db.selected().unwrap(), 50);
        // The hotbar migrates the legacy selection into the selected slot.
        assert_eq!(db.hotbar().unwrap().material(), 50);
        assert_eq!(db.load_edits((0, 0)).unwrap()[0].material, 51);
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
            let revision = db.edit(edit).unwrap();
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
            db.save_hotbar(&hotbar).unwrap();
        }
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.hotbar().unwrap().material(), 25);
        assert_eq!(db.terrain((-1, 0)).unwrap().0.voxel([15, 120, 0]), 24);
        db.edit(Edit {
            material: 0,
            ..edit
        })
        .unwrap();
        assert_eq!(db.terrain((-1, 0)).unwrap().0.voxel([15, 120, 0]), 0);
        assert!(
            db.edit(Edit {
                position: [0, 0, 0],
                material: 0
            })
            .is_err()
        );
    }

    #[test]
    fn wal_reopens_chunks_seed_camera_and_window() {
        let world = TestWorld::new();
        let data = ChunkData::generate((-2, 1)).encode();
        let camera = Camera {
            position: [-192.25, 93.5, 400.125],
            yaw: 2.5,
            pitch: -1.55,
            ..Camera::default()
        };
        let window = WindowState {
            x: -1280,
            y: 120,
            width: 1100,
            height: 700,
            maximized: true,
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
            assert_eq!(db.seed().unwrap(), world::WORLD_SEED);
            db.execute(c"UPDATE metadata SET seed=12345 WHERE id=1")
                .unwrap();
            db.save_chunk((-2, 1), &data).unwrap();
            db.save_session(&camera, true, window).unwrap();
            let reader = Database::open(&world.path).unwrap();
            assert_eq!(reader.load_chunk((-2, 1)).unwrap().unwrap(), data);
            assert!(reader.load_chunk((999, 999)).unwrap().is_none());
        }
        let db = Database::open(&world.path).unwrap();
        assert_eq!(db.seed().unwrap(), 12345);
        assert_eq!(db.load_chunk((-2, 1)).unwrap().unwrap(), data);
        let mut restored = Camera::default();
        assert!(db.restore_state(&mut restored).unwrap());
        assert_eq!(restored.position, camera.position);
        assert_eq!(restored.yaw, camera.yaw);
        assert_eq!(restored.pitch, camera.pitch);
        assert_eq!(db.load_window().unwrap(), Some(window));
        db.save_session(
            &restored,
            false,
            WindowState {
                maximized: false,
                ..window
            },
        )
        .unwrap();
        assert!(!db.restore_state(&mut restored).unwrap());
        assert!(!db.load_window().unwrap().unwrap().maximized);
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
