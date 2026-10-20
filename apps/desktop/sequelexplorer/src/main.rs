/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

#![doc = include_str!("../README.md")]
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use bsql::{
    ColumnType, Connection, MysqlTransport, PoolOptions, SqliteMode, StatementError, Value,
};
use bwebview::{WebviewBuilder, WebviewEvent};
use bwindow::{
    Accelerator, Event, EventLoopBuilder, FileDialog, KeyCode, LogicalSize, MenuBarBuilder,
    MenuBuilder, MenuItem, Modifiers, Theme, WindowBuilder,
};
use keyring::{Entry as CredentialEntry, Error as CredentialError};
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use serde_json::json;
use small_http::{Request, Response, Status};
use small_router::RouterBuilder;
use zeroize::Zeroizing;

#[derive(Embed)]
#[folder = "web"]
struct WebAssets;

// MARK: IPC messages
#[derive(Deserialize, Serialize)]
#[allow(clippy::enum_variant_names)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum IpcMessage {
    Ready,
    MenuAction {
        action: String,
    },
    RestoreLastFile,
    OpenFile {
        path: String,
    },
    OpenFileDialog {
        request_id: u64,
    },
    OpenFileDialogResponse {
        request_id: u64,
        path: Option<String>,
    },
    OpenDatabase {
        request_id: u64,
        path: String,
    },
    OpenDatabaseResponse {
        request_id: u64,
        ok: bool,
        error: Option<String>,
    },
    OpenMysql {
        request_id: u64,
        transport: String,
        host: String,
        port: u16,
        socket: String,
        user: String,
        password: Option<Zeroizing<String>>,
        tls: bool,
        remember: bool,
        previous_connection: Option<MysqlCredentialIdentity>,
    },
    OpenMysqlResponse {
        request_id: u64,
        ok: bool,
        error: Option<String>,
        credential_saved: bool,
        credential_error: Option<String>,
    },
    SelectMysqlDatabase {
        request_id: u64,
        database: String,
    },
    SelectMysqlDatabaseResponse {
        request_id: u64,
        ok: bool,
        error: Option<String>,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct MysqlCredentialIdentity {
    transport: String,
    host: String,
    port: u16,
    socket: String,
    user: String,
}

// MARK: State
#[derive(Clone, Copy, PartialEq, Eq)]
enum DatabaseBackend {
    Sqlite,
    Mysql,
}

#[derive(Clone)]
enum MysqlConnectionSettings {
    Tcp {
        host: String,
        port: u16,
        user: String,
        password: Zeroizing<String>,
        tls: bool,
    },
    Unix {
        socket: String,
        user: String,
        password: Zeroizing<String>,
    },
}

impl MysqlConnectionSettings {
    fn password(&self) -> &str {
        match self {
            Self::Tcp { password, .. } | Self::Unix { password, .. } => password,
        }
    }

    fn connect(&self, database: Option<&str>) -> Result<Connection, String> {
        match self {
            Self::Tcp {
                host,
                port,
                user,
                password,
                tls,
            } => Connection::open_mysql(
                MysqlTransport::tcp(host.as_str(), *port, *tls),
                user,
                password.as_str(),
                database,
                PoolOptions::single_connection(),
            )
            .map_err(|error| error.to_string()),
            Self::Unix {
                socket,
                user,
                password,
            } => {
                #[cfg(unix)]
                {
                    Connection::open_mysql(
                        MysqlTransport::unix(socket.as_str()),
                        user,
                        password.as_str(),
                        database,
                        PoolOptions::single_connection(),
                    )
                    .map_err(|error| error.to_string())
                }
                #[cfg(not(unix))]
                {
                    _ = socket;
                    _ = user;
                    _ = password;
                    _ = database;
                    Err("Unix sockets are not supported on this platform".to_string())
                }
            }
        }
    }
}

#[derive(Default)]
struct DatabaseState {
    connection: Option<Connection>,
    backend: Option<DatabaseBackend>,
    mysql_settings: Option<MysqlConnectionSettings>,
    database: Option<String>,
    table_metadata: HashMap<String, TableMetadata>,
}

type State = Arc<Mutex<DatabaseState>>;

fn replace_database_state_if_current(
    state: &State,
    connection_generation: &AtomicU64,
    request_generation: u64,
    new_state: DatabaseState,
) -> bool {
    let mut database_state = state.lock().expect("mutex poisoned");
    if connection_generation.load(Ordering::Acquire) != request_generation {
        return false;
    }
    *database_state = new_state;
    true
}

const MYSQL_CREDENTIAL_SERVICE: &str = "nl.bplaat.SequelExplorer.mysql";

fn mysql_credential_account(
    transport: &str,
    host: &str,
    port: u16,
    socket: &str,
    user: &str,
) -> String {
    json!({
        "transport": transport,
        "host": host,
        "port": port,
        "socket": socket,
        "user": user,
    })
    .to_string()
}

fn mysql_identity_credential_account(identity: &MysqlCredentialIdentity) -> String {
    mysql_credential_account(
        &identity.transport,
        &identity.host,
        identity.port,
        &identity.socket,
        &identity.user,
    )
}

fn delete_saved_password(account: &str) -> std::result::Result<(), String> {
    match CredentialEntry::new(MYSQL_CREDENTIAL_SERVICE, account)
        .and_then(|entry| entry.delete_credential())
    {
        Ok(()) | Err(CredentialError::NoEntry) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

struct OpenMysqlRequest {
    request_id: u64,
    connection_generation: u64,
    transport: String,
    host: String,
    port: u16,
    socket: String,
    user: String,
    password: Option<Zeroizing<String>>,
    tls: bool,
    remember: bool,
    previous_connection: Option<MysqlCredentialIdentity>,
}

fn open_mysql(
    request: OpenMysqlRequest,
    state: &State,
    connection_generation: &AtomicU64,
) -> IpcMessage {
    let credential_account = mysql_credential_account(
        &request.transport,
        &request.host,
        request.port,
        &request.socket,
        &request.user,
    );
    let previous_credential_account = request
        .previous_connection
        .as_ref()
        .map(mysql_identity_credential_account)
        .filter(|account| account != &credential_account);
    let loaded_saved_password = request.password.is_none();
    let password = match request.password {
        Some(password) => Ok(password),
        None => CredentialEntry::new(MYSQL_CREDENTIAL_SERVICE, &credential_account)
            .and_then(|entry| entry.get_secret())
            .map_err(|error| format!("Failed to load saved password: {error}"))
            .and_then(|secret| {
                let secret = Zeroizing::new(secret);
                std::str::from_utf8(&secret)
                    .map(|password| Zeroizing::new(password.to_string()))
                    .map_err(|_| "Saved password is not valid UTF-8".to_string())
            }),
    };
    let password = match password {
        Ok(password) => password,
        Err(error) => {
            return IpcMessage::OpenMysqlResponse {
                request_id: request.request_id,
                ok: false,
                error: Some(error),
                credential_saved: false,
                credential_error: None,
            };
        }
    };
    let settings = if request.transport == "tcp" {
        Ok(MysqlConnectionSettings::Tcp {
            host: request.host,
            port: request.port,
            user: request.user,
            password,
            tls: request.tls,
        })
    } else if request.transport == "unix" {
        Ok(MysqlConnectionSettings::Unix {
            socket: request.socket,
            user: request.user,
            password,
        })
    } else {
        Err("Unknown MySQL transport".to_string())
    };
    let result = settings.and_then(|settings| {
        settings
            .connect(None)
            .map(|connection| (connection, settings))
    });
    let (ok, error, credential_saved, credential_error) = match result {
        Ok((connection, settings)) => {
            if connection_generation.load(Ordering::Acquire) != request.connection_generation {
                return IpcMessage::OpenMysqlResponse {
                    request_id: request.request_id,
                    ok: false,
                    error: Some("Connection request was superseded".to_string()),
                    credential_saved: false,
                    credential_error: None,
                };
            }
            let (credential_saved, mut credential_error) = if request.remember {
                if loaded_saved_password {
                    (true, None)
                } else {
                    match CredentialEntry::new(MYSQL_CREDENTIAL_SERVICE, &credential_account)
                        .and_then(|entry| entry.set_secret(settings.password().as_bytes()))
                    {
                        Ok(()) => (true, None),
                        Err(error) => (false, Some(error.to_string())),
                    }
                }
            } else {
                (false, None)
            };
            if !replace_database_state_if_current(
                state,
                connection_generation,
                request.connection_generation,
                DatabaseState {
                    connection: Some(connection),
                    backend: Some(DatabaseBackend::Mysql),
                    mysql_settings: Some(settings),
                    database: None,
                    table_metadata: HashMap::new(),
                },
            ) {
                return IpcMessage::OpenMysqlResponse {
                    request_id: request.request_id,
                    ok: false,
                    error: Some("Connection request was superseded".to_string()),
                    credential_saved: false,
                    credential_error: None,
                };
            }
            if request.remember && credential_saved {
                if let Some(previous_account) = &previous_credential_account {
                    credential_error = delete_saved_password(previous_account).err();
                }
            } else if !request.remember {
                credential_error = delete_saved_password(&credential_account).err();
                if credential_error.is_none()
                    && let Some(previous_account) = &previous_credential_account
                {
                    credential_error = delete_saved_password(previous_account).err();
                }
            }
            (true, None, credential_saved, credential_error)
        }
        Err(error) => (false, Some(error), false, None),
    };
    IpcMessage::OpenMysqlResponse {
        request_id: request.request_id,
        ok,
        error,
        credential_saved,
        credential_error,
    }
}

struct MysqlConnectionPendingGuard(Arc<AtomicBool>);

impl Drop for MysqlConnectionPendingGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

// MARK: Database helpers
fn get_connection(state: &State) -> Result<std::sync::MutexGuard<'_, DatabaseState>, Response> {
    let guard = state.lock().expect("mutex poisoned");
    if guard.connection.is_none() {
        return Err(Response::with_json(json!({ "error": "No database open" })));
    }
    Ok(guard)
}

fn quote_identifier(backend: DatabaseBackend, identifier: &str) -> String {
    match backend {
        DatabaseBackend::Sqlite => format!("\"{}\"", identifier.replace('"', "\"\"")),
        DatabaseBackend::Mysql => format!("`{}`", identifier.replace('`', "``")),
    }
}

fn quote_string(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn current_database(state: &DatabaseState) -> Result<&str, StatementError> {
    state
        .database
        .as_deref()
        .ok_or_else(|| StatementError::new("No MySQL database selected"))
}

// MARK: Databases
fn db_databases(_req: &Request, state: &State) -> Result<Response> {
    let guard = match get_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    if guard.backend != Some(DatabaseBackend::Mysql) {
        return Ok(Response::with_json(Vec::<String>::new()));
    }
    let conn = guard.connection.as_ref().expect("connection checked above");
    let databases = conn
        .query::<String>(
            "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME",
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Response::with_json(&databases))
}

// MARK: Tables
fn db_tables(_req: &Request, state: &State) -> Result<Response> {
    let guard = match get_connection(state) {
        Ok(g) => g,
        Err(e) => return Ok(e),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    let table_names = match guard.backend.expect("backend set with connection") {
        DatabaseBackend::Sqlite => conn
            .query::<String>(
                "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
                (),
            )?
            .collect::<Result<Vec<_>, _>>()?,
        DatabaseBackend::Mysql => conn
            .query::<String>(
                "SELECT TABLE_NAME FROM information_schema.TABLES
                 WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
                 ORDER BY TABLE_NAME",
                current_database(&guard)?.to_string(),
            )?
            .collect::<Result<Vec<_>, _>>()?,
    };
    Ok(Response::with_json(&table_names))
}

// MARK: Table data
#[derive(Deserialize)]
struct TableDataQuery {
    #[serde(default)]
    offset: i64,
    #[serde(default = "default_limit")]
    limit: i64,
    #[serde(default)]
    cursor: Option<String>,
}

const fn default_limit() -> i64 {
    100
}

#[derive(Clone, Serialize)]
struct CellValue {
    kind: &'static str,
    value: serde_json::Value,
}

fn cell_value(value: Value) -> CellValue {
    match value {
        Value::Null => CellValue {
            kind: "null",
            value: serde_json::Value::Null,
        },
        Value::Integer(value) => CellValue {
            kind: "integer",
            value: json!(value.to_string()),
        },
        Value::Float(value) => CellValue {
            kind: "float",
            value: json!(value),
        },
        Value::Text(value) => CellValue {
            kind: "text",
            value: json!(value),
        },
        Value::Blob(value) => CellValue {
            kind: "blob",
            value: json!(BASE64_STANDARD.encode(&value)),
        },
    }
}

#[derive(Serialize)]
struct TableData {
    columns: Vec<ColumnInfo>,
    rows: Vec<Vec<CellValue>>,
    total: Option<i64>,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
struct ColumnInfo {
    name: String,
    r#type: String,
    is_blob: bool,
    foreign_key: Option<ColumnForeignKey>,
}

#[derive(Clone, Serialize)]
struct ColumnForeignKey {
    table: String,
    column: String,
}

#[derive(Clone, Default)]
struct TableMetadata {
    declared_types: HashMap<String, String>,
    foreign_keys: HashMap<String, ColumnForeignKey>,
    key_columns: Vec<String>,
    order_columns: Vec<String>,
}

fn load_foreign_keys(
    conn: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    tables: &HashSet<String>,
) -> Result<HashMap<(String, String), ColumnForeignKey>, StatementError> {
    let mut foreign_keys = HashMap::new();
    match backend {
        DatabaseBackend::Sqlite => {
            for table in tables {
                let rows = conn.query::<(String, String, String)>(
                    format!(
                        "SELECT \"from\", \"table\", \"to\" FROM pragma_foreign_key_list({})",
                        quote_string(table)
                    ),
                    (),
                )?;
                for row in rows {
                    let (column, foreign_table, foreign_column) = row?;
                    foreign_keys.insert(
                        (table.clone(), column),
                        ColumnForeignKey {
                            table: foreign_table,
                            column: foreign_column,
                        },
                    );
                }
            }
        }
        DatabaseBackend::Mysql => {
            if tables.is_empty() {
                return Ok(foreign_keys);
            }
            let placeholders = std::iter::repeat_n("?", tables.len())
                .collect::<Vec<_>>()
                .join(", ");
            let mut rows = conn.prepare::<(String, String, String, String)>(format!(
                "SELECT TABLE_NAME, COLUMN_NAME, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME
                 FROM information_schema.KEY_COLUMN_USAGE
                 WHERE TABLE_SCHEMA = ? AND REFERENCED_TABLE_NAME IS NOT NULL
                   AND TABLE_NAME IN ({placeholders})"
            ))?;
            rows.bind_value(
                0,
                database
                    .expect("MySQL database checked before loading foreign keys")
                    .to_string(),
            )?;
            for (index, table) in tables.iter().enumerate() {
                rows.bind_value(index as i32 + 1, table.clone())?;
            }
            for row in rows {
                let (table, column, foreign_table, foreign_column) = row?;
                foreign_keys.insert(
                    (table, column),
                    ColumnForeignKey {
                        table: foreign_table,
                        column: foreign_column,
                    },
                );
            }
        }
    }
    Ok(foreign_keys)
}

// MARK: Statement processing
fn process_statement(
    stmt: &mut bsql::Statement<()>,
    table_metadata: Option<&TableMetadata>,
) -> Result<StatementOutput, StatementError> {
    let mut has_current_row = stmt.step()?.is_some();
    let column_count = stmt.column_count();
    let source_columns = (0..column_count)
        .map(|index| {
            (
                stmt.column_table_name(index),
                stmt.column_origin_name(index),
            )
        })
        .collect::<Vec<_>>();
    let columns = (0..column_count)
        .map(|index| {
            let name = stmt.column_name(index);
            let (_, origin_name) = &source_columns[index as usize];
            let declared_type = origin_name
                .as_ref()
                .and_then(|name| {
                    table_metadata.and_then(|metadata| metadata.declared_types.get(name))
                })
                .cloned()
                .or_else(|| stmt.column_declared_type(index));
            let value_type = has_current_row.then(|| stmt.column_type(index));
            ColumnInfo {
                name,
                r#type: declared_type.clone().unwrap_or_else(|| {
                    value_type.map_or_else(
                        || "UNKNOWN".to_string(),
                        |value_type| column_type_name(value_type).to_string(),
                    )
                }),
                is_blob: value_type == Some(ColumnType::Blob)
                    || value_type.is_none()
                        && declared_type.as_deref().is_some_and(declared_type_is_blob),
                foreign_key: origin_name.as_ref().and_then(|column| {
                    table_metadata
                        .and_then(|metadata| metadata.foreign_keys.get(column))
                        .cloned()
                }),
            }
        })
        .collect();
    let mut rows = Vec::new();

    while has_current_row {
        let row = (0..column_count)
            .map(|index| cell_value(stmt.column_value(index)))
            .collect();
        rows.push(row);
        has_current_row = stmt.step()?.is_some();
    }

    Ok(StatementOutput {
        columns,
        rows,
        source_columns,
    })
}

struct StatementOutput {
    columns: Vec<ColumnInfo>,
    rows: Vec<Vec<CellValue>>,
    source_columns: Vec<(Option<String>, Option<String>)>,
}

fn add_foreign_keys(
    output: &mut StatementOutput,
    conn: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
) -> Result<(), StatementError> {
    let source_tables = output
        .source_columns
        .iter()
        .filter_map(|(table, _)| table.clone())
        .collect::<HashSet<_>>();
    let foreign_keys = load_foreign_keys(conn, backend, database, &source_tables)?;
    for (column, (table, origin_name)) in output.columns.iter_mut().zip(&output.source_columns) {
        column.foreign_key = table
            .as_ref()
            .zip(origin_name.as_ref())
            .and_then(|(table, origin_name)| {
                foreign_keys.get(&(table.clone(), origin_name.clone()))
            })
            .cloned();
    }
    Ok(())
}

fn load_table_metadata(
    conn: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    table: &str,
) -> Result<TableMetadata, StatementError> {
    match backend {
        DatabaseBackend::Sqlite => load_sqlite_table_metadata(conn, table),
        DatabaseBackend::Mysql => load_mysql_table_metadata(
            conn,
            database.expect("MySQL database checked before loading table metadata"),
            table,
        ),
    }
}

fn load_sqlite_table_metadata(
    conn: &Connection,
    table: &str,
) -> Result<TableMetadata, StatementError> {
    let table_name = quote_string(table);
    let columns = conn
        .query::<(String, String, i64)>(
            format!("SELECT name, type, pk FROM pragma_table_info({table_name}) ORDER BY cid"),
            (),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut primary_key = columns
        .iter()
        .filter(|(_, _, position)| *position > 0)
        .map(|(name, _, position)| (*position, name.clone()))
        .collect::<Vec<_>>();
    primary_key.sort_by_key(|(position, _)| *position);
    let key_columns = primary_key
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<Vec<_>>();
    let order_columns = if primary_key.is_empty() {
        columns.iter().map(|(name, _, _)| name.clone()).collect()
    } else {
        primary_key.into_iter().map(|(_, name)| name).collect()
    };
    let declared_types = columns
        .into_iter()
        .map(|(name, declared_type, _)| (name, declared_type))
        .collect();
    let tables = HashSet::from([table.to_string()]);
    let foreign_keys = load_foreign_keys(conn, DatabaseBackend::Sqlite, None, &tables)?
        .into_iter()
        .map(|((_, column), foreign_key)| (column, foreign_key))
        .collect();
    Ok(TableMetadata {
        declared_types,
        foreign_keys,
        key_columns,
        order_columns,
    })
}

fn load_mysql_table_metadata(
    conn: &Connection,
    database: &str,
    table: &str,
) -> Result<TableMetadata, StatementError> {
    let columns = conn
        .query::<(String, String)>(
            "SELECT COLUMN_NAME, COLUMN_TYPE
             FROM information_schema.COLUMNS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
             ORDER BY ORDINAL_POSITION",
            (database.to_string(), table.to_string()),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let key_columns = conn
        .query::<String>(
            "SELECT COLUMN_NAME FROM information_schema.STATISTICS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND INDEX_NAME = 'PRIMARY'
             ORDER BY SEQ_IN_INDEX",
            (database.to_string(), table.to_string()),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let order_columns = if key_columns.is_empty() {
        columns.iter().map(|(name, _)| name.clone()).collect()
    } else {
        key_columns.clone()
    };
    let tables = HashSet::from([table.to_string()]);
    let foreign_keys = load_foreign_keys(conn, DatabaseBackend::Mysql, Some(database), &tables)?
        .into_iter()
        .map(|((_, column), foreign_key)| (column, foreign_key))
        .collect();
    Ok(TableMetadata {
        declared_types: columns.into_iter().collect(),
        foreign_keys,
        key_columns,
        order_columns,
    })
}

const fn column_type_name(column_type: ColumnType) -> &'static str {
    match column_type {
        ColumnType::Null => "NULL",
        ColumnType::Integer => "INTEGER",
        ColumnType::Float => "FLOAT",
        ColumnType::Text => "TEXT",
        ColumnType::Blob => "BLOB",
    }
}

fn declared_type_is_blob(declared_type: &str) -> bool {
    let declared_type = declared_type.to_ascii_uppercase();
    let base_type = declared_type
        .split_once('(')
        .map_or(declared_type.as_str(), |(base_type, _)| base_type)
        .trim();
    declared_type.contains("BLOB")
        || declared_type.contains("BINARY")
        || matches!(
            base_type,
            "BIT"
                | "GEOMETRY"
                | "POINT"
                | "LINESTRING"
                | "POLYGON"
                | "MULTIPOINT"
                | "MULTILINESTRING"
                | "MULTIPOLYGON"
                | "GEOMETRYCOLLECTION"
        )
}

#[derive(Deserialize)]
struct CursorValue {
    kind: String,
    value: serde_json::Value,
}

fn parse_cursor(cursor: &str) -> Result<Vec<Value>, StatementError> {
    let values = serde_json::from_str::<Vec<CursorValue>>(cursor)
        .map_err(|_| StatementError::new("Invalid table cursor"))?;
    values
        .into_iter()
        .map(|cell| match cell.kind.as_str() {
            "null" => Ok(Value::Null),
            "integer" => cell
                .value
                .as_str()
                .and_then(|value| value.parse().ok())
                .map(Value::Integer)
                .ok_or_else(|| StatementError::new("Invalid integer table cursor")),
            "float" => cell
                .value
                .as_f64()
                .map(Value::Float)
                .ok_or_else(|| StatementError::new("Invalid float table cursor")),
            "text" => cell
                .value
                .as_str()
                .map(|value| Value::Text(value.to_string()))
                .ok_or_else(|| StatementError::new("Invalid text table cursor")),
            "blob" => cell
                .value
                .as_str()
                .ok_or_else(|| StatementError::new("Invalid blob table cursor"))
                .and_then(|value| {
                    BASE64_STANDARD
                        .decode(value)
                        .map(Value::Blob)
                        .map_err(|_| StatementError::new("Invalid blob table cursor"))
                }),
            _ => Err(StatementError::new("Invalid table cursor value type")),
        })
        .collect()
}

fn next_cursor(
    columns: &[ColumnInfo],
    rows: &[Vec<CellValue>],
    key_columns: &[String],
) -> Option<String> {
    let row = rows.last()?;
    let values = key_columns
        .iter()
        .map(|key_column| {
            columns
                .iter()
                .position(|column| column.name == *key_column)
                .and_then(|index| row.get(index))
                .cloned()
        })
        .collect::<Option<Vec<_>>>()?;
    serde_json::to_string(&values).ok()
}

fn db_table_data(req: &Request, state: &State) -> Result<Response> {
    let name = req.params.get("name").expect("Should be some");

    let query = match req.url.query() {
        Some(q) => match serde_urlencoded::from_str::<TableDataQuery>(q) {
            Ok(query) => query,
            Err(_) => {
                return Ok(Response::with_json(
                    json!({ "error": "Invalid query parameters" }),
                ));
            }
        },
        None => TableDataQuery {
            offset: 0,
            limit: 100,
            cursor: None,
        },
    };
    if query.offset < 0 || !(1..=1_000).contains(&query.limit) {
        return Ok(Response::with_json(
            json!({ "error": "Offset or limit is out of range" }),
        ));
    }

    let mut guard = match get_connection(state) {
        Ok(g) => g,
        Err(e) => return Ok(e),
    };
    let conn = guard
        .connection
        .as_ref()
        .expect("connection checked above")
        .clone();
    let backend = guard.backend.expect("backend set with connection");
    let database = if backend == DatabaseBackend::Mysql {
        Some(current_database(&guard)?.to_string())
    } else {
        None
    };
    let metadata = match guard.table_metadata.get(name) {
        Some(metadata) => metadata.clone(),
        None => {
            let metadata = load_table_metadata(&conn, backend, database.as_deref(), name)?;
            guard
                .table_metadata
                .insert(name.to_string(), metadata.clone());
            metadata
        }
    };
    let table = quote_identifier(backend, name);
    let total = (query.offset == 0)
        .then(|| conn.query_some::<i64>(format!("SELECT COUNT(*) FROM {table}"), ()))
        .transpose()?;
    let order_by = metadata
        .order_columns
        .iter()
        .map(|column_name| {
            let column = quote_identifier(backend, column_name);
            if backend == DatabaseBackend::Mysql
                && metadata
                    .declared_types
                    .get(column_name)
                    .is_some_and(|declared_type| declared_type_is_blob(declared_type))
            {
                format!("HEX({column})")
            } else {
                column
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let cursor = query.cursor.as_deref().map(parse_cursor).transpose()?;
    let use_cursor = !metadata.key_columns.is_empty() && cursor.is_some();
    let mut stmt = if use_cursor {
        let key_columns = metadata
            .key_columns
            .iter()
            .map(|column| quote_identifier(backend, column))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = std::iter::repeat_n("?", metadata.key_columns.len())
            .collect::<Vec<_>>()
            .join(", ");
        conn.prepare::<()>(format!(
            "SELECT * FROM {table} WHERE ({key_columns}) > ({placeholders}) \
             ORDER BY {order_by} LIMIT ?"
        ))?
    } else {
        conn.prepare::<()>(format!(
            "SELECT * FROM {table} ORDER BY {order_by} LIMIT ? OFFSET ?"
        ))?
    };
    if use_cursor {
        let cursor = cursor.expect("cursor checked above");
        if cursor.len() != metadata.key_columns.len() {
            return Ok(Response::with_json(
                json!({ "error": "Invalid table cursor" }),
            ));
        }
        for (index, value) in cursor.into_iter().enumerate() {
            stmt.bind_value(index as i32, value)?;
        }
        stmt.bind_value(metadata.key_columns.len() as i32, query.limit)?;
    } else {
        stmt.bind_value(0, query.limit)?;
        stmt.bind_value(1, query.offset)?;
    }

    let StatementOutput { columns, rows, .. } = process_statement(&mut stmt, Some(&metadata))?;
    let next_cursor = (!metadata.key_columns.is_empty())
        .then(|| next_cursor(&columns, &rows, &metadata.key_columns))
        .flatten();
    Ok(Response::with_json(&TableData {
        columns,
        rows,
        total,
        next_cursor,
    }))
}

// MARK: Table schema
#[derive(Serialize)]
struct TableSchema {
    sql: String,
}

fn db_table_schema(req: &Request, state: &State) -> Result<Response> {
    let name = req.params.get("name").expect("Should be some");

    let guard = match get_connection(state) {
        Ok(g) => g,
        Err(e) => return Ok(e),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    let backend = guard.backend.expect("backend set with connection");
    let sql = match backend {
        DatabaseBackend::Sqlite => conn
            .query::<String>(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
                name.to_string(),
            )?
            .next()
            .transpose()?,
        DatabaseBackend::Mysql => conn
            .query::<(String, String)>(
                format!("SHOW CREATE TABLE {}", quote_identifier(backend, name)),
                (),
            )?
            .next()
            .transpose()?
            .map(|(_, sql)| sql),
    };

    match sql {
        Some(sql) => {
            let sql = sql.replace("   ", " ").replace("\n    )", "\n)");
            Ok(Response::with_json(&TableSchema { sql }))
        }
        None => Ok(Response::with_json(json!({ "error": "Table not found" }))),
    }
}

// MARK: Custom query
#[derive(Deserialize)]
struct QueryBody {
    sql: String,
}

#[derive(Serialize)]
struct QueryResult {
    columns: Vec<ColumnInfo>,
    rows: Vec<Vec<CellValue>>,
    truncated: bool,
}

const CUSTOM_QUERY_ROW_LIMIT: usize = 10_000;

fn bounded_select(
    conn: &Connection,
    backend: DatabaseBackend,
    sql: &str,
) -> Result<String, StatementError> {
    let alias = quote_identifier(backend, "sequel_explorer_result");
    let limit = CUSTOM_QUERY_ROW_LIMIT + 1;
    if backend == DatabaseBackend::Sqlite {
        return Ok(format!("SELECT * FROM ({sql}) AS {alias} LIMIT {limit}"));
    }

    let statement = conn.prepare::<()>(sql)?;
    let output_names = (0..statement.column_count())
        .map(|index| statement.column_name(index))
        .collect::<Vec<_>>();
    drop(statement);
    Ok(mysql_bounded_select(sql, &output_names, limit))
}

fn mysql_bounded_select(sql: &str, output_names: &[String], limit: usize) -> String {
    let alias = quote_identifier(DatabaseBackend::Mysql, "sequel_explorer_result");
    let internal_names = (0..output_names.len())
        .map(|index| quote_identifier(DatabaseBackend::Mysql, &format!("column_{index}")))
        .collect::<Vec<_>>();
    let projection = internal_names
        .iter()
        .zip(output_names)
        .map(|(internal_name, output_name)| {
            format!(
                "{alias}.{internal_name} AS {}",
                quote_identifier(DatabaseBackend::Mysql, output_name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let derived_columns = internal_names.join(", ");
    format!("SELECT {projection} FROM ({sql}) AS {alias} ({derived_columns}) LIMIT {limit}")
}

fn execute_custom_query(
    conn: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    sql: &str,
) -> Result<QueryResult, StatementError> {
    let mut stmt = conn.prepare::<()>(bounded_select(conn, backend, sql)?)?;
    let mut output = process_statement(&mut stmt, None)?;
    drop(stmt);
    add_foreign_keys(&mut output, conn, backend, database)?;
    let truncated = output.rows.len() > CUSTOM_QUERY_ROW_LIMIT;
    output.rows.truncate(CUSTOM_QUERY_ROW_LIMIT);
    Ok(QueryResult {
        columns: output.columns,
        rows: output.rows,
        truncated,
    })
}

fn db_query(req: &Request, state: &State) -> Result<Response> {
    let body: QueryBody = match serde_json::from_slice(req.body.as_deref().unwrap_or(&[])) {
        Ok(b) => b,
        Err(e) => return Ok(Response::with_json(json!({ "error": e.to_string() }))),
    };

    if let Err(error) = validate_read_only_query(&body.sql) {
        return Ok(Response::with_json(json!({ "error": error })));
    }

    let guard = match get_connection(state) {
        Ok(g) => g,
        Err(e) => return Ok(e),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    let backend = guard.backend.expect("backend set with connection");
    let database = guard.database.as_deref();

    let result = execute_custom_query(conn, backend, database, &body.sql);

    match result {
        Ok(result) => Ok(Response::with_json(&result)),
        Err(e) => Ok(Response::with_json(json!({ "error": e.to_string() }))),
    }
}

fn validate_read_only_query(sql: &str) -> Result<(), &'static str> {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        SingleQuote,
        DoubleQuote,
        Backtick,
        LineComment,
        BlockComment,
    }

    let bytes = sql.as_bytes();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut state = State::Normal;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            State::Normal if byte.is_ascii_alphanumeric() || byte == b'_' => {
                word.push(char::from(byte));
            }
            State::Normal => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                match byte {
                    b';' => {
                        return Err("Only one SELECT statement without a semicolon is allowed");
                    }
                    b'\'' => state = State::SingleQuote,
                    b'"' => state = State::DoubleQuote,
                    b'`' => state = State::Backtick,
                    b'#' => state = State::LineComment,
                    b'-' if bytes.get(index + 1) == Some(&b'-') => {
                        state = State::LineComment;
                        index += 1;
                    }
                    b'/' if bytes.get(index + 1) == Some(&b'*') => {
                        if bytes.get(index + 2) == Some(&b'!') {
                            return Err("MySQL executable comments are not allowed");
                        }
                        state = State::BlockComment;
                        index += 1;
                    }
                    _ => {}
                }
            }
            State::SingleQuote if byte == b'\\' => index += 1,
            State::SingleQuote if byte == b'\'' => {
                if bytes.get(index + 1) == Some(&b'\'') {
                    index += 1;
                } else {
                    state = State::Normal;
                }
            }
            State::DoubleQuote if byte == b'"' => {
                if bytes.get(index + 1) == Some(&b'"') {
                    index += 1;
                } else {
                    state = State::Normal;
                }
            }
            State::Backtick if byte == b'`' => {
                if bytes.get(index + 1) == Some(&b'`') {
                    index += 1;
                } else {
                    state = State::Normal;
                }
            }
            State::LineComment if byte == b'\n' => state = State::Normal,
            State::BlockComment if byte == b'*' && bytes.get(index + 1) == Some(&b'/') => {
                state = State::Normal;
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }
    if !word.is_empty() {
        words.push(word);
    }

    if words.is_empty() {
        return Err("Enter a SELECT query");
    }
    if !words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("SELECT"))
    {
        return Err("Only SELECT queries are allowed");
    }
    if words.iter().any(|word| word.eq_ignore_ascii_case("INTO")) {
        return Err("SELECT INTO is not allowed in read-only mode");
    }
    if words.windows(2).any(|words| {
        words[0].eq_ignore_ascii_case("FOR")
            && (words[1].eq_ignore_ascii_case("UPDATE") || words[1].eq_ignore_ascii_case("SHARE"))
    }) {
        return Err("Locking SELECT queries are not allowed in read-only mode");
    }
    Ok(())
}

// MARK: Main

pub(crate) enum AppEvent {
    Webview(bwindow::WindowId, WebviewEvent),
    UserEvent(String),
}

const fn background_color(theme: Theme) -> u32 {
    match theme {
        Theme::Light => 0xffffff,
        Theme::Dark => 0x222222,
    }
}

fn main() {
    let startup_path = if cfg!(any(target_os = "macos", windows)) {
        std::env::args().nth(1)
    } else {
        None
    };
    let state: State = Arc::new(Mutex::new(DatabaseState::default()));
    let event_loop = EventLoopBuilder::new()
        .with_user_event::<AppEvent>()
        .app_id("nl", "bplaat", "SequelExplorer")
        .single_instance(false)
        .menu(
            MenuBarBuilder::new()
                .menu(
                    MenuBuilder::new("File")
                        .item(
                            MenuItem::new("Connect to Database...", "open")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyO)),
                        )
                        .separator(),
                )
                .menu(
                    MenuBuilder::new("View")
                        .item(
                            MenuItem::new("Data", "showData")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::Digit1)),
                        )
                        .item(
                            MenuItem::new("Schema", "showSchema")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::Digit2)),
                        ),
                )
                .menu(
                    MenuBuilder::new("Query")
                        .item(
                            MenuItem::new("Run Query", "runQuery")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::Enter)),
                        )
                        .item(
                            MenuItem::new("Clear Query", "clearQuery")
                                .accelerator(Accelerator::new(Modifiers::PRIMARY, KeyCode::KeyK)),
                        ),
                ),
        )
        .build();

    let router = RouterBuilder::<State>::with(Arc::clone(&state))
        .get("/api/databases", db_databases)
        .get("/api/tables", db_tables)
        .get("/api/table/:name/data", db_table_data)
        .get("/api/table/:name/schema", db_table_schema)
        .post("/api/query", db_query)
        .build();
    let event_loop_proxy = Arc::new(event_loop.create_proxy());
    let mysql_connection_pending = Arc::new(AtomicBool::new(false));
    let connection_generation = Arc::new(AtomicU64::new(0));

    #[allow(unused_mut)]
    let mut window_builder = WindowBuilder::new()
        .title("Sequel Explorer")
        .size(LogicalSize::new(1200.0, 768.0))
        .min_size(LogicalSize::new(800.0, 480.0))
        .background_color(background_color(event_loop.theme()))
        .center()
        .remember_window_state()
        .allow_file_drop(true);
    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder.macos_titlebar_style(bwindow::MacosTitlebarStyle::Hidden);
    }
    let mut window = window_builder.build();

    let mut webview = WebviewBuilder::new(&window)
        .on_event(event_loop.create_proxy(), AppEvent::Webview)
        .load_rust_embed_with_custom_handler::<WebAssets>(move |req| {
            let res = router.handle(req);
            if res.status != Status::NotFound {
                Some(res)
            } else {
                None
            }
        })
        .build();

    #[cfg(target_os = "macos")]
    webview.add_user_script(
        format!(
            "document.documentElement.style.setProperty('--macos-titlebar-height', '{}px');",
            window.macos_titlebar_size().height
        ),
        bwebview::InjectionTime::DocumentStart,
    );

    let mut page_ready = false;
    let mut pending_menu_action: Option<String> = None;
    let mut pending_open_path = startup_path;
    let mut initial_restore_sent = false;
    event_loop.run(move |event| match event {
        Event::Window(_, bwindow::WindowEvent::ThemeChanged(theme)) => {
            window.set_background_color(background_color(theme));
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageLoadStart)) => {
            page_ready = false
        }
        Event::OpenFiles(paths) => {
            if let Some(path) = paths.first() {
                let path = path.to_string_lossy().into_owned();
                if page_ready {
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::OpenFile { path })
                            .expect("Failed to serialize open file message"),
                    );
                } else {
                    pending_open_path = Some(path);
                }
            }
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::PageTitleChange(title))) => {
            window.set_title(title)
        }
        Event::Action(action) => {
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::MenuAction { action })
                        .expect("Failed to serialize menu action"),
                );
            } else {
                pending_menu_action = Some(action);
            }
        }
        Event::Window(_, bwindow::WindowEvent::DroppedFile(path)) => {
            let path = path.to_string_lossy().into_owned();
            if page_ready {
                webview.send_ipc_message(
                    serde_json::to_string(&IpcMessage::OpenFile { path })
                        .expect("Failed to serialize open file message"),
                );
            } else {
                pending_open_path = Some(path);
            }
        }
        #[cfg(target_os = "macos")]
        Event::Window(_, bwindow::WindowEvent::MacosFullscreenChange(is_fullscreen)) => {
            if is_fullscreen {
                webview.evaluate_script("document.body.classList.add('is-fullscreen');");
            } else {
                webview.evaluate_script("document.body.classList.remove('is-fullscreen');");
            }
        }
        Event::UserEvent(AppEvent::Webview(_window_id, WebviewEvent::MessageReceive(message))) => {
            let message = Zeroizing::new(message);
            let message = match serde_json::from_str(&message) {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("Ignoring invalid IPC message: {error}");
                    return;
                }
            };
            match message {
                IpcMessage::Ready => {
                    page_ready = true;
                    if let Some(path) = pending_open_path.take() {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::OpenFile { path })
                                .expect("Failed to serialize open file message"),
                        );
                    } else if !initial_restore_sent {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::RestoreLastFile)
                                .expect("Failed to serialize restore last file message"),
                        );
                    }
                    initial_restore_sent = true;
                    if let Some(action) = pending_menu_action.take() {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::MenuAction { action })
                                .expect("Failed to serialize menu action"),
                        );
                    }
                }
                IpcMessage::RestoreLastFile => {}
                IpcMessage::OpenFileDialog { request_id } => {
                    let path = FileDialog::new()
                        .parent(&window)
                        .title("Open SQLite Database")
                        .add_filter("SQLite databases", &["db", "sqlite", "sqlite3"])
                        .pick_file()
                        .map(|p| p.to_string_lossy().into_owned());
                    let response = IpcMessage::OpenFileDialogResponse { request_id, path };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::OpenDatabase { request_id, path } => {
                    let request_generation =
                        connection_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    let result = Connection::open_sqlite(
                        &path,
                        SqliteMode::ReadOnly,
                        PoolOptions::single_connection(),
                    );
                    let (ok, error) = match result {
                        Ok(conn) => {
                            if replace_database_state_if_current(
                                &state,
                                &connection_generation,
                                request_generation,
                                DatabaseState {
                                    connection: Some(conn),
                                    backend: Some(DatabaseBackend::Sqlite),
                                    mysql_settings: None,
                                    database: None,
                                    table_metadata: HashMap::new(),
                                },
                            ) {
                                (true, None)
                            } else {
                                (false, Some("Connection request was superseded".to_string()))
                            }
                        }
                        Err(e) => (false, Some(e.to_string())),
                    };
                    let response = IpcMessage::OpenDatabaseResponse {
                        request_id,
                        ok,
                        error,
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&response).expect("Failed to serialize response"),
                    );
                }
                IpcMessage::OpenMysql {
                    request_id,
                    transport,
                    host,
                    port,
                    socket,
                    user,
                    password,
                    tls,
                    remember,
                    previous_connection,
                } => {
                    if mysql_connection_pending.swap(true, Ordering::AcqRel) {
                        webview.send_ipc_message(
                            serde_json::to_string(&IpcMessage::OpenMysqlResponse {
                                request_id,
                                ok: false,
                                error: Some(
                                    "A MySQL connection is already being opened".to_string(),
                                ),
                                credential_saved: false,
                                credential_error: None,
                            })
                            .expect("Failed to serialize response"),
                        );
                        return;
                    }
                    let request = OpenMysqlRequest {
                        request_id,
                        connection_generation: connection_generation.fetch_add(1, Ordering::AcqRel)
                            + 1,
                        transport,
                        host,
                        port,
                        socket,
                        user,
                        password,
                        tls,
                        remember,
                        previous_connection,
                    };
                    let state = Arc::clone(&state);
                    let event_loop_proxy = Arc::clone(&event_loop_proxy);
                    let mysql_connection_pending = Arc::clone(&mysql_connection_pending);
                    let connection_generation = Arc::clone(&connection_generation);
                    std::thread::Builder::new()
                        .name("mysql-connection".to_string())
                        .spawn(move || {
                            let _pending_guard =
                                MysqlConnectionPendingGuard(mysql_connection_pending);
                            let request_id = request.request_id;
                            let response =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    open_mysql(request, &state, &connection_generation)
                                }))
                                .unwrap_or_else(|_| {
                                    IpcMessage::OpenMysqlResponse {
                                        request_id,
                                        ok: false,
                                        error: Some("Failed to open MySQL connection".to_string()),
                                        credential_saved: false,
                                        credential_error: None,
                                    }
                                });
                            let _ = event_loop_proxy.send_user_event(AppEvent::UserEvent(
                                serde_json::to_string(&response)
                                    .expect("Failed to serialize MySQL response"),
                            ));
                        })
                        .expect("Failed to spawn MySQL connection thread");
                }
                IpcMessage::SelectMysqlDatabase {
                    request_id,
                    database,
                } => {
                    let request_generation =
                        connection_generation.fetch_add(1, Ordering::AcqRel) + 1;
                    let settings = state.lock().expect("mutex poisoned").mysql_settings.clone();
                    let result = settings
                        .ok_or_else(|| "No MySQL connection open".to_string())
                        .and_then(|settings| {
                            settings
                                .connect(Some(&database))
                                .map(|connection| (connection, settings))
                        });
                    let (ok, error) = match result {
                        Ok((connection, settings)) => {
                            if replace_database_state_if_current(
                                &state,
                                &connection_generation,
                                request_generation,
                                DatabaseState {
                                    connection: Some(connection),
                                    backend: Some(DatabaseBackend::Mysql),
                                    mysql_settings: Some(settings),
                                    database: Some(database),
                                    table_metadata: HashMap::new(),
                                },
                            ) {
                                (true, None)
                            } else {
                                (false, Some("Connection request was superseded".to_string()))
                            }
                        }
                        Err(error) => (false, Some(error)),
                    };
                    webview.send_ipc_message(
                        serde_json::to_string(&IpcMessage::SelectMysqlDatabaseResponse {
                            request_id,
                            ok,
                            error,
                        })
                        .expect("Failed to serialize response"),
                    );
                }
                _ => {}
            }
        }
        Event::UserEvent(AppEvent::UserEvent(message)) => webview.send_ipc_message(message),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_account_identifies_connection() {
        let account = mysql_credential_account("tcp", "localhost", 3306, "", "root");
        assert_eq!(
            account,
            mysql_credential_account("tcp", "localhost", 3306, "", "root")
        );
        assert_ne!(
            account,
            mysql_credential_account("tcp", "example.com", 3306, "", "root")
        );
        assert_ne!(
            account,
            mysql_credential_account("tcp", "localhost", 3306, "", "other")
        );
    }

    #[test]
    fn stale_connection_state_is_not_committed() {
        let state = Arc::new(Mutex::new(DatabaseState::default()));
        let generation = AtomicU64::new(2);

        let committed = replace_database_state_if_current(
            &state,
            &generation,
            1,
            DatabaseState {
                backend: Some(DatabaseBackend::Mysql),
                ..DatabaseState::default()
            },
        );

        assert!(!committed);
        assert!(state.lock().unwrap().backend.is_none());
    }

    #[test]
    fn pending_guard_resets_flag_when_unwinding() {
        let pending = Arc::new(AtomicBool::new(true));
        let result = std::panic::catch_unwind({
            let pending = Arc::clone(&pending);
            move || {
                let _guard = MysqlConnectionPendingGuard(pending);
                panic!("worker failed");
            }
        });

        assert!(result.is_err());
        assert!(!pending.load(Ordering::Acquire));
    }
}
