/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString, c_char};
use std::path::Path;
use std::ptr;

use libsqlite3_sys::*;

use super::Prepared;
use crate::connection::{Connection as GenericConnection, SqlitePoolOptions};
use crate::{ConnectionError, PoolOptions, StatementError};

/// SQLite database access mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqliteMode {
    /// Open an existing database for reads only.
    ReadOnly,
    /// Open a database for reads and writes, creating it when needed.
    ReadWrite,
}

pub(crate) struct Connection(*mut sqlite3);

// SAFETY: Connection exclusively owns its SQLite handle and never aliases mutable ownership.
unsafe impl Send for Connection {}

impl Connection {
    pub(crate) fn is_threadsafe() -> bool {
        // SAFETY: sqlite3_threadsafe takes no arguments and reads immutable compile-time state.
        unsafe { sqlite3_threadsafe() != 0 }
    }

    pub(crate) fn open(path: &Path, mode: SqliteMode) -> Result<Self, String> {
        let mut database = ptr::null_mut();
        let path = path
            .to_str()
            .ok_or_else(|| "database path is not valid Unicode".to_string())?;
        let path =
            CString::new(path).map_err(|_| "database path contains a null byte".to_string())?;
        // SAFETY: path is NUL-terminated, database is an output pointer, flags are valid, and a
        // null VFS selects SQLite's default implementation.
        let result = unsafe {
            sqlite3_open_v2(
                path.as_ptr(),
                &mut database,
                match mode {
                    SqliteMode::ReadOnly => SQLITE_OPEN_READONLY | SQLITE_OPEN_NOMUTEX,
                    SqliteMode::ReadWrite => {
                        SQLITE_OPEN_CREATE | SQLITE_OPEN_READWRITE | SQLITE_OPEN_NOMUTEX
                    }
                },
                ptr::null(),
            )
        };
        if result != SQLITE_OK {
            let error = if database.is_null() {
                "unknown error (database handle is null)".to_string()
            } else {
                // SAFETY: database is non-null and errmsg remains valid until the next API call.
                let error = unsafe { CStr::from_ptr(sqlite3_errmsg(database)) }
                    .to_string_lossy()
                    .into_owned();
                // SAFETY: database was returned by sqlite3_open_v2 and may be closed after failure.
                unsafe { sqlite3_close(database) };
                error
            };
            return Err(format!("failed to open SQLite database: {error}"));
        }
        Ok(Self(database))
    }

    pub(crate) fn execute_script(&self, sql: &str) -> Result<(), StatementError> {
        let sql = CString::new(sql)
            .map_err(|_| StatementError::new("SQL script contains a null byte"))?;
        let mut error_message: *mut c_char = ptr::null_mut();
        // SAFETY: the handle and SQL string are valid; error_message receives SQLite-owned memory.
        let result = unsafe {
            sqlite3_exec(
                self.0,
                sql.as_ptr(),
                None,
                ptr::null_mut(),
                &mut error_message,
            )
        };
        if result == SQLITE_OK {
            return Ok(());
        }
        let message = if error_message.is_null() {
            "unknown error".to_string()
        } else {
            // SAFETY: error_message is non-null and points to a NUL-terminated SQLite string.
            let message = unsafe { CStr::from_ptr(error_message) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: error_message was allocated by SQLite for sqlite3_exec.
            unsafe { sqlite3_free(error_message.cast()) };
            message
        };
        Err(StatementError::new(format!(
            "failed to execute SQLite script: {message}"
        )))
    }

    pub(crate) fn prepare(&self, query: &str) -> Result<Prepared, StatementError> {
        let mut statement = ptr::null_mut();
        let query_len = i32::try_from(query.len())
            .map_err(|_| StatementError::new("SQLite query is too large"))?;
        // SAFETY: the database is valid, query points to query_len bytes, and statement is an
        // initialized output pointer.
        let result = unsafe {
            sqlite3_prepare_v2(
                self.0,
                query.as_ptr().cast(),
                query_len,
                &mut statement,
                ptr::null_mut(),
            )
        };
        if result != SQLITE_OK {
            // SAFETY: self.0 is valid and errmsg remains valid until the next SQLite API call.
            let error = unsafe { CStr::from_ptr(sqlite3_errmsg(self.0)) }.to_string_lossy();
            return Err(StatementError::new(format!(
                "failed to prepare SQLite statement '{query}': {error}"
            )));
        }
        if statement.is_null() {
            return Err(StatementError::new(
                "SQLite query does not contain a statement",
            ));
        }
        Ok(Prepared(statement))
    }

    pub(crate) fn begin_transaction(&self) -> Result<(), StatementError> {
        self.execute_script("BEGIN IMMEDIATE")
    }

    pub(crate) fn commit(&self) -> Result<(), StatementError> {
        self.execute_script("COMMIT")
    }

    pub(crate) fn rollback(&self) -> Result<(), StatementError> {
        self.execute_script("ROLLBACK")
    }

    pub(crate) fn is_autocommit(&self) -> bool {
        // SAFETY: self.0 is a valid open SQLite handle.
        unsafe { sqlite3_get_autocommit(self.0) != 0 }
    }

    pub(crate) fn affected_rows(&self) -> u64 {
        // SAFETY: self.0 is a valid open SQLite handle.
        unsafe { sqlite3_changes(self.0) as u64 }
    }

    pub(crate) fn last_insert_row_id(&self) -> u64 {
        // SAFETY: self.0 is a valid open SQLite handle.
        unsafe { sqlite3_last_insert_rowid(self.0).try_into().unwrap_or(0) }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: self.0 is exclusively owned and is closed exactly once.
        unsafe { sqlite3_close(self.0) };
    }
}

impl GenericConnection {
    /// Open a SQLite database with the requested access mode and pool configuration.
    pub fn open_sqlite(
        path: impl AsRef<Path>,
        mode: SqliteMode,
        pool_options: PoolOptions,
    ) -> Result<Self, ConnectionError> {
        if path.as_ref() == Path::new(":memory:") {
            if mode == SqliteMode::ReadOnly {
                return Err(ConnectionError::new(
                    "in-memory SQLite databases cannot be opened read-only",
                ));
            }
            if pool_options.max_connections != 1 {
                return Err(ConnectionError::new(
                    "in-memory SQLite databases require exactly one connection",
                ));
            }
        }
        Self::from_sqlite_options(
            SqlitePoolOptions {
                path: path.as_ref().to_path_buf(),
                mode,
            },
            pool_options,
        )
    }

    /// Open an in-memory SQLite database.
    pub fn open_sqlite_memory() -> Result<Self, ConnectionError> {
        Self::from_sqlite_options(
            SqlitePoolOptions {
                path: ":memory:".into(),
                mode: SqliteMode::ReadWrite,
            },
            PoolOptions::single_connection(),
        )
    }

    /// Set the SQLite journal mode to write-ahead logging.
    pub fn enable_wal_logging(&self) -> Result<(), StatementError> {
        let pool = self.sqlite_pool()?;
        let writer = pool.acquire_writer()?;
        writer.connection().execute_script(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )
    }
}
