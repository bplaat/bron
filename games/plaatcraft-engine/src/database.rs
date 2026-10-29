/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! The world database, a SQLite file named after the world that every platform reads and
//! writes with these statements, so worlds move between them. It stores the seed, every
//! chunk that was loaded as [`ChunkData::encode_blocks`], the players as [`Player::encode`]
//! and the blocks whose fluid still flows.
//!
//! [`ChunkData::encode_blocks`]: crate::world::ChunkData::encode_blocks
//! [`Player::encode`]: crate::protocol::Player::encode

use alloc::format;
use alloc::string::String;
use core::ffi::CStr;

use crate::protocol::valid_char;

/// The longest world name in characters.
pub const MAX_NAME_LENGTH: usize = 32;

/// The `application_id` of world databases, "PLCW".
pub const APPLICATION_ID: i64 = 0x504c_4357;
/// The `user_version` of world databases.
pub const VERSION: i64 = 1;
/// Creates the tables of a world, run in a transaction.
pub const SCHEMA: &CStr = c"PRAGMA application_id=1347175255; PRAGMA user_version=1;
    CREATE TABLE IF NOT EXISTS metadata (id INTEGER PRIMARY KEY CHECK(id=1), seed INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS chunks (x INTEGER NOT NULL, z INTEGER NOT NULL, data BLOB NOT NULL, PRIMARY KEY(x,z));
    CREATE TABLE IF NOT EXISTS game_state (id INTEGER PRIMARY KEY CHECK(id=1), data BLOB NOT NULL);
    CREATE TABLE IF NOT EXISTS players (name BLOB PRIMARY KEY, data BLOB NOT NULL) WITHOUT ROWID;
    CREATE TABLE IF NOT EXISTS fluid_updates (x INTEGER NOT NULL, y INTEGER NOT NULL, z INTEGER NOT NULL, PRIMARY KEY(x,y,z)) WITHOUT ROWID;";
/// Reads the `application_id` and `user_version` of a database, both 0 when it's new.
pub const IDENTITY: &CStr =
    c"SELECT application_id, user_version FROM pragma_application_id, pragma_user_version";
/// Sets the seed of a new world.
pub const SET_SEED: &CStr = c"INSERT INTO metadata(id,seed) VALUES(1,?)";
/// Reads the seed.
pub const SEED: &CStr = c"SELECT seed FROM metadata WHERE id=1";
/// Reads the stored chunk at x and z.
pub const CHUNK: &CStr = c"SELECT data FROM chunks WHERE x=? AND z=?";
/// Stores the chunk at x and z.
pub const SAVE_CHUNK: &CStr =
    c"INSERT INTO chunks(x,z,data) VALUES(?,?,?) ON CONFLICT(x,z) DO UPDATE SET data=excluded.data";
/// Stores a generated chunk at x and z, unless it was stored already.
pub const ADD_CHUNK: &CStr = c"INSERT OR IGNORE INTO chunks(x,z,data) VALUES(?,?,?)";
/// Reads the host player.
pub const PLAYER: &CStr = c"SELECT data FROM game_state WHERE id=1";
/// Stores the host player.
pub const SAVE_PLAYER: &CStr =
    c"INSERT INTO game_state(id,data) VALUES(1,?) ON CONFLICT(id) DO UPDATE SET data=excluded.data";
/// Reads a guest by name.
pub const GUEST: &CStr = c"SELECT data FROM players WHERE name=?";
/// Stores a guest by name.
pub const SAVE_GUEST: &CStr = c"INSERT INTO players(name,data) VALUES(?,?) ON CONFLICT(name) DO UPDATE SET data=excluded.data";
/// Reads the blocks whose fluid still flows.
pub const FLUIDS: &CStr = c"SELECT x,y,z FROM fluid_updates";
/// Forgets the flowing fluid before it's stored again.
pub const CLEAR_FLUIDS: &CStr = c"DELETE FROM fluid_updates";
/// Stores a block whose fluid still flows.
pub const ADD_FLUID: &CStr = c"INSERT OR IGNORE INTO fluid_updates(x,y,z) VALUES(?,?,?)";

/// Checks a world name, which is also its file name, and returns it trimmed.
///
/// # Errors
///
/// When the name is empty, too long, or no safe file name.
pub fn validate_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        Err("Enter a name for the world".into())
    } else if name.chars().count() > MAX_NAME_LENGTH {
        Err(format!("Use at most {MAX_NAME_LENGTH} characters"))
    } else if name.starts_with('.') || name.ends_with('.') || !name.chars().all(valid_char) {
        Err("Names can't contain < > : \" / \\ | ? * or start or end with a dot".into())
    } else {
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use alloc::string::ToString;

    use super::*;

    #[test]
    fn names_are_trimmed_and_safe_file_names() {
        assert_eq!(validate_name("  Hello World "), Ok("Hello World"));
        for name in ["", "   ", ".hidden", "end.", "a/b", "a\\b", "what?", "a\tb"] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(validate_name(&"x".repeat(MAX_NAME_LENGTH)).is_ok());
        assert!(validate_name(&"x".repeat(MAX_NAME_LENGTH + 1)).is_err());
        assert_eq!(validate_name("x").map(ToString::to_string), Ok("x".into()));
    }

    #[test]
    fn schema_sets_the_identity() {
        let schema = SCHEMA.to_str().unwrap();
        assert!(schema.contains(&format!("application_id={APPLICATION_ID};")));
        assert!(schema.contains(&format!("user_version={VERSION};")));
    }
}
