/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The worlds and settings on a FAT32 SD card, the only storage the client uses. A world
//! is a SQLite database of the engine's schema named after the world, like on computers,
//! so worlds move between them. The consoles' SQLite has no WAL, so worlds use a rollback
//! journal with full syncs, computers switch them back to it when they close them.
//!
//! The consoles' SQLite is single threaded, so the open world's connection lives in
//! [`Sqlite`] and every SQLite call holds its lock, the stream's worker reads and saves
//! chunks with it.
#![allow(unsafe_code)]

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::ffi::CString;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_int};
use core::marker::PhantomData;
use core::ptr;

use libsqlite3_sys as sql;

use super::files;
use super::local::Saved;
use super::platform::Platform;
use super::settings::Settings;
use super::sync::Mutex;
use crate::database::{self as schema, APPLICATION_ID, VERSION, validate_name};
use crate::protocol::{MAX_BLOB_SIZE, Player};
use crate::world::{ChunkData, Coord};

const DIRECTORIES: [&str; 3] = ["/data", "/data/plaatcraft", ROOT];
const ROOT: &str = "/data/plaatcraft/worlds";
const SETTINGS: &CStr = c"/data/plaatcraft/settings.db";

/// The connection of the open world, `None` while no world of this console is played.
pub(super) type Sqlite<P> = Mutex<<P as Platform>::Lock, Option<WorldDatabase>>;

/// A saved world, `modified` is when it was last saved in seconds since 1970.
pub(super) type WorldFile = files::Entry;

/// What the game needs of a world when it opens.
pub(super) struct Opened {
    pub(super) seed: u64,
    pub(super) saved: Saved,
}

pub(super) struct Storage<P: Platform> {
    pub(super) ready: bool,
    pub(super) sqlite: &'static Sqlite<P>,
    platform: PhantomData<P>,
}

impl<P: Platform> Storage<P> {
    pub(super) fn new() -> Self {
        Self {
            ready: mount::<P>(),
            sqlite: Box::leak(Box::new(Mutex::new(None))),
            platform: PhantomData,
        }
    }

    /// Mounts the card again, so a replaced card is noticed. Only while no world is open.
    pub(super) fn refresh(&mut self) -> bool {
        if self.sqlite.lock().is_none() {
            self.ready = mount::<P>();
        }
        self.ready
    }

    fn path(name: &str, suffix: &str) -> String {
        format!("{ROOT}/{name}.db{suffix}")
    }

    /// The worlds on the card, the most recently saved first.
    pub(super) fn worlds(&self) -> Vec<WorldFile> {
        if !self.ready {
            return Vec::new();
        }
        let mut worlds: Vec<_> = files::entries(ROOT)
            .into_iter()
            .filter_map(|mut entry| {
                let name = entry.name.strip_suffix(".db")?;
                let length = name.len();
                (validate_name(name) == Ok(name)).then(|| {
                    entry.name.truncate(length);
                    entry
                })
            })
            .collect();
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
            || files::exists(&Self::path(name, ""))
        {
            return Err("A world with this name already exists".into());
        }
        Ok(name)
    }

    /// Creates a world with the seed and returns its trimmed name.
    pub(super) fn create(&self, name: &str, seed: u64) -> Result<String, String> {
        let name = self.available(name)?;
        let _sqlite = self.sqlite.lock();
        let db = Database::open(&c_path(&Self::path(name, "")), sql::SQLITE_OPEN_CREATE)?;
        db.transaction(|| {
            db.exec(schema::SCHEMA)?;
            let statement = db.prepare(schema::SET_SEED)?;
            statement.bind_integer(1, seed as i64)?;
            statement.done()
        })?;
        Ok(name.into())
    }

    /// Opens a world for the stream's worker and reads what the game needs of it.
    pub(super) fn open(&self, name: &str) -> Result<Opened, String> {
        let mut sqlite = self.sqlite.lock();
        let db = Database::open(&c_path(&Self::path(name, "")), 0)
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
    pub(super) fn close(&self) {
        self.sqlite.lock().take();
    }

    /// The saved player of a guest of the open world.
    pub(super) fn guest(&self, name: &str) -> Result<Option<Player>, String> {
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

    pub(super) fn rename(&self, from: &str, to: &str) -> Result<String, String> {
        let to = validate_name(to)?;
        if to != from {
            if !to.eq_ignore_ascii_case(from) {
                self.available(to)?;
            }
            if !files::move_file(&Self::path(from, ""), &Self::path(to, "")) {
                return Err("Could not rename the world".into());
            }
        }
        Ok(to.into())
    }

    /// Deletes a world with its journal, the journal first, so a failure leaves the world.
    pub(super) fn delete(&self, name: &str) -> bool {
        self.ready
            && files::remove(&Self::path(name, "-journal"))
            && files::remove(&Self::path(name, ""))
    }

    pub(super) fn load_settings(&self) -> Option<Settings> {
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

    pub(super) fn save_settings(&self, settings: &Settings) -> Result<(), String> {
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
pub(super) struct WorldDatabase(Database);

impl WorldDatabase {
    /// The stored blocks of a chunk, `None` while it was never loaded.
    pub(super) fn chunk(&self, (x, z): Coord) -> Result<Option<ChunkData>, String> {
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
    pub(super) fn save(
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

// SAFETY: The connection is only used under the lock of `Sqlite`.
unsafe impl Send for Database {}

impl Database {
    fn open(path: &CStr, flags: c_int) -> Result<Self, String> {
        let mut raw = ptr::null_mut();
        // SAFETY: The path is a C string, SQLite writes the connection to raw.
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
        // SAFETY: The connection is open, SQLite returns a C string it owns.
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
        // SAFETY: The connection is open and the query is a C string.
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
        // SAFETY: The connection is open and the query is a C string, SQLite writes the
        // statement to raw.
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
        // SAFETY: The statements borrow the connection, so they are finalized already.
        unsafe {
            sql::sqlite3_close(self.0);
        }
    }
}

struct Statement<'a>(*mut sql::sqlite3_stmt, &'a Database);

impl Statement<'_> {
    // Steps to the next row, `false` when there is none.
    fn row(&self) -> Result<bool, String> {
        // SAFETY: The statement was prepared, null ones only after a failed prepare.
        match unsafe { sql::sqlite3_step(self.0) } {
            sql::SQLITE_ROW => Ok(true),
            sql::SQLITE_DONE => Ok(false),
            _ => Err(self.1.error()),
        }
    }

    // Runs the statement and makes it ready to run again with new bindings.
    fn done(&self) -> Result<(), String> {
        let row = self.row();
        // SAFETY: The statement was prepared.
        unsafe { sql::sqlite3_reset(self.0) };
        match row? {
            false => Ok(()),
            true => Err("Unexpected SQLite row".into()),
        }
    }

    fn integer(&self, column: c_int) -> i64 {
        // SAFETY: The statement was prepared and stepped to a row.
        unsafe { sql::sqlite3_column_int64(self.0, column) }
    }

    // The blob of a column of the current row, valid until the statement steps again.
    fn blob(&self, column: c_int) -> Result<&[u8], String> {
        // SAFETY: The statement was prepared and stepped to a row.
        let size = unsafe { sql::sqlite3_column_bytes(self.0, column) };
        // SAFETY: The statement was prepared and stepped to a row.
        let data = unsafe { sql::sqlite3_column_blob(self.0, column) };
        if data.is_null() || !(1..=MAX_BLOB_SIZE as c_int).contains(&size) {
            return Err("Invalid saved data".into());
        }
        // SAFETY: SQLite keeps the blob of the size until the statement steps again, the slice
        // borrows the statement.
        Ok(unsafe { core::slice::from_raw_parts(data.cast(), size as usize) })
    }

    // SQLite borrows the data until the statement steps.
    fn bind(&self, index: c_int, data: &[u8]) -> Result<(), String> {
        let length = data.len() as c_int;
        // SAFETY: The statement was prepared and the data outlives its next step.
        self.1.check(unsafe {
            sql::sqlite3_bind_blob(self.0, index, data.as_ptr().cast(), length, None)
        })
    }

    fn bind_integer(&self, index: c_int, value: i64) -> Result<(), String> {
        self.1
            // SAFETY: The statement was prepared.
            .check(unsafe { sql::sqlite3_bind_int64(self.0, index, value) })
    }
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        // SAFETY: Finalizing accepts null statements and the statement isn't used after.
        unsafe {
            sql::sqlite3_finalize(self.0);
        }
    }
}

fn c_path(path: &str) -> CString {
    CString::new(path).expect("Paths have no NUL")
}

// Mounts the card again with all files closed, so a replaced card is noticed
fn mount<P: Platform>() -> bool {
    P::mount_storage() && files::create_directories(&DIRECTORIES)
}
