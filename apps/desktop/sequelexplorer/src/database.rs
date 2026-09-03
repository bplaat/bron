/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use bsql::{ColumnType, Connection, MysqlTransport, PoolOptions, StatementError, Value};
use keyring::{Entry as CredentialEntry, Error as CredentialError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use small_http::{Request, Response};
use zeroize::Zeroizing;

use crate::ipc::{IpcMessage, MysqlCredentialIdentity};

// MARK: State
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatabaseBackend {
    Sqlite,
    Mysql,
}

#[derive(Clone)]
pub(crate) enum MysqlConnectionSettings {
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

    pub(crate) fn connect(&self, database: Option<&str>) -> Result<Connection, String> {
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
pub(crate) struct DatabaseState {
    connection: Option<Connection>,
    backend: Option<DatabaseBackend>,
    mysql_settings: Option<MysqlConnectionSettings>,
    database: Option<String>,
    table_metadata: HashMap<String, TableMetadata>,
}

impl DatabaseState {
    pub(crate) fn sqlite(connection: Connection) -> Self {
        Self {
            connection: Some(connection),
            backend: Some(DatabaseBackend::Sqlite),
            ..Self::default()
        }
    }

    pub(crate) fn mysql(
        connection: Connection,
        settings: MysqlConnectionSettings,
        database: String,
    ) -> Self {
        Self {
            connection: Some(connection),
            backend: Some(DatabaseBackend::Mysql),
            mysql_settings: Some(settings),
            database: Some(database),
            table_metadata: HashMap::new(),
        }
    }

    pub(crate) fn mysql_settings(&self) -> Option<MysqlConnectionSettings> {
        self.mysql_settings.clone()
    }

    pub(crate) fn connection_snapshot(
        &self,
    ) -> Result<(Connection, DatabaseBackend, Option<String>), String> {
        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| "No database open".to_string())?
            .clone();
        let backend = self
            .backend
            .ok_or_else(|| "No database backend selected".to_string())?;
        Ok((connection, backend, self.database.clone()))
    }

    pub(crate) fn clear_table_metadata(&mut self) {
        self.table_metadata.clear();
    }
}

pub(crate) type State = Arc<Mutex<DatabaseState>>;

pub(crate) fn replace_database_state_if_current(
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

pub(crate) struct OpenMysqlRequest {
    pub(crate) request_id: u64,
    pub(crate) connection_generation: u64,
    pub(crate) transport: String,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) socket: String,
    pub(crate) user: String,
    pub(crate) password: Option<Zeroizing<String>>,
    pub(crate) tls: bool,
    pub(crate) remember: bool,
    pub(crate) previous_connection: Option<MysqlCredentialIdentity>,
}

pub(crate) fn open_mysql(
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

pub(crate) struct MysqlConnectionPendingGuard(pub(crate) Arc<AtomicBool>);

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

fn mysql_grant_database_pattern(database: &str) -> String {
    database
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn mysql_grant_database_name(pattern: &str) -> Option<String> {
    let mut name = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => name.push(chars.next()?),
            '%' | '_' => return None,
            _ => name.push(character),
        }
    }
    Some(name)
}

fn quote_mysql_grant_database(database: &str) -> String {
    let pattern = mysql_grant_database_pattern(database).replace('`', "``");
    format!("`{pattern}`")
}

fn mysql_account(user: &str, host: &str) -> String {
    format!("{}@{}", quote_string(user), quote_string(host))
}

fn validate_mysql_account_part(value: &str, name: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{name} is required"));
    }
    if value.contains(['\'', '\\', '\0', '\n', '\r']) {
        return Err(format!("{name} contains unsupported characters"));
    }
    Ok(())
}

fn current_database(state: &DatabaseState) -> Result<&str, StatementError> {
    state
        .database
        .as_deref()
        .ok_or_else(|| StatementError::new("No MySQL database selected"))
}

// MARK: Databases
pub(crate) fn db_databases(_req: &Request, state: &State) -> Result<Response> {
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

// MARK: MySQL users
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MysqlUser {
    user: String,
    host: String,
    databases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MysqlUserBody {
    user: String,
    host: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    databases: Vec<String>,
    old_user: Option<String>,
    old_host: Option<String>,
}

#[derive(Deserialize)]
struct MysqlUserIdentityBody {
    user: String,
    host: String,
}

const MYSQL_FULL_DATABASE_GRANT_PREDICATE: &str = "Select_priv = 'Y'
    AND Insert_priv = 'Y'
    AND Update_priv = 'Y'
    AND Delete_priv = 'Y'
    AND Create_priv = 'Y'
    AND Drop_priv = 'Y'
    AND References_priv = 'Y'
    AND Index_priv = 'Y'
    AND Alter_priv = 'Y'
    AND Create_tmp_table_priv = 'Y'
    AND Lock_tables_priv = 'Y'
    AND Create_view_priv = 'Y'
    AND Show_view_priv = 'Y'
    AND Create_routine_priv = 'Y'
    AND Alter_routine_priv = 'Y'
    AND Execute_priv = 'Y'
    AND Event_priv = 'Y'
    AND Trigger_priv = 'Y'";

fn mysql_connection(state: &State) -> Result<std::sync::MutexGuard<'_, DatabaseState>, Response> {
    let guard = get_connection(state)?;
    if guard.backend != Some(DatabaseBackend::Mysql) {
        return Err(Response::with_json(
            json!({ "error": "User management is only available for MySQL connections" }),
        ));
    }
    Ok(guard)
}

fn parse_json_body<T: for<'de> Deserialize<'de>>(req: &Request) -> Result<T, Response> {
    serde_json::from_slice(req.body.as_deref().unwrap_or(&[]))
        .map_err(|error| Response::with_json(json!({ "error": error.to_string() })))
}

fn load_mysql_users(conn: &Connection) -> Result<Vec<MysqlUser>, StatementError> {
    let accounts = conn
        .query::<(String, String)>("SELECT User, Host FROM mysql.user ORDER BY User, Host", ())?
        .collect::<Result<Vec<_>, _>>()?;
    let grants = conn
        .query::<(String, String, String)>(
            format!(
                "SELECT User, Host, Db FROM mysql.db \
                 WHERE {MYSQL_FULL_DATABASE_GRANT_PREDICATE} \
                 ORDER BY User, Host, Db"
            ),
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut databases_by_account: HashMap<(String, String), Vec<String>> = HashMap::new();
    for (user, host, database_pattern) in grants {
        let Some(database) = mysql_grant_database_name(&database_pattern) else {
            continue;
        };
        databases_by_account
            .entry((user, host))
            .or_default()
            .push(database);
    }
    Ok(accounts
        .into_iter()
        .map(|(user, host)| MysqlUser {
            databases: databases_by_account
                .remove(&(user.clone(), host.clone()))
                .unwrap_or_default(),
            user,
            host,
        })
        .collect())
}

fn validate_mysql_databases(
    conn: &Connection,
    databases: &[String],
) -> Result<Vec<String>, String> {
    let mut databases = databases.to_vec();
    databases.sort();
    databases.dedup();
    let available = conn
        .query::<String>("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA", ())
        .map_err(|error| error.to_string())?
        .collect::<Result<HashSet<_>, _>>()
        .map_err(|error| error.to_string())?;
    if let Some(database) = databases
        .iter()
        .find(|database| !available.contains(*database))
    {
        return Err(format!(
            "Database {database} does not exist or is not accessible"
        ));
    }
    Ok(databases)
}

fn set_mysql_user_databases(
    conn: &Connection,
    user: &str,
    host: &str,
    databases: &[String],
) -> Result<(), StatementError> {
    let account = mysql_account(user, host);
    let current = load_full_mysql_user_databases(conn, user, host)?;
    let selected = databases.iter().cloned().collect::<HashSet<_>>();
    for database in current.difference(&selected) {
        conn.execute(
            format!(
                "REVOKE ALL PRIVILEGES ON {}.* FROM {account}",
                quote_mysql_grant_database(database)
            ),
            (),
        )?;
    }
    for database in selected.difference(&current) {
        conn.execute(
            format!(
                "GRANT ALL PRIVILEGES ON {}.* TO {account}",
                quote_mysql_grant_database(database)
            ),
            (),
        )?;
    }
    Ok(())
}

fn load_full_mysql_user_databases(
    conn: &Connection,
    user: &str,
    host: &str,
) -> Result<HashSet<String>, StatementError> {
    conn.query::<String>(
        format!(
            "SELECT Db FROM mysql.db \
                 WHERE User = ? AND Host = ? AND {MYSQL_FULL_DATABASE_GRANT_PREDICATE} \
                 ORDER BY Db"
        ),
        (user.to_string(), host.to_string()),
    )?
    .map(|database| database.map(|pattern| mysql_grant_database_name(&pattern)))
    .filter_map(|database| database.transpose())
    .collect()
}

fn rollback_mysql_user_update(
    conn: &Connection,
    old_user: &str,
    old_host: &str,
    current_user: &str,
    current_host: &str,
    original_databases: &[String],
) -> Result<(), StatementError> {
    let mut rollback_errors = Vec::new();
    if let Err(error) =
        set_mysql_user_databases(conn, current_user, current_host, original_databases)
    {
        rollback_errors.push(format!("failed to restore database grants: {error}"));
    }
    if (old_user != current_user || old_host != current_host)
        && let Err(error) = conn.execute(
            format!(
                "RENAME USER {} TO {}",
                mysql_account(current_user, current_host),
                mysql_account(old_user, old_host)
            ),
            (),
        )
    {
        rollback_errors.push(format!("failed to restore account identity: {error}"));
    }
    if rollback_errors.is_empty() {
        Ok(())
    } else {
        Err(StatementError::new(rollback_errors.join("; ")))
    }
}

pub(crate) fn db_users(_req: &Request, state: &State) -> Result<Response> {
    let guard = match mysql_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    match load_mysql_users(conn) {
        Ok(users) => Ok(Response::with_json(&users)),
        Err(error) => Ok(Response::with_json(json!({ "error": error.to_string() }))),
    }
}

pub(crate) fn db_users_create(req: &Request, state: &State) -> Result<Response> {
    let body: MysqlUserBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    if let Err(error) = validate_mysql_account_part(&body.user, "User")
        .and_then(|()| validate_mysql_account_part(&body.host, "Host"))
    {
        return Ok(Response::with_json(json!({ "error": error })));
    }
    let guard = match mysql_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    let databases = match validate_mysql_databases(conn, &body.databases) {
        Ok(databases) => databases,
        Err(error) => return Ok(Response::with_json(json!({ "error": error }))),
    };
    let account = mysql_account(&body.user, &body.host);
    let result = conn
        .execute(
            format!("CREATE USER {account} IDENTIFIED BY ?"),
            body.password,
        )
        .and_then(|_| {
            set_mysql_user_databases(conn, &body.user, &body.host, &databases).map_err(|error| {
                match conn.execute(format!("DROP USER {account}"), ()) {
                    Ok(_) => error,
                    Err(rollback_error) => StatementError::new(format!(
                        "{error}; additionally failed to remove the partially created user: {rollback_error}"
                    )),
                }
            })
        });
    match result {
        Ok(()) => Ok(Response::with_json(json!({ "ok": true }))),
        Err(error) => Ok(Response::with_json(json!({ "error": error.to_string() }))),
    }
}

pub(crate) fn db_users_update(req: &Request, state: &State) -> Result<Response> {
    let body: MysqlUserBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let old_user = body.old_user.as_deref().unwrap_or(&body.user);
    let old_host = body.old_host.as_deref().unwrap_or(&body.host);
    let validation = validate_mysql_account_part(old_user, "Current user")
        .and_then(|()| validate_mysql_account_part(old_host, "Current host"))
        .and_then(|()| validate_mysql_account_part(&body.user, "User"))
        .and_then(|()| validate_mysql_account_part(&body.host, "Host"));
    if let Err(error) = validation {
        return Ok(Response::with_json(json!({ "error": error })));
    }
    let guard = match mysql_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    let databases = match validate_mysql_databases(conn, &body.databases) {
        Ok(databases) => databases,
        Err(error) => return Ok(Response::with_json(json!({ "error": error }))),
    };
    let old_account = mysql_account(old_user, old_host);
    let account = mysql_account(&body.user, &body.host);
    let original_databases = load_full_mysql_user_databases(conn, old_user, old_host)?
        .into_iter()
        .collect::<Vec<_>>();
    let renamed = old_account != account;
    let mut rename_applied = false;
    let result: std::result::Result<(), StatementError> = (|| {
        if renamed {
            conn.execute(format!("RENAME USER {old_account} TO {account}"), ())?;
            rename_applied = true;
        }
        set_mysql_user_databases(conn, &body.user, &body.host, &databases)?;
        if !body.password.is_empty() {
            conn.execute(
                format!("ALTER USER {account} IDENTIFIED BY ?"),
                body.password,
            )?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(Response::with_json(json!({ "ok": true }))),
        Err(error) => {
            let (current_user, current_host) = if rename_applied {
                (body.user.as_str(), body.host.as_str())
            } else {
                (old_user, old_host)
            };
            let rollback = rollback_mysql_user_update(
                conn,
                old_user,
                old_host,
                current_user,
                current_host,
                &original_databases,
            );
            let error = match rollback {
                Ok(()) => error.to_string(),
                Err(rollback_error) => {
                    format!("{error}; recovery was incomplete: {rollback_error}")
                }
            };
            Ok(Response::with_json(
                json!({ "error": error, "reload": true }),
            ))
        }
    }
}

pub(crate) fn db_users_delete(req: &Request, state: &State) -> Result<Response> {
    let body: MysqlUserIdentityBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    if let Err(error) = validate_mysql_account_part(&body.user, "User")
        .and_then(|()| validate_mysql_account_part(&body.host, "Host"))
    {
        return Ok(Response::with_json(json!({ "error": error })));
    }
    let guard = match mysql_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    match conn.execute(
        format!("DROP USER {}", mysql_account(&body.user, &body.host)),
        (),
    ) {
        Ok(_) => Ok(Response::with_json(json!({ "ok": true }))),
        Err(error) => Ok(Response::with_json(json!({ "error": error.to_string() }))),
    }
}

// MARK: Tables
pub(crate) fn db_tables(_req: &Request, state: &State) -> Result<Response> {
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
    is_primary_key: bool,
    foreign_key: Option<ColumnForeignKey>,
}

#[derive(Clone, Serialize)]
struct ColumnForeignKey {
    table: String,
    column: String,
}

#[derive(Clone, Default)]
struct TableMetadata {
    columns: Vec<String>,
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
    process_statement_limited(stmt, table_metadata, usize::MAX)
}

fn process_statement_limited(
    stmt: &mut bsql::Statement<()>,
    table_metadata: Option<&TableMetadata>,
    row_limit: usize,
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
                is_primary_key: origin_name.as_ref().is_some_and(|column| {
                    table_metadata.is_some_and(|metadata| metadata.key_columns.contains(column))
                }),
                foreign_key: origin_name.as_ref().and_then(|column| {
                    table_metadata
                        .and_then(|metadata| metadata.foreign_keys.get(column))
                        .cloned()
                }),
            }
        })
        .collect();
    let mut rows = Vec::new();

    while has_current_row && rows.len() < row_limit {
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
        .iter()
        .cloned()
        .map(|(name, declared_type, _)| (name, declared_type))
        .collect();
    let column_names = columns.into_iter().map(|(name, _, _)| name).collect();
    let tables = HashSet::from([table.to_string()]);
    let foreign_keys = load_foreign_keys(conn, DatabaseBackend::Sqlite, None, &tables)?
        .into_iter()
        .map(|((_, column), foreign_key)| (column, foreign_key))
        .collect();
    Ok(TableMetadata {
        columns: column_names,
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
    let column_names = columns.iter().map(|(name, _)| name.clone()).collect();
    let tables = HashSet::from([table.to_string()]);
    let foreign_keys = load_foreign_keys(conn, DatabaseBackend::Mysql, Some(database), &tables)?
        .into_iter()
        .map(|((_, column), foreign_key)| (column, foreign_key))
        .collect();
    Ok(TableMetadata {
        columns: column_names,
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
struct InputCellValue {
    kind: String,
    value: serde_json::Value,
}

fn input_cell_value(cell: InputCellValue) -> Result<Value, StatementError> {
    match cell.kind.as_str() {
        "null" => Ok(Value::Null),
        "integer" => cell
            .value
            .as_str()
            .and_then(|value| value.parse().ok())
            .map(Value::Integer)
            .ok_or_else(|| StatementError::new("Invalid integer cell value")),
        "float" => cell
            .value
            .as_f64()
            .map(Value::Float)
            .ok_or_else(|| StatementError::new("Invalid float cell value")),
        "text" => cell
            .value
            .as_str()
            .map(|value| Value::Text(value.to_string()))
            .ok_or_else(|| StatementError::new("Invalid text cell value")),
        "blob" => cell
            .value
            .as_str()
            .ok_or_else(|| StatementError::new("Invalid blob cell value"))
            .and_then(|value| {
                BASE64_STANDARD
                    .decode(value)
                    .map(Value::Blob)
                    .map_err(|_| StatementError::new("Invalid blob cell value"))
            }),
        _ => Err(StatementError::new("Invalid cell value type")),
    }
}

fn parse_cursor(cursor: &str) -> Result<Vec<Value>, StatementError> {
    let values = serde_json::from_str::<Vec<InputCellValue>>(cursor)
        .map_err(|_| StatementError::new("Invalid table cursor"))?;
    values.into_iter().map(input_cell_value).collect()
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

pub(crate) fn db_table_data(req: &Request, state: &State) -> Result<Response> {
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

#[derive(Deserialize)]
struct TableUpdateBody {
    column: String,
    value: InputCellValue,
    keys: HashMap<String, InputCellValue>,
}

#[derive(Deserialize)]
struct TableDeleteBody {
    keys: HashMap<String, InputCellValue>,
}

#[derive(Deserialize)]
struct TableInsertBody {
    values: HashMap<String, Option<InputCellValue>>,
}

pub(crate) fn db_table_insert(req: &Request, state: &State) -> Result<Response> {
    let name = req.params.get("name").expect("Should be some");
    let body: TableInsertBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let mut guard = match get_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
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
    if body.values.len() != metadata.columns.len()
        || metadata
            .columns
            .iter()
            .any(|column| !body.values.contains_key(column))
    {
        return Ok(Response::with_json(
            json!({ "error": "Every column must have a value, NULL, or Default" }),
        ));
    }

    let supplied_columns = metadata
        .columns
        .iter()
        .filter(|column| body.values[*column].is_some())
        .collect::<Vec<_>>();
    let table = quote_identifier(backend, name);
    let sql = if supplied_columns.is_empty() {
        if backend == DatabaseBackend::Mysql {
            format!("INSERT INTO {table} () VALUES ()")
        } else {
            format!("INSERT INTO {table} DEFAULT VALUES")
        }
    } else {
        let columns = supplied_columns
            .iter()
            .map(|column| quote_identifier(backend, column))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = std::iter::repeat_n("?", supplied_columns.len())
            .collect::<Vec<_>>()
            .join(", ");
        format!("INSERT INTO {table} ({columns}) VALUES ({placeholders})")
    };
    let mut statement = conn.prepare::<()>(sql)?;
    for (index, column) in supplied_columns.into_iter().enumerate() {
        let value = body.values[column]
            .as_ref()
            .expect("supplied columns contain values");
        statement.bind_value(
            index as i32,
            input_cell_value(InputCellValue {
                kind: value.kind.clone(),
                value: value.value.clone(),
            })?,
        )?;
    }
    statement.next().transpose()?;
    let result = statement.execution_result();
    if result.affected_rows != 1 {
        return Ok(Response::with_json(
            json!({ "error": "The row was not inserted" }),
        ));
    }
    Ok(Response::with_json(json!({ "ok": true })))
}

pub(crate) fn db_table_update(req: &Request, state: &State) -> Result<Response> {
    let name = req.params.get("name").expect("Should be some");
    let body: TableUpdateBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let mut guard = match get_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
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
    if !metadata.declared_types.contains_key(&body.column) {
        return Ok(Response::with_json(
            json!({ "error": "Column does not exist" }),
        ));
    }
    if metadata.key_columns.is_empty()
        || body.keys.len() != metadata.key_columns.len()
        || metadata
            .key_columns
            .iter()
            .any(|column| !body.keys.contains_key(column))
    {
        return Ok(Response::with_json(
            json!({ "error": "A complete primary key is required to update this row" }),
        ));
    }

    let table = quote_identifier(backend, name);
    let column = quote_identifier(backend, &body.column);
    let comparison = if backend == DatabaseBackend::Mysql {
        "<=>"
    } else {
        "IS"
    };
    let where_clause = metadata
        .key_columns
        .iter()
        .map(|column| format!("{} {comparison} ?", quote_identifier(backend, column)))
        .collect::<Vec<_>>()
        .join(" AND ");
    let mut statement = conn.prepare::<()>(format!(
        "UPDATE {table} SET {column} = ? WHERE {where_clause}"
    ))?;
    statement.bind_value(0, input_cell_value(body.value)?)?;
    let key_values = metadata
        .key_columns
        .iter()
        .map(|key_column| {
            let key = body
                .keys
                .get(key_column)
                .expect("primary key presence checked above");
            input_cell_value(InputCellValue {
                kind: key.kind.clone(),
                value: key.value.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for (index, key) in key_values.iter().cloned().enumerate() {
        statement.bind_value(index as i32 + 1, key)?;
    }
    statement.next().transpose()?;
    let result = statement.execution_result();
    if result.affected_rows == 0 {
        drop(statement);
        let mut exists =
            conn.prepare::<i64>(format!("SELECT COUNT(*) FROM {table} WHERE {where_clause}"))?;
        for (index, key) in key_values.into_iter().enumerate() {
            exists.bind_value(index as i32, key)?;
        }
        if exists.next().transpose()?.unwrap_or(0) == 0 {
            return Ok(Response::with_json(
                json!({ "error": "The row was not updated; it may have changed or been deleted" }),
            ));
        }
    }
    if result.affected_rows > 1 {
        return Ok(Response::with_json(
            json!({ "error": "The update matched more than one row" }),
        ));
    }
    Ok(Response::with_json(json!({ "ok": true })))
}

pub(crate) fn db_table_delete(req: &Request, state: &State) -> Result<Response> {
    let name = req.params.get("name").expect("Should be some");
    let body: TableDeleteBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let mut guard = match get_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
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
    if metadata.key_columns.is_empty()
        || body.keys.len() != metadata.key_columns.len()
        || metadata
            .key_columns
            .iter()
            .any(|column| !body.keys.contains_key(column))
    {
        return Ok(Response::with_json(
            json!({ "error": "A complete primary key is required to delete this row" }),
        ));
    }

    let table = quote_identifier(backend, name);
    let comparison = if backend == DatabaseBackend::Mysql {
        "<=>"
    } else {
        "IS"
    };
    let where_clause = metadata
        .key_columns
        .iter()
        .map(|column| format!("{} {comparison} ?", quote_identifier(backend, column)))
        .collect::<Vec<_>>()
        .join(" AND ");
    let mut statement = conn.prepare::<()>(format!("DELETE FROM {table} WHERE {where_clause}"))?;
    for (index, key_column) in metadata.key_columns.iter().enumerate() {
        let key = body
            .keys
            .get(key_column)
            .expect("primary key presence checked above");
        statement.bind_value(
            index as i32,
            input_cell_value(InputCellValue {
                kind: key.kind.clone(),
                value: key.value.clone(),
            })?,
        )?;
    }
    statement.next().transpose()?;
    let result = statement.execution_result();
    if result.affected_rows != 1 {
        return Ok(Response::with_json(json!({
            "error": if result.affected_rows == 0 {
                "The row no longer exists"
            } else {
                "More than one row matched"
            }
        })));
    }
    Ok(Response::with_json(json!({ "ok": true })))
}

// MARK: Custom query
#[derive(Deserialize)]
struct QueryBody {
    sql: String,
}

#[derive(Serialize)]
pub(crate) struct QueryResult {
    columns: Vec<ColumnInfo>,
    rows: Vec<Vec<CellValue>>,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum QueryResponse {
    Success(QueryResult),
    Error { error: String },
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

pub(crate) struct QueryConnection {
    connection: Connection,
    backend: DatabaseBackend,
    database: Option<String>,
}

pub(crate) fn query_connection(state: &State) -> Result<QueryConnection, String> {
    let guard = state
        .lock()
        .map_err(|_| "Database state unavailable".to_string())?;
    Ok(QueryConnection {
        connection: guard
            .connection
            .clone()
            .ok_or_else(|| "No database open".to_string())?,
        backend: guard.backend.expect("backend set with connection"),
        database: guard.database.clone(),
    })
}

pub(crate) fn run_query(connection: &QueryConnection, sql: &str) -> Result<QueryResult, String> {
    validate_read_only_query(sql).map_err(str::to_string)?;
    execute_custom_query(
        &connection.connection,
        connection.backend,
        connection.database.as_deref(),
        sql,
    )
    .map_err(|error| error.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RawQueryResult {
    columns: Vec<ColumnInfo>,
    rows: Vec<Vec<CellValue>>,
    affected_rows: u64,
    truncated: bool,
}

fn execute_raw_query(
    conn: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    sql: &str,
) -> Result<RawQueryResult, StatementError> {
    let mut statement = conn.prepare::<()>(sql)?;
    let mut output = process_statement_limited(&mut statement, None, CUSTOM_QUERY_ROW_LIMIT + 1)?;
    let affected_rows = statement.execution_result().affected_rows;
    drop(statement);
    add_foreign_keys(&mut output, conn, backend, database)?;
    let truncated = output.rows.len() > CUSTOM_QUERY_ROW_LIMIT;
    output.rows.truncate(CUSTOM_QUERY_ROW_LIMIT);
    Ok(RawQueryResult {
        columns: output.columns,
        rows: output.rows,
        affected_rows,
        truncated,
    })
}

pub(crate) fn db_raw_query(req: &Request, state: &State) -> Result<Response> {
    let body: QueryBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    if body.sql.trim().is_empty() {
        return Ok(Response::with_json(json!({ "error": "Query is required" })));
    }
    let mut guard = match get_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    let conn = guard
        .connection
        .as_ref()
        .expect("connection checked above")
        .clone();
    let backend = guard.backend.expect("backend set with connection");
    let database = guard.database.clone();
    match execute_raw_query(&conn, backend, database.as_deref(), &body.sql) {
        Ok(result) => {
            guard.table_metadata.clear();
            Ok(Response::with_json(&result))
        }
        Err(error) => Ok(Response::with_json(json!({ "error": error.to_string() }))),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn query_state() -> State {
        let connection = Connection::open_sqlite_memory().expect("open test database");
        connection
            .execute_script(
                "CREATE TABLE parents (id INTEGER PRIMARY KEY);
                 CREATE TABLE children (parent_id INTEGER REFERENCES parents(id), payload BLOB);
                 INSERT INTO parents VALUES (1);
                 INSERT INTO children VALUES (1, X'00FF80');",
            )
            .expect("create query fixtures");
        Arc::new(Mutex::new(DatabaseState::sqlite(connection)))
    }

    #[test]
    fn query_ipc_preserves_request_id_rows_and_metadata() {
        let request: IpcMessage = serde_json::from_value(json!({
            "type": "runQuery",
            "requestId": 42,
            "sql": "SELECT parent_id, payload FROM children",
        }))
        .expect("deserialize query request");
        let IpcMessage::RunQuery { request_id, sql } = request else {
            panic!("expected query request");
        };
        let state = query_state();
        let connection = query_connection(&state).expect("capture query connection");
        let result = std::thread::spawn(move || run_query(&connection, &sql))
            .join()
            .expect("query worker completed")
            .expect("run query");
        let response = serde_json::to_value(IpcMessage::RunQueryResponse {
            request_id,
            result: QueryResponse::Success(result),
        })
        .expect("serialize query response");

        assert_eq!(response["type"], "runQueryResponse");
        assert_eq!(response["requestId"], 42);
        assert_eq!(response["columns"][0]["foreign_key"]["table"], "parents");
        assert_eq!(response["columns"][1]["is_blob"], true);
        assert_eq!(response["rows"][0][0]["value"], "1");
        assert_eq!(response["rows"][0][1]["value"], "AP+A");
        assert_eq!(response["truncated"], false);
        assert!(response.get("error").is_none());
    }

    #[test]
    fn query_rejects_writes_and_reports_database_errors() {
        let state = query_state();
        let connection = query_connection(&state).expect("capture query connection");
        assert!(run_query(&connection, "DELETE FROM children").is_err());
        assert!(run_query(&connection, "SELECT 1; DELETE FROM children").is_err());
        assert!(run_query(&connection, "SELECT * FROM missing_table").is_err());
        let result = run_query(&connection, "SELECT * FROM children").expect("rows preserved");
        assert_eq!(result.rows.len(), 1);

        let state = Arc::new(Mutex::new(DatabaseState::default()));
        let error = query_connection(&state).err().expect("no database");
        let response = serde_json::to_value(IpcMessage::RunQueryResponse {
            request_id: 43,
            result: QueryResponse::Error { error },
        })
        .expect("serialize query error");
        assert_eq!(
            response,
            json!({
                "type": "runQueryResponse",
                "requestId": 43,
                "error": "No database open",
            })
        );
    }

    #[test]
    fn query_preserves_row_limit_and_truncation() {
        let sql = format!(
            "SELECT n FROM (
                WITH RECURSIVE numbers(n) AS (
                    VALUES(1) UNION ALL SELECT n + 1 FROM numbers WHERE n <= {CUSTOM_QUERY_ROW_LIMIT}
                ) SELECT n FROM numbers
            )"
        );
        let connection = query_connection(&query_state()).expect("capture query connection");
        let result = run_query(&connection, &sql).expect("run bounded query");
        assert_eq!(result.rows.len(), CUSTOM_QUERY_ROW_LIMIT);
        assert!(result.truncated);
    }

    #[test]
    fn queued_query_keeps_its_original_connection() {
        let state = query_state();
        let connection = query_connection(&state).expect("capture query connection");
        *state.lock().expect("lock database state") = DatabaseState::sqlite(
            Connection::open_sqlite_memory().expect("open replacement database"),
        );

        let result = std::thread::spawn(move || run_query(&connection, "SELECT * FROM children"))
            .join()
            .expect("query worker completed")
            .expect("query original database");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0].value, "1");
    }

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

    #[test]
    fn mysql_account_quotes_identity_parts() {
        assert_eq!(mysql_account("reader", "%"), "'reader'@'%'");
        assert_eq!(mysql_account("reader", "localhost"), "'reader'@'localhost'");
    }

    #[test]
    fn mysql_account_parts_reject_sql_delimiters() {
        assert!(validate_mysql_account_part("reader", "User").is_ok());
        assert!(validate_mysql_account_part("", "User").is_err());
        assert!(validate_mysql_account_part("bad'user", "User").is_err());
        assert!(validate_mysql_account_part("bad\\user", "User").is_err());
    }

    #[test]
    fn mysql_grant_database_escapes_wildcards() {
        assert_eq!(
            quote_mysql_grant_database("customer_data%"),
            "`customer\\_data\\%`"
        );
        assert_eq!(quote_mysql_grant_database("path\\name"), "`path\\\\name`");
        assert_eq!(quote_mysql_grant_database("tick`name"), "`tick``name`");
        assert_eq!(
            mysql_grant_database_name(&mysql_grant_database_pattern("customer_data%")),
            Some("customer_data%".to_string())
        );
        assert_eq!(mysql_grant_database_name("customer_data"), None);
    }

    #[test]
    fn table_update_uses_primary_key() {
        let connection = Connection::open_sqlite_memory().unwrap();
        connection
            .execute_script(
                "CREATE TABLE persons (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\
                 INSERT INTO persons (id, name) VALUES (1, 'Before');",
            )
            .unwrap();
        let state = Arc::new(Mutex::new(DatabaseState {
            connection: Some(connection),
            backend: Some(DatabaseBackend::Sqlite),
            ..DatabaseState::default()
        }));
        let mut request = Request::default();
        request
            .params
            .insert("name".to_string(), "persons".to_string());
        request.body = Some(
            serde_json::to_vec(&json!({
                "column": "name",
                "value": { "kind": "text", "value": "After" },
                "keys": { "id": { "kind": "integer", "value": "1" } }
            }))
            .unwrap(),
        );

        let response = db_table_update(&request, &state).unwrap();

        assert_eq!(response.body, br#"{"ok":true}"#);
        let guard = state.lock().unwrap();
        let name = guard
            .connection
            .as_ref()
            .unwrap()
            .query_some::<String>("SELECT name FROM persons WHERE id = 1", ())
            .unwrap();
        assert_eq!(name, "After");
    }

    #[test]
    fn table_delete_binds_primary_key_values() {
        let connection = Connection::open_sqlite_memory().unwrap();
        connection
            .execute(
                "CREATE TABLE items (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
                (),
            )
            .unwrap();
        let key = "backslash\\'quote";
        connection
            .execute(
                "INSERT INTO items (id, name) VALUES (?, ?)",
                (key.to_string(), "Delete me".to_string()),
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO items (id, name) VALUES (?, ?)",
                ("keep".to_string(), "Keep me".to_string()),
            )
            .unwrap();
        let state = Arc::new(Mutex::new(DatabaseState {
            connection: Some(connection),
            backend: Some(DatabaseBackend::Sqlite),
            ..DatabaseState::default()
        }));
        let mut request = Request::default();
        request
            .params
            .insert("name".to_string(), "items".to_string());
        request.body = Some(
            serde_json::to_vec(&json!({
                "keys": { "id": { "kind": "text", "value": key } }
            }))
            .unwrap(),
        );

        let response = db_table_delete(&request, &state).unwrap();

        assert_eq!(response.body, br#"{"ok":true}"#);
        let guard = state.lock().unwrap();
        let remaining = guard
            .connection
            .as_ref()
            .unwrap()
            .query_some::<i64>("SELECT COUNT(*) FROM items", ())
            .unwrap();
        assert_eq!(remaining, 1);
    }

    #[test]
    fn table_insert_accepts_values_and_column_defaults() {
        let connection = Connection::open_sqlite_memory().unwrap();
        connection
            .execute_script(
                "CREATE TABLE items (\
                    id INTEGER PRIMARY KEY AUTOINCREMENT,\
                    name TEXT NOT NULL,\
                    enabled INTEGER NOT NULL DEFAULT 1\
                );",
            )
            .unwrap();
        let state = Arc::new(Mutex::new(DatabaseState {
            connection: Some(connection),
            backend: Some(DatabaseBackend::Sqlite),
            ..DatabaseState::default()
        }));
        let mut request = Request::default();
        request
            .params
            .insert("name".to_string(), "items".to_string());
        request.body = Some(
            serde_json::to_vec(&json!({
                "values": {
                    "id": null,
                    "name": { "kind": "text", "value": "Created" },
                    "enabled": null
                }
            }))
            .unwrap(),
        );

        let response = db_table_insert(&request, &state).unwrap();

        assert_eq!(response.body, br#"{"ok":true}"#);
        let guard = state.lock().unwrap();
        let row = guard
            .connection
            .as_ref()
            .unwrap()
            .query_some::<(String, i64)>("SELECT name, enabled FROM items", ())
            .unwrap();
        assert_eq!(row, ("Created".to_string(), 1));
    }

    #[test]
    fn raw_query_executes_writes_and_returns_results() {
        let connection = Connection::open_sqlite_memory().unwrap();
        connection
            .execute_script(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\
                 INSERT INTO items (id, name) VALUES (1, 'Before');",
            )
            .unwrap();

        let update = execute_raw_query(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "UPDATE items SET name = 'After' WHERE id = 1",
        )
        .unwrap();
        let select = execute_raw_query(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "SELECT name FROM items WHERE id = 1",
        )
        .unwrap();

        assert_eq!(update.affected_rows, 1);
        assert!(update.columns.is_empty());
        assert_eq!(select.rows.len(), 1);
        assert_eq!(select.rows[0][0].value, json!("After"));
    }

    #[test]
    fn raw_query_limits_large_results_while_reading() {
        let connection = Connection::open_sqlite_memory().unwrap();

        let result = execute_raw_query(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "WITH RECURSIVE numbers(value) AS (\
                SELECT 1 UNION ALL SELECT value + 1 FROM numbers WHERE value <= 10000\
             ) SELECT value FROM numbers",
        )
        .unwrap();

        assert_eq!(result.rows.len(), CUSTOM_QUERY_ROW_LIMIT);
        assert!(result.truncated);
    }
}
