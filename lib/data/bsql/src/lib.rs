/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![allow(unsafe_code)]

#[cfg(not(any(feature = "mysql", feature = "sqlite")))]
compile_error!("bsql requires the `mysql` or `sqlite` feature");

#[cfg(feature = "derive")]
pub use bsql_derive::run_migrations;
#[cfg(all(feature = "derive", any(feature = "mysql", feature = "sqlite")))]
pub use bsql_derive::{FromRow, FromValue};

#[cfg(any(feature = "mysql", feature = "sqlite"))]
pub use crate::bind::Bind;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
pub use crate::connection::{
    Connection, ConnectionError, DatabaseType, ExecutionResult, PoolOptions,
};
#[cfg(any(feature = "mysql", feature = "sqlite"))]
pub use crate::from_row::FromRow;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
pub use crate::migration::{Migration, MigrationError};
#[cfg(feature = "mysql")]
pub use crate::mysql::MysqlTransport;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
pub use crate::search::preprocess_fts_query;
#[cfg(feature = "sqlite")]
pub use crate::sqlite::SqliteMode;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
pub use crate::statement::{ColumnType, RawStatement, Statement, StatementError};
#[cfg(any(feature = "mysql", feature = "sqlite"))]
pub use crate::value::{Value, ValueError};

#[cfg(any(feature = "mysql", feature = "sqlite"))]
mod bind;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
mod connection;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
mod from_row;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
mod migration;
#[cfg(feature = "mysql")]
mod mysql;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
mod search;
#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
mod statement;
#[cfg(any(feature = "mysql", feature = "sqlite"))]
mod value;
