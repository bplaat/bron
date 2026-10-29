/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! SQLite worlds on a FAT32 SD card. The Wii VFS has no other storage backend.
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::{CStr, c_char, c_int};
use core::ptr;

use libsqlite3_sys as sql;

use crate::snapshot::{MAX_SAVE_BYTES, Settings, Snapshot, WORLD_SLOTS};

const ROOT: &str = "/data/plaatcraft/worlds";
const SETTINGS: &str = "/data/plaatcraft/settings.db\0";
const APPLICATION_ID: i64 = 0x504c4357;
// The version of the world files.
const VERSION: i64 = 1;

pub struct Storage {
    pub ready: bool,
}
impl Storage {
    pub fn new() -> Self {
        Self { ready: mount() }
    }
    pub fn refresh(&mut self) -> bool {
        self.ready = mount();
        self.ready
    }
    fn path(slot: usize) -> String {
        format!("{ROOT}/world{}.db\0", slot + 1)
    }
    pub fn load(&self, slot: usize) -> Option<Snapshot> {
        if !self.ready || !mount() {
            return None;
        }
        self.read(slot)
    }
    // Reads a world of the already validated card. It's opened writable, so SQLite can roll
    // back the hot journal of a save that was interrupted, read-only it would fail to open.
    fn read(&self, slot: usize) -> Option<Snapshot> {
        if slot >= WORLD_SLOTS {
            return None;
        }
        let path = Self::path(slot);
        let db = Database::open(&path, sql::SQLITE_OPEN_READWRITE).ok()?;
        if db.integer(c"PRAGMA application_id").ok()? != APPLICATION_ID
            || db.integer(c"PRAGMA user_version").ok()? != VERSION
        {
            return None;
        }
        let mut statement = db.prepare(c"SELECT snapshot FROM state WHERE id=1").ok()?;
        Snapshot::decode(statement.blob(MAX_SAVE_BYTES)?)
    }
    /// The name and save time of the world in each slot.
    pub fn worlds(&self) -> Vec<Option<(String, i64)>> {
        // Validating remounts the card, so once for all slots.
        let valid = self.ready && mount();
        (0..WORLD_SLOTS)
            .map(|slot| {
                let save = if valid { self.read(slot) } else { None };
                save.map(|save| (save.name, save.saved)).or_else(|| {
                    if !self.ready {
                        return None;
                    }
                    let name = match exists(&Self::path(slot)) {
                        Ok(true) => "[Unreadable world]",
                        Ok(false) => return None,
                        Err(()) => "[SD card read error]",
                    };
                    Some((name.into(), 0))
                })
            })
            .collect()
    }
    pub fn load_settings(&self) -> Option<Settings> {
        if !self.ready || !mount() {
            return None;
        }
        let db = Database::open(SETTINGS, sql::SQLITE_OPEN_READWRITE).ok()?;
        let mut statement = db.prepare(c"SELECT data FROM settings WHERE id=1").ok()?;
        Settings::decode(statement.blob(256)?)
    }
    pub fn save_settings(&self, settings: &Settings) -> Result<(), &'static str> {
        if !mount() {
            return Err("Insert a FAT32 SD card to save the settings");
        }
        let db = Database::open(
            SETTINGS,
            sql::SQLITE_OPEN_READWRITE | sql::SQLITE_OPEN_CREATE,
        )?;
        db.exec(c"PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY CHECK(id=1), data BLOB NOT NULL);")?;
        // The bound data has to outlive the statement
        let data = settings.encode();
        let statement = db.prepare(c"INSERT OR REPLACE INTO settings(id,data) VALUES(1,?1)")?;
        if statement.bind(1, &data) && statement.done() {
            Ok(())
        } else {
            Err("Could not write the SD card settings")
        }
    }
    pub fn save(&self, slot: usize, save: &Snapshot) -> Result<(), &'static str> {
        if !self.ready || !mount() {
            return Err("Insert a FAT32 SD card to save this world");
        }
        if slot >= WORLD_SLOTS {
            return Err("Invalid world slot");
        }
        let data = save.encode().ok_or("World exceeds the 8 MiB save limit")?;
        let path = Self::path(slot);
        let exists = exists(&path).map_err(|_| "Could not read the SD-card world directory")?;
        let db = Database::open(
            &path,
            sql::SQLITE_OPEN_READWRITE | if exists { 0 } else { sql::SQLITE_OPEN_CREATE },
        )?;
        if exists {
            if db.integer(c"PRAGMA application_id")? != APPLICATION_ID
                || db.integer(c"PRAGMA user_version")? != VERSION
            {
                return Err("Unsupported world file; it was left intact");
            }
            if db.integer(c"SELECT seed FROM state WHERE id=1")? != save.seed as i64 {
                return Err("This SD card contains a different world in that slot");
            }
        }
        db.exec(c"PRAGMA temp_store=MEMORY; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA cache_size=-256; BEGIN IMMEDIATE;")?;
        let result = (|| {
            if !exists {
                // The pragmas take no parameters, these are APPLICATION_ID and VERSION
                db.exec(c"PRAGMA application_id=1347175255; PRAGMA user_version=1; CREATE TABLE state(id INTEGER PRIMARY KEY CHECK(id=1), seed INTEGER NOT NULL, snapshot BLOB NOT NULL);")?;
            }
            let statement =
                db.prepare(c"INSERT OR REPLACE INTO state(id,seed,snapshot) VALUES(1,?1,?2)")?;
            if !statement.bind_integer(1, save.seed as i64)
                || !statement.bind(2, &data)
                || !statement.done()
            {
                return Err("Could not write the SD-card world");
            }
            drop(statement);
            db.exec(c"COMMIT")
        })();
        if result.is_err() {
            let _ = db.exec(c"ROLLBACK");
        }
        result
    }
    pub fn delete(&self, slot: usize) -> bool {
        if !self.ready || slot >= WORLD_SLOTS || !mount() {
            return false;
        }
        let path = Self::path(slot);
        // Delete sidecars first; stop on errors so a partially failed deletion is visible.
        for suffix in ["-journal", ""] {
            let path = format!("{}{suffix}\0", path.trim_end_matches('\0'));
            if !remove(&path) {
                return false;
            }
        }
        true
    }
}
struct Database(*mut sql::sqlite3);
impl Database {
    fn open(path: &str, flags: i32) -> Result<Self, &'static str> {
        let mut raw = ptr::null_mut();
        let result = unsafe {
            sql::sqlite3_open_v2(
                path.as_ptr().cast(),
                &mut raw,
                flags | sql::SQLITE_OPEN_NOMUTEX,
                ptr::null(),
            )
        };
        if raw.is_null() {
            return Err("Could not allocate the world database");
        }
        let db = Self(raw);
        if result != sql::SQLITE_OK {
            return Err("Could not open the SD-card world");
        }
        Ok(db)
    }
    fn exec(&self, query: &CStr) -> Result<(), &'static str> {
        if unsafe {
            sql::sqlite3_exec(
                self.0,
                query.as_ptr(),
                None,
                ptr::null_mut(),
                ptr::null_mut(),
            )
        } == sql::SQLITE_OK
        {
            Ok(())
        } else {
            Err("SD-card database operation failed")
        }
    }
    fn prepare(&self, query: &CStr) -> Result<Statement<'_>, &'static str> {
        let mut raw = ptr::null_mut();
        if unsafe { sql::sqlite3_prepare_v2(self.0, query.as_ptr(), -1, &mut raw, ptr::null_mut()) }
            != sql::SQLITE_OK
        {
            if !raw.is_null() {
                unsafe {
                    sql::sqlite3_finalize(raw);
                }
            }
            return Err("Could not read the SD-card database");
        }
        Ok(Statement(raw, core::marker::PhantomData))
    }
    fn integer(&self, query: &CStr) -> Result<i64, &'static str> {
        let statement = self.prepare(query)?;
        if unsafe { sql::sqlite3_step(statement.0) } != sql::SQLITE_ROW {
            return Err("Could not read the SD card database");
        }
        Ok(unsafe { sql::sqlite3_column_int64(statement.0, 0) })
    }
}
impl Drop for Database {
    fn drop(&mut self) {
        unsafe {
            sql::sqlite3_close_v2(self.0);
        }
    }
}
struct Statement<'a>(
    *mut sql::sqlite3_stmt,
    core::marker::PhantomData<&'a Database>,
);
impl Statement<'_> {
    // The blob of the first column of the next row, at most max bytes. It's borrowed mutably,
    // because SQLite frees it when the statement steps again.
    fn blob(&mut self, max: usize) -> Option<&[u8]> {
        if unsafe { sql::sqlite3_step(self.0) } != sql::SQLITE_ROW {
            return None;
        }
        let size = unsafe { sql::sqlite3_column_bytes(self.0, 0) };
        let data = unsafe { sql::sqlite3_column_blob(self.0, 0) };
        if data.is_null() || !(1..=max as i32).contains(&size) {
            return None;
        }
        Some(unsafe { core::slice::from_raw_parts(data.cast(), size as usize) })
    }
    // The data stays borrowed by SQLite until the statement steps.
    fn bind(&self, index: i32, data: &[u8]) -> bool {
        let length = data.len() as i32;
        let result =
            unsafe { sql::sqlite3_bind_blob(self.0, index, data.as_ptr().cast(), length, None) };
        result == sql::SQLITE_OK
    }
    fn bind_integer(&self, index: i32, value: i64) -> bool {
        unsafe { sql::sqlite3_bind_int64(self.0, index, value) == sql::SQLITE_OK }
    }
    fn done(&self) -> bool {
        let result = unsafe { sql::sqlite3_step(self.0) };
        result == sql::SQLITE_DONE
    }
}
impl Drop for Statement<'_> {
    fn drop(&mut self) {
        unsafe {
            sql::sqlite3_finalize(self.0);
        }
    }
}

unsafe extern "C" {
    fn mkdir(path: *const c_char, mode: u32) -> c_int;
    fn access(path: *const c_char, mode: c_int) -> c_int;
    fn unlink(path: *const c_char) -> c_int;
    fn time(time: *mut i64) -> i64;
    static mut errno: c_int;
}
/// The current time in seconds since 1970, from the Wii's clock.
pub fn unix_time() -> i64 {
    unsafe { time(ptr::null_mut()) }
}
const ENOENT: c_int = 2;
const EEXIST: c_int = 17;
// Mounts the card again before each database operation, so a replaced card is noticed
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
fn exists(path: &str) -> Result<bool, ()> {
    if unsafe { access(path.as_ptr().cast(), 0) } == 0 {
        Ok(true)
    } else if unsafe { errno } == ENOENT {
        Ok(false)
    } else {
        Err(())
    }
}
fn remove(path: &str) -> bool {
    unsafe { unlink(path.as_ptr().cast()) == 0 || errno == ENOENT }
}
