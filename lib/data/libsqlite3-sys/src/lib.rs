/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A minimal replacement for the [libsqlite3-sys](https://crates.io/crates/libsqlite3-sys) crate.

#![allow(non_camel_case_types, non_snake_case, missing_docs)]
#![cfg_attr(not(test), no_std)]

use core::ffi::{c_char, c_int, c_longlong, c_uchar, c_void};

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sqlite3 {
    _unused: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct sqlite3_stmt {
    _unused: [u8; 0],
}

pub type sqlite3_int64 = c_longlong;
pub type sqlite3_destructor_type = Option<unsafe extern "C" fn(arg1: *mut c_void)>;

pub const SQLITE_OK: i32 = 0;

pub const SQLITE_OPEN_READONLY: i32 = 0x00000001;
pub const SQLITE_OPEN_READWRITE: i32 = 0x00000002;
pub const SQLITE_OPEN_CREATE: i32 = 0x00000004;
pub const SQLITE_OPEN_NOMUTEX: i32 = 0x00008000;

pub const SQLITE_ROW: i32 = 100;
pub const SQLITE_DONE: i32 = 101;

pub const SQLITE_INTEGER: i32 = 1;
pub const SQLITE_FLOAT: i32 = 2;
pub const SQLITE_TEXT: i32 = 3;
pub const SQLITE_BLOB: i32 = 4;
pub const SQLITE_NULL: i32 = 5;

pub const SQLITE_FCNTL_PERSIST_WAL: i32 = 10;

#[must_use]
#[allow(unsafe_code)]
pub fn SQLITE_TRANSIENT() -> sqlite3_destructor_type {
    // SAFETY: The SQLite docs say that the value of `SQLITE_TRANSIENT` is -1.
    Some(unsafe { core::mem::transmute::<isize, unsafe extern "C" fn(*mut c_void)>(-1) })
}

#[allow(unsafe_code)]
unsafe extern "C" {
    pub fn sqlite3_threadsafe() -> c_int;
    pub fn sqlite3_free(ptr: *mut c_void);

    // sqlite3
    pub fn sqlite3_open_v2(
        filename: *const c_char,
        ppDb: *mut *mut sqlite3,
        flags: c_int,
        zVfs: *const c_char,
    ) -> c_int;
    pub fn sqlite3_file_control(
        db: *mut sqlite3,
        zDbName: *const c_char,
        op: c_int,
        arg: *mut c_void,
    ) -> c_int;
    pub fn sqlite3_busy_timeout(db: *mut sqlite3, ms: c_int) -> c_int;
    pub fn sqlite3_exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: Option<
            unsafe extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int,
        >,
        data: *mut c_void,
        errmsg: *mut *mut c_char,
    ) -> c_int;
    pub fn sqlite3_prepare_v2(
        db: *mut sqlite3,
        zSql: *const c_char,
        nByte: c_int,
        ppStmt: *mut *mut sqlite3_stmt,
        pzTail: *mut *const c_char,
    ) -> c_int;
    pub fn sqlite3_stmt_readonly(statement: *mut sqlite3_stmt) -> c_int;
    pub fn sqlite3_changes(db: *mut sqlite3) -> c_int;
    pub fn sqlite3_last_insert_rowid(db: *mut sqlite3) -> sqlite3_int64;
    pub fn sqlite3_get_autocommit(db: *mut sqlite3) -> c_int;
    pub fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
    pub fn sqlite3_close(db: *mut sqlite3) -> c_int;

    // sqlite3_stmt
    pub fn sqlite3_db_handle(pStmt: *mut sqlite3_stmt) -> *mut sqlite3;
    pub fn sqlite3_sql(pStmt: *mut sqlite3_stmt) -> *const c_char;
    pub fn sqlite3_step(pStmt: *mut sqlite3_stmt) -> c_int;
    pub fn sqlite3_reset(pStmt: *mut sqlite3_stmt) -> c_int;
    pub fn sqlite3_finalize(pStmt: *mut sqlite3_stmt) -> c_int;

    pub fn sqlite3_bind_parameter_index(pStmt: *mut sqlite3_stmt, zName: *const c_char) -> c_int;
    pub fn sqlite3_bind_null(pStmt: *mut sqlite3_stmt, i: c_int) -> c_int;
    pub fn sqlite3_bind_int64(pStmt: *mut sqlite3_stmt, i: c_int, value: sqlite3_int64) -> c_int;
    pub fn sqlite3_bind_double(pStmt: *mut sqlite3_stmt, i: c_int, value: f64) -> c_int;
    pub fn sqlite3_bind_text(
        pStmt: *mut sqlite3_stmt,
        i: c_int,
        z: *const c_char,
        n: c_int,
        xDel: sqlite3_destructor_type,
    ) -> c_int;
    pub fn sqlite3_bind_blob(
        pStmt: *mut sqlite3_stmt,
        i: c_int,
        z: *const c_void,
        n: c_int,
        xDel: sqlite3_destructor_type,
    ) -> c_int;

    pub fn sqlite3_column_count(pStmt: *mut sqlite3_stmt) -> c_int;
    pub fn sqlite3_column_name(pStmt: *mut sqlite3_stmt, iCol: c_int) -> *const c_char;
    pub fn sqlite3_column_type(pStmt: *mut sqlite3_stmt, iCol: c_int) -> c_int;
    pub fn sqlite3_column_decltype(pStmt: *mut sqlite3_stmt, iCol: c_int) -> *const c_char;
    pub fn sqlite3_column_table_name(pStmt: *mut sqlite3_stmt, iCol: c_int) -> *const c_char;
    pub fn sqlite3_column_origin_name(pStmt: *mut sqlite3_stmt, iCol: c_int) -> *const c_char;
    pub fn sqlite3_column_int64(pStmt: *mut sqlite3_stmt, iCol: c_int) -> sqlite3_int64;
    pub fn sqlite3_column_double(pStmt: *mut sqlite3_stmt, iCol: c_int) -> f64;
    pub fn sqlite3_column_text(pStmt: *mut sqlite3_stmt, iCol: c_int) -> *const c_uchar;
    pub fn sqlite3_column_blob(pStmt: *mut sqlite3_stmt, iCol: c_int) -> *const c_void;
    pub fn sqlite3_column_bytes(pStmt: *mut sqlite3_stmt, iCol: c_int) -> c_int;
}
