/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The worlds and settings on a FAT32 SD card, the Wii VFS has no other storage backend.
//! A world is a SQLite database of the engine's schema named after the world, like on
//! computers, so worlds move between them. The Wii's SQLite has no WAL, so worlds use a
//! rollback journal with full syncs, computers switch them back to it when they close them.
//!
//! The Wii's SQLite is single threaded, so the open world's connection lives in [`Sqlite`]
//! and every SQLite call holds its lock, the stream's worker reads and saves chunks with it.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::ffi::CString;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char, c_int, c_long};
use core::ptr;

use libsqlite3_sys as sql;
use plaatcraft_engine::database::{self as schema, APPLICATION_ID, VERSION, validate_name};
use plaatcraft_engine::protocol::{MAX_BLOB_SIZE, Player};
use plaatcraft_engine::world::{ChunkData, Coord};

use crate::local::Saved;
use crate::settings::Settings;
use crate::sync::Mutex;

const ROOT: &str = "/data/plaatcraft/worlds";
const SETTINGS: &CStr = c"/data/plaatcraft/settings.db";

/// The connection of the open world, `None` while no world of this Wii is played.
pub type Sqlite = Mutex<Option<WorldDatabase>>;

/// A saved world, `modified` is when it was last saved in seconds since 1970.
pub struct WorldFile {
    pub name: String,
    pub modified: i64,
}

/// What the game needs of a world when it opens.
pub struct Opened {
    pub seed: u64,
    pub saved: Saved,
}

pub struct Storage {
    pub ready: bool,
    pub sqlite: &'static Sqlite,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            ready: mount(),
            sqlite: Box::leak(Box::new(Mutex::new(None))),
        }
    }

    /// Mounts the card again, so a replaced card is noticed. Only while no world is open.
    pub fn refresh(&mut self) -> bool {
        if self.sqlite.lock().is_none() {
            self.ready = mount();
        }
        self.ready
    }

    fn path(name: &str, suffix: &str) -> CString {
        CString::new(format!("{ROOT}/{name}.db{suffix}")).expect("World names have no NUL")
    }

    /// The worlds on the card, the most recently saved first.
    pub fn worlds(&self) -> Vec<WorldFile> {
        let mut worlds = Vec::new();
        if !self.ready {
            return worlds;
        }
        let directory = unsafe { opendir(c"/data/plaatcraft/worlds".as_ptr()) };
        if directory.is_null() {
            return worlds;
        }
        loop {
            let entry = unsafe { readdir(directory) };
            if entry.is_null() {
                break;
            }
            let file = unsafe { CStr::from_ptr((*entry).name.as_ptr()) };
            let Some(name) = file.to_str().ok().and_then(|f| f.strip_suffix(".db")) else {
                continue;
            };
            let mut status = Stat::default();
            if validate_name(name) == Ok(name)
                && unsafe { stat(Self::path(name, "").as_ptr(), &mut status) } == 0
            {
                worlds.push(WorldFile {
                    name: name.into(),
                    modified: status.modified.seconds,
                });
            }
        }
        unsafe { closedir(directory) };
        worlds.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.name.cmp(&b.name))
        });
        worlds
    }

    // FAT ignores case, so names that only differ in case are taken too.
    fn available<'a>(&self, name: &'a str) -> Result<&'a str, String> {
        let name = validate_name(name)?;
        if !self.ready {
            return Err("Insert a FAT32 SD card to save worlds".into());
        }
        if self
            .worlds()
            .iter()
            .any(|world| world.name.eq_ignore_ascii_case(name))
            || exists(&Self::path(name, ""))
        {
            return Err("A world with this name already exists".into());
        }
        Ok(name)
    }

    /// Creates a world with the seed and returns its trimmed name.
    pub fn create(&self, name: &str, seed: u64) -> Result<String, String> {
        let name = self.available(name)?;
        let _sqlite = self.sqlite.lock();
        let db = Database::open(&Self::path(name, ""), sql::SQLITE_OPEN_CREATE)?;
        db.transaction(|| {
            db.exec(schema::SCHEMA)?;
            let statement = db.prepare(schema::SET_SEED)?;
            statement.bind_integer(1, seed as i64)?;
            statement.done()
        })?;
        Ok(name.into())
    }

    /// Opens a world for the stream's worker and reads what the game needs of it.
    pub fn open(&self, name: &str) -> Result<Opened, String> {
        let mut sqlite = self.sqlite.lock();
        let db = Database::open(&Self::path(name, ""), 0)
            .map_err(|_| "Could not open this world, play it on a computer first")?;
        let opened = Self::read(&db)?;
        *sqlite = Some(WorldDatabase(db));
        Ok(opened)
    }

    fn read(db: &Database) -> Result<Opened, String> {
        let statement = db.prepare(schema::IDENTITY)?;
        if !statement.row()?
            || [statement.integer(0), statement.integer(1)] != [APPLICATION_ID, VERSION]
        {
            return Err("Unsupported world, it was left intact".into());
        }
        let statement = db.prepare(schema::SEED)?;
        if !statement.row()? {
            return Err("The world has no seed".into());
        }
        let seed = statement.integer(0) as u64;
        let statement = db.prepare(schema::PLAYER)?;
        let player = if statement.row()? {
            Player::decode(statement.blob(0)?)?
        } else {
            Player::default()
        };
        let statement = db.prepare(schema::FLUIDS)?;
        let mut fluids = Vec::new();
        while statement.row()? {
            fluids.push([
                statement.integer(0),
                statement.integer(1),
                statement.integer(2),
            ]);
        }
        Ok(Opened {
            seed,
            saved: Saved {
                player,
                guests: BTreeMap::new(),
                fluids,
            },
        })
    }

    /// Closes the open world, the stream saved it.
    pub fn close(&self) {
        self.sqlite.lock().take();
    }

    /// The saved player of a guest of the open world.
    pub fn guest(&self, name: &str) -> Result<Option<Player>, String> {
        let sqlite = self.sqlite.lock();
        let db = &sqlite.as_ref().ok_or("No world is open")?.0;
        let statement = db.prepare(schema::GUEST)?;
        statement.bind(1, name.as_bytes())?;
        if statement.row()? {
            Player::decode(statement.blob(0)?).map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<String, String> {
        let to = validate_name(to)?;
        if to != from {
            if !to.eq_ignore_ascii_case(from) {
                self.available(to)?;
            }
            let (from, to) = (Self::path(from, ""), Self::path(to, ""));
            if unsafe { rename(from.as_ptr(), to.as_ptr()) } != 0 {
                return Err("Could not rename the world".into());
            }
        }
        Ok(to.into())
    }

    /// Deletes a world with its journal, the journal first, so a failure leaves the world.
    pub fn delete(&self, name: &str) -> bool {
        self.ready && remove(&Self::path(name, "-journal")) && remove(&Self::path(name, ""))
    }

    pub fn load_settings(&self) -> Option<Settings> {
        if !self.ready {
            return None;
        }
        let _sqlite = self.sqlite.lock();
        let db = Database::open(SETTINGS, 0).ok()?;
        let statement = db.prepare(c"SELECT data FROM settings WHERE id=1").ok()?;
        if !statement.row().ok()? {
            return None;
        }
        Settings::decode(statement.blob(0).ok()?)
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        if !self.ready {
            return Err("Insert a FAT32 SD card to save the settings".into());
        }
        let _sqlite = self.sqlite.lock();
        let db = Database::open(SETTINGS, sql::SQLITE_OPEN_CREATE)?;
        db.exec(c"CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY CHECK(id=1), data BLOB NOT NULL)")?;
        let statement = db.prepare(c"INSERT OR REPLACE INTO settings(id,data) VALUES(1,?1)")?;
        statement.bind(1, &settings.encode())?;
        statement.done()
    }
}

/// The open world, which the stream's worker reads and saves chunks with.
pub struct WorldDatabase(Database);

impl WorldDatabase {
    /// The stored blocks of a chunk, `None` while it was never loaded.
    pub fn chunk(&self, (x, z): Coord) -> Result<Option<ChunkData>, String> {
        let statement = self.0.prepare(schema::CHUNK)?;
        statement.bind_integer(1, x)?;
        statement.bind_integer(2, z)?;
        if statement.row()? {
            ChunkData::decode_blocks(statement.blob(0)?).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Saves the encoded chunks, and the players and the flowing fluid when given, at once.
    pub fn save(
        &self,
        chunks: &BTreeMap<Coord, Vec<u8>>,
        saved: Option<&Saved>,
    ) -> Result<(), String> {
        let db = &self.0;
        db.transaction(|| {
            let statement = db.prepare(schema::SAVE_CHUNK)?;
            for (&(x, z), data) in chunks {
                statement.bind_integer(1, x)?;
                statement.bind_integer(2, z)?;
                statement.bind(3, data)?;
                statement.done()?;
            }
            let Some(saved) = saved else {
                return Ok(());
            };
            let statement = db.prepare(schema::SAVE_PLAYER)?;
            let player = saved.player.encode();
            statement.bind(1, &player)?;
            statement.done()?;
            let statement = db.prepare(schema::SAVE_GUEST)?;
            for (name, player) in &saved.guests {
                let player = player.encode();
                statement.bind(1, name.as_bytes())?;
                statement.bind(2, &player)?;
                statement.done()?;
            }
            db.exec(schema::CLEAR_FLUIDS)?;
            let statement = db.prepare(schema::ADD_FLUID)?;
            for position in &saved.fluids {
                for (index, &value) in (1..).zip(position) {
                    statement.bind_integer(index, value)?;
                }
                statement.done()?;
            }
            Ok(())
        })
    }
}

struct Database(*mut sql::sqlite3);

// The connection is only used under the lock of `Sqlite`.
unsafe impl Send for Database {}

impl Database {
    fn open(path: &CStr, flags: c_int) -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        let result = unsafe {
            sql::sqlite3_open_v2(
                path.as_ptr(),
                &mut raw,
                sql::SQLITE_OPEN_READWRITE | sql::SQLITE_OPEN_NOMUTEX | flags,
                ptr::null(),
            )
        };
        if raw.is_null() {
            return Err("Could not allocate the database".into());
        }
        let db = Self(raw);
        if result != sql::SQLITE_OK {
            return Err(db.error());
        }
        // An interrupted save is rolled back when the database is opened again
        db.exec(c"PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=-512")?;
        Ok(db)
    }

    fn error(&self) -> String {
        let message = unsafe { CStr::from_ptr(sql::sqlite3_errmsg(self.0)) };
        format!("SD card: {}", message.to_string_lossy())
    }

    fn check(&self, result: c_int) -> Result<(), String> {
        if result == sql::SQLITE_OK {
            Ok(())
        } else {
            Err(self.error())
        }
    }

    fn exec(&self, query: &CStr) -> Result<(), String> {
        self.check(unsafe {
            sql::sqlite3_exec(
                self.0,
                query.as_ptr(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        })
    }

    // Runs the writes in one transaction, which is rolled back when they fail.
    fn transaction(&self, writes: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        self.exec(c"BEGIN IMMEDIATE")?;
        let result = writes().and_then(|()| self.exec(c"COMMIT"));
        if result.is_err() {
            let _ = self.exec(c"ROLLBACK");
        }
        result
    }

    fn prepare(&self, query: &CStr) -> Result<Statement<'_>, String> {
        let mut raw = ptr::null_mut();
        let result = unsafe {
            sql::sqlite3_prepare_v2(self.0, query.as_ptr(), -1, &mut raw, ptr::null_mut())
        };
        let statement = Statement(raw, self);
        self.check(result)?;
        Ok(statement)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        unsafe {
            sql::sqlite3_close(self.0);
        }
    }
}

struct Statement<'a>(*mut sql::sqlite3_stmt, &'a Database);

impl Statement<'_> {
    // Steps to the next row, `false` when there is none.
    fn row(&self) -> Result<bool, String> {
        match unsafe { sql::sqlite3_step(self.0) } {
            sql::SQLITE_ROW => Ok(true),
            sql::SQLITE_DONE => Ok(false),
            _ => Err(self.1.error()),
        }
    }

    // Runs the statement and makes it ready to run again with new bindings.
    fn done(&self) -> Result<(), String> {
        let row = self.row();
        unsafe { sql::sqlite3_reset(self.0) };
        match row? {
            false => Ok(()),
            true => Err("Unexpected SQLite row".into()),
        }
    }

    fn integer(&self, column: c_int) -> i64 {
        unsafe { sql::sqlite3_column_int64(self.0, column) }
    }

    // The blob of a column of the current row, valid until the statement steps again.
    fn blob(&self, column: c_int) -> Result<&[u8], String> {
        let size = unsafe { sql::sqlite3_column_bytes(self.0, column) };
        let data = unsafe { sql::sqlite3_column_blob(self.0, column) };
        if data.is_null() || !(1..=MAX_BLOB_SIZE as c_int).contains(&size) {
            return Err("Invalid saved data".into());
        }
        Ok(unsafe { core::slice::from_raw_parts(data.cast(), size as usize) })
    }

    // SQLite borrows the data until the statement steps.
    fn bind(&self, index: c_int, data: &[u8]) -> Result<(), String> {
        let length = data.len() as c_int;
        self.1.check(unsafe {
            sql::sqlite3_bind_blob(self.0, index, data.as_ptr().cast(), length, None)
        })
    }

    fn bind_integer(&self, index: c_int, value: i64) -> Result<(), String> {
        self.1
            .check(unsafe { sql::sqlite3_bind_int64(self.0, index, value) })
    }
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        unsafe {
            sql::sqlite3_finalize(self.0);
        }
    }
}

// picolibc's directory entry and file status
#[repr(C)]
struct Dirent {
    inode: u64,
    kind: u8,
    name: [c_char; 256],
}

#[repr(C)]
#[derive(Default)]
struct Timespec {
    seconds: i64,
    nanoseconds: c_long,
}

#[repr(C)]
#[derive(Default)]
struct Stat {
    device: u64,
    inode: u64,
    mode: u32,
    links: u64,
    user: u32,
    group: u32,
    special: u64,
    size: i64,
    block_size: i32,
    blocks: i64,
    accessed: Timespec,
    modified: Timespec,
    changed: Timespec,
}

unsafe extern "C" {
    fn mkdir(path: *const c_char, mode: u32) -> c_int;
    fn access(path: *const c_char, mode: c_int) -> c_int;
    fn unlink(path: *const c_char) -> c_int;
    fn rename(from: *const c_char, to: *const c_char) -> c_int;
    fn stat(path: *const c_char, status: *mut Stat) -> c_int;
    fn opendir(path: *const c_char) -> *mut core::ffi::c_void;
    fn readdir(directory: *mut core::ffi::c_void) -> *mut Dirent;
    fn closedir(directory: *mut core::ffi::c_void) -> c_int;
    fn time(time: *mut i64) -> i64;
    static mut errno: c_int;
}

/// The current time in seconds since 1970, from the Wii's clock.
pub fn unix_time() -> i64 {
    unsafe { time(ptr::null_mut()) }
}

const ENOENT: c_int = 2;
const EEXIST: c_int = 17;

// Mounts the card again with all files closed, so a replaced card is noticed
fn mount() -> bool {
    if unsafe { libogc_sys::wiiMountSd(1) } == 0 {
        return false;
    }
    for path in [c"/data", c"/data/plaatcraft", c"/data/plaatcraft/worlds"] {
        if unsafe { mkdir(path.as_ptr(), 0o777) } != 0 && unsafe { errno } != EEXIST {
            return false;
        }
    }
    true
}

fn exists(path: &CStr) -> bool {
    unsafe { access(path.as_ptr(), 0) == 0 }
}

fn remove(path: &CStr) -> bool {
    unsafe { unlink(path.as_ptr()) == 0 || errno == ENOENT }
}
