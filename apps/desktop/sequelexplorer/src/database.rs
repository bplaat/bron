/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::{HashMap, HashSet};
use std::fmt::Write;
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
use crate::sql::{
    SqlStatement, quote_identifier, quote_identifiers, quote_mysql_text, quote_text,
    split_statements,
};

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
    table_metadata: HashMap<String, Arc<TableMetadata>>,
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
    format!("{}@{}", quote_text(user), quote_text(host))
}

/// Builds an IDENTIFIED BY clause, MySQL rejects placeholders there so the password is a literal
fn mysql_identified_by(conn: &Connection, password: &str) -> Result<String, StatementError> {
    // Backslashes are only escapes when the session does not use NO_BACKSLASH_ESCAPES
    let no_backslash_escapes = password.contains('\\')
        && conn.query_some::<i64>(
            "SELECT @@SESSION.sql_mode LIKE '%NO_BACKSLASH_ESCAPES%'",
            (),
        )? == 1;
    Ok(mysql_identified_by_literal(password, no_backslash_escapes))
}

fn mysql_identified_by_literal(password: &str, no_backslash_escapes: bool) -> String {
    let literal = if no_backslash_escapes {
        quote_text(password)
    } else {
        quote_mysql_text(password)
    };
    format!("IDENTIFIED BY {literal}")
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DatabaseBody {
    name: String,
    #[serde(default)]
    character_set: Option<String>,
    #[serde(default)]
    collation: Option<String>,
}

fn validate_database_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Database name is required".to_string());
    }
    if name.contains(['\0', '\n', '\r', '/', '\\', '.']) || name.ends_with(' ') {
        return Err("Database name contains unsupported characters".to_string());
    }
    Ok(())
}

fn validate_charset_name(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(format!(
            "{label} must only contain letters, digits, and underscores"
        ));
    }
    Ok(())
}

fn create_database_sql(body: &DatabaseBody) -> Result<String, String> {
    validate_database_name(&body.name)?;
    let mut sql = format!(
        "CREATE DATABASE {}",
        quote_identifier(DatabaseBackend::Mysql, &body.name)
    );
    if let Some(character_set) = body
        .character_set
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        validate_charset_name(character_set, "Character set")?;
        write!(sql, " CHARACTER SET {character_set}").expect("writing to a string cannot fail");
    }
    if let Some(collation) = body.collation.as_deref().filter(|value| !value.is_empty()) {
        validate_charset_name(collation, "Collation")?;
        write!(sql, " COLLATE {collation}").expect("writing to a string cannot fail");
    }
    Ok(sql)
}

pub(crate) fn db_databases_create(req: &Request, state: &State) -> Result<Response> {
    let body: DatabaseBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    let sql = match create_database_sql(&body) {
        Ok(sql) => sql,
        Err(error) => return Ok(error_response(error)),
    };
    let guard = match mysql_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    let conn = guard.connection.as_ref().expect("connection checked above");
    Ok(match conn.execute(sql, ()) {
        Ok(_) => Response::with_json(json!({ "ok": true })),
        Err(error) => error_response(error),
    })
}

pub(crate) fn db_databases_drop(req: &Request, state: &State) -> Result<Response> {
    let body: DatabaseBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    if let Err(error) = validate_database_name(&body.name) {
        return Ok(error_response(error));
    }
    let mut guard = match mysql_connection(state) {
        Ok(guard) => guard,
        Err(response) => return Ok(response),
    };
    let conn = guard.connection.clone().expect("connection checked above");
    let sql = format!(
        "DROP DATABASE {}",
        quote_identifier(DatabaseBackend::Mysql, &body.name)
    );
    Ok(match conn.execute(sql, ()) {
        Ok(_) => {
            if guard.database.as_deref() == Some(body.name.as_str()) {
                guard.database = None;
                guard.table_metadata.clear();
            }
            Response::with_json(json!({ "ok": true }))
        }
        Err(error) => error_response(error),
    })
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
            json!({ "error": "This action is only available for MySQL connections" }),
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
    let result = mysql_identified_by(conn, &body.password)
        .and_then(|identified_by| conn.execute(format!("CREATE USER {account} {identified_by}"), ()))
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
                format!(
                    "ALTER USER {account} {}",
                    mysql_identified_by(conn, &body.password)?
                ),
                (),
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
    #[serde(default)]
    sort: Option<String>,
    #[serde(default)]
    desc: bool,
    #[serde(default)]
    filter_column: Option<String>,
    #[serde(default)]
    filter_operator: Option<FilterOperator>,
    #[serde(default)]
    filter_value: Option<String>,
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
    generated_columns: HashSet<String>,
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
                // `REFERENCES parent` without a column list has a NULL `to` and targets the primary key
                let rows = conn.query::<(String, String, Option<String>)>(
                    format!(
                        "SELECT fk.\"from\", fk.\"table\", COALESCE(fk.\"to\",
                            (SELECT name FROM pragma_table_info(fk.\"table\") WHERE pk = 1))
                         FROM pragma_foreign_key_list({}) AS fk",
                        quote_text(table)
                    ),
                    (),
                )?;
                for row in rows {
                    let (column, foreign_table, Some(foreign_column)) = row? else {
                        continue;
                    };
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
    let table_name = quote_text(table);
    // Hidden 1 marks virtual table columns, 2 and 3 are generated columns
    let rows = conn
        .query::<(String, String, i64, i64)>(
            format!(
                "SELECT name, type, pk, hidden FROM pragma_table_xinfo({table_name})
                 WHERE hidden <> 1 ORDER BY cid"
            ),
            (),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let generated_columns = rows
        .iter()
        .filter(|(_, _, _, hidden)| *hidden != 0)
        .map(|(name, _, _, _)| name.clone())
        .collect();
    let columns = rows
        .into_iter()
        .map(|(name, declared_type, position, _)| (name, declared_type, position))
        .collect::<Vec<_>>();
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
        generated_columns,
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
    let rows = conn
        .query::<(String, String, String)>(
            "SELECT COLUMN_NAME, COLUMN_TYPE, GENERATION_EXPRESSION
             FROM information_schema.COLUMNS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?
             ORDER BY ORDINAL_POSITION",
            (database.to_string(), table.to_string()),
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let generated_columns = rows
        .iter()
        .filter(|(_, _, expression)| !expression.is_empty())
        .map(|(name, _, _)| name.clone())
        .collect();
    let columns = rows
        .into_iter()
        .map(|(name, column_type, _)| (name, column_type))
        .collect::<Vec<_>>();
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
        generated_columns,
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

#[derive(Clone, Deserialize)]
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

struct TableContext {
    connection: Connection,
    backend: DatabaseBackend,
    metadata: Arc<TableMetadata>,
}

impl TableContext {
    fn table(&self, name: &str) -> String {
        quote_identifier(self.backend, name)
    }

    /// Null-safe equality on every primary key column, the values are bound in key order
    fn key_condition(&self) -> String {
        let comparison = match self.backend {
            DatabaseBackend::Mysql => "<=>",
            DatabaseBackend::Sqlite => "IS",
        };
        self.metadata
            .key_columns
            .iter()
            .map(|column| format!("{} {comparison} ?", quote_identifier(self.backend, column)))
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    fn key_values(
        &self,
        keys: &HashMap<String, InputCellValue>,
    ) -> Result<Vec<Value>, StatementError> {
        if self.metadata.key_columns.is_empty() || keys.len() != self.metadata.key_columns.len() {
            return Err(StatementError::new(
                "A complete primary key is required to change this row",
            ));
        }
        self.metadata
            .key_columns
            .iter()
            .map(|column| {
                keys.get(column)
                    .cloned()
                    .ok_or_else(|| {
                        StatementError::new("A complete primary key is required to change this row")
                    })
                    .and_then(input_cell_value)
            })
            .collect()
    }

    fn order_column(&self, column: &str) -> String {
        let quoted = quote_identifier(self.backend, column);
        if self.backend == DatabaseBackend::Mysql
            && self
                .metadata
                .declared_types
                .get(column)
                .is_some_and(|declared_type| declared_type_is_blob(declared_type))
        {
            format!("HEX({quoted})")
        } else {
            quoted
        }
    }

    fn check_column(&self, column: &str) -> Result<(), StatementError> {
        if self.metadata.columns.iter().any(|name| name == column) {
            Ok(())
        } else {
            Err(StatementError::new(format!(
                "Column {column} does not exist"
            )))
        }
    }
}

pub(crate) fn error_response(error: impl ToString) -> Response {
    Response::with_json(json!({ "error": error.to_string() }))
}

fn table_context(state: &State, name: &str) -> Result<TableContext, Response> {
    let mut guard = get_connection(state)?;
    let connection = guard.connection.clone().expect("connection checked above");
    let backend = guard.backend.expect("backend set with connection");
    let metadata = match guard.table_metadata.get(name) {
        Some(metadata) => Arc::clone(metadata),
        None => {
            let database = match backend {
                DatabaseBackend::Mysql => Some(
                    current_database(&guard)
                        .map_err(error_response)?
                        .to_string(),
                ),
                DatabaseBackend::Sqlite => None,
            };
            let metadata = Arc::new(
                load_table_metadata(&connection, backend, database.as_deref(), name)
                    .map_err(error_response)?,
            );
            guard
                .table_metadata
                .insert(name.to_string(), Arc::clone(&metadata));
            metadata
        }
    };
    Ok(TableContext {
        connection,
        backend,
        metadata,
    })
}

/// Runs a table route handler, turning database errors into JSON error responses
fn table_request(
    req: &Request,
    state: &State,
    handler: impl FnOnce(&str, &TableContext) -> Result<Response, StatementError>,
) -> Result<Response> {
    let name = req.params.get("name").expect("route has a table name");
    Ok(match table_context(state, name) {
        Ok(context) => handler(name, &context).unwrap_or_else(error_response),
        Err(response) => response,
    })
}

// MARK: Table data filters
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
enum FilterOperator {
    #[serde(rename = "=")]
    Equal,
    #[serde(rename = "!=")]
    NotEqual,
    #[serde(rename = "<")]
    Less,
    #[serde(rename = "<=")]
    LessOrEqual,
    #[serde(rename = ">")]
    Greater,
    #[serde(rename = ">=")]
    GreaterOrEqual,
    Contains,
    NotContains,
    StartsWith,
    EndsWith,
    IsNull,
    IsNotNull,
}

fn escape_like(value: &str) -> String {
    value
        .replace('!', "!!")
        .replace('%', "!%")
        .replace('_', "!_")
}

/// Builds a WHERE condition for one column filter, returning the value to bind if any
fn filter_condition(
    context: &TableContext,
    column: &str,
    operator: FilterOperator,
    value: Option<&str>,
) -> Result<(String, Option<Value>), StatementError> {
    context.check_column(column)?;
    let column = quote_identifier(context.backend, column);
    let value = || {
        value
            .map(str::to_string)
            .ok_or_else(|| StatementError::new("Filter value is required"))
    };
    let (condition, bound) = match operator {
        FilterOperator::IsNull => (format!("{column} IS NULL"), None),
        FilterOperator::IsNotNull => (format!("{column} IS NOT NULL"), None),
        FilterOperator::Equal => (format!("{column} = ?"), Some(value()?)),
        FilterOperator::NotEqual => (format!("{column} <> ?"), Some(value()?)),
        FilterOperator::Less => (format!("{column} < ?"), Some(value()?)),
        FilterOperator::LessOrEqual => (format!("{column} <= ?"), Some(value()?)),
        FilterOperator::Greater => (format!("{column} > ?"), Some(value()?)),
        FilterOperator::GreaterOrEqual => (format!("{column} >= ?"), Some(value()?)),
        FilterOperator::Contains => (
            format!("{column} LIKE ? ESCAPE '!'"),
            Some(format!("%{}%", escape_like(&value()?))),
        ),
        FilterOperator::NotContains => (
            format!("({column} IS NULL OR {column} NOT LIKE ? ESCAPE '!')"),
            Some(format!("%{}%", escape_like(&value()?))),
        ),
        FilterOperator::StartsWith => (
            format!("{column} LIKE ? ESCAPE '!'"),
            Some(format!("{}%", escape_like(&value()?))),
        ),
        FilterOperator::EndsWith => (
            format!("{column} LIKE ? ESCAPE '!'"),
            Some(format!("%{}", escape_like(&value()?))),
        ),
    };
    Ok((condition, bound.map(Value::Text)))
}

// MARK: Table data queries
pub(crate) fn db_table_data(req: &Request, state: &State) -> Result<Response> {
    let query = match serde_urlencoded::from_str::<TableDataQuery>(req.url.query().unwrap_or("")) {
        Ok(query) => query,
        Err(_) => return Ok(error_response("Invalid query parameters")),
    };
    if query.offset < 0 || !(1..=1_000).contains(&query.limit) {
        return Ok(error_response("Offset or limit is out of range"));
    }
    table_request(req, state, |name, context| table_data(name, context, query))
}

fn table_data(
    name: &str,
    context: &TableContext,
    query: TableDataQuery,
) -> Result<Response, StatementError> {
    let metadata = &context.metadata;
    let table = context.table(name);
    let filter = match query
        .filter_column
        .as_deref()
        .filter(|column| !column.is_empty())
    {
        Some(column) => Some(filter_condition(
            context,
            column,
            query
                .filter_operator
                .ok_or_else(|| StatementError::new("Filter operator is required"))?,
            query.filter_value.as_deref(),
        )?),
        None => None,
    };
    let where_clause = filter
        .as_ref()
        .map_or_else(String::new, |(condition, _)| format!(" WHERE {condition}"));
    let filter_value = filter.and_then(|(_, value)| value);

    let total = if query.offset == 0 {
        let mut count = context
            .connection
            .prepare::<i64>(format!("SELECT COUNT(*) FROM {table}{where_clause}"))?;
        if let Some(value) = filter_value.clone() {
            count.bind_value(0, value)?;
        }
        count.next().transpose()?
    } else {
        None
    };

    let sort = query.sort.as_deref().filter(|sort| !sort.is_empty());
    let mut order_by = Vec::new();
    if let Some(sort) = sort {
        context.check_column(sort)?;
        let direction = if query.desc { "DESC" } else { "ASC" };
        order_by.push(format!("{} {direction}", context.order_column(sort)));
    }
    order_by.extend(
        metadata
            .order_columns
            .iter()
            .filter(|column| sort != Some(column.as_str()))
            .map(|column| context.order_column(column)),
    );
    let order_by = order_by.join(", ");

    // Keyset pagination only follows the primary key order, so sorted or filtered views use offsets
    let cursor = if sort.is_none() && where_clause.is_empty() {
        query.cursor.as_deref().map(parse_cursor).transpose()?
    } else {
        None
    };
    let mut values = Vec::new();
    let mut stmt = match cursor.filter(|_| !metadata.key_columns.is_empty()) {
        Some(cursor) => {
            if cursor.len() != metadata.key_columns.len() {
                return Err(StatementError::new("Invalid table cursor"));
            }
            let key_columns = quote_identifiers(context.backend, &metadata.key_columns);
            let placeholders = vec!["?"; cursor.len()].join(", ");
            values.extend(cursor);
            values.push(Value::Integer(query.limit));
            context.connection.prepare::<()>(format!(
                "SELECT * FROM {table} WHERE ({key_columns}) > ({placeholders}) \
                 ORDER BY {order_by} LIMIT ?"
            ))?
        }
        None => {
            values.extend(filter_value);
            values.push(Value::Integer(query.limit));
            values.push(Value::Integer(query.offset));
            context.connection.prepare::<()>(format!(
                "SELECT * FROM {table}{where_clause} ORDER BY {order_by} LIMIT ? OFFSET ?"
            ))?
        }
    };
    for (index, value) in values.into_iter().enumerate() {
        stmt.bind_value(index as i32, value)?;
    }

    let StatementOutput { columns, rows, .. } = process_statement(&mut stmt, Some(metadata))?;
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

// MARK: Table rows
#[derive(Deserialize)]
struct TableUpdateBody {
    values: HashMap<String, InputCellValue>,
    keys: HashMap<String, InputCellValue>,
}

#[derive(Deserialize)]
struct TableDeleteBody {
    rows: Vec<HashMap<String, InputCellValue>>,
}

#[derive(Deserialize)]
struct TableInsertBody {
    rows: Vec<HashMap<String, Option<InputCellValue>>>,
}

pub(crate) fn db_table_insert(req: &Request, state: &State) -> Result<Response> {
    let body: TableInsertBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    table_request(req, state, |name, context| {
        if body.rows.is_empty() {
            return Err(StatementError::new("No rows to insert"));
        }
        context
            .connection
            .transaction::<_, StatementError>(|transaction| {
                for values in &body.rows {
                    insert_row(transaction, context, name, values)?;
                }
                Ok(())
            })?;
        Ok(Response::with_json(
            json!({ "ok": true, "inserted": body.rows.len() }),
        ))
    })
}

fn insert_row(
    connection: &Connection,
    context: &TableContext,
    name: &str,
    values: &HashMap<String, Option<InputCellValue>>,
) -> Result<(), StatementError> {
    let metadata = &context.metadata;
    if let Some(column) = values
        .keys()
        .find(|column| !metadata.columns.contains(column))
    {
        return Err(StatementError::new(format!(
            "Column {column} does not exist"
        )));
    }
    let mut supplied = Vec::new();
    for column in &metadata.columns {
        let value = values.get(column);
        if metadata.generated_columns.contains(column) {
            if value.is_some_and(Option::is_some) {
                return Err(StatementError::new(format!(
                    "Generated column {column} cannot be set"
                )));
            }
            continue;
        }
        match value {
            Some(Some(value)) => supplied.push((column, value.clone())),
            Some(None) => {}
            None => {
                return Err(StatementError::new(
                    "Every column must have a value, NULL, or Default",
                ));
            }
        }
    }
    let table = context.table(name);
    let sql = if supplied.is_empty() {
        match context.backend {
            DatabaseBackend::Mysql => format!("INSERT INTO {table} () VALUES ()"),
            DatabaseBackend::Sqlite => format!("INSERT INTO {table} DEFAULT VALUES"),
        }
    } else {
        let names = supplied
            .iter()
            .map(|(column, _)| quote_identifier(context.backend, column))
            .collect::<Vec<_>>()
            .join(", ");
        let placeholders = vec!["?"; supplied.len()].join(", ");
        format!("INSERT INTO {table} ({names}) VALUES ({placeholders})")
    };
    let mut statement = connection.prepare::<()>(sql)?;
    for (index, (_, value)) in supplied.into_iter().enumerate() {
        statement.bind_value(index as i32, input_cell_value(value)?)?;
    }
    statement.next().transpose()?;
    if statement.execution_result().affected_rows != 1 {
        return Err(StatementError::new("The row was not inserted"));
    }
    Ok(())
}

pub(crate) fn db_table_update(req: &Request, state: &State) -> Result<Response> {
    let body: TableUpdateBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    table_request(req, state, |name, context| {
        if body.values.is_empty() {
            return Err(StatementError::new("No values to update"));
        }
        for column in body.values.keys() {
            context.check_column(column)?;
            if context.metadata.generated_columns.contains(column) {
                return Err(StatementError::new("Generated columns are read-only"));
            }
        }
        // Bind in table column order so the statement text is stable for the same columns
        let mut values = body.values.clone();
        let updates = context
            .metadata
            .columns
            .iter()
            .filter_map(|column| values.remove(column).map(|value| (column, value)))
            .collect::<Vec<_>>();
        let key_values = context.key_values(&body.keys)?;
        let table = context.table(name);
        let condition = context.key_condition();
        let assignments = updates
            .iter()
            .map(|(column, _)| format!("{} = ?", quote_identifier(context.backend, column)))
            .collect::<Vec<_>>()
            .join(", ");
        // Run in a transaction so an update that matched several rows is rolled back
        context
            .connection
            .transaction::<_, StatementError>(|transaction| {
                let mut statement = transaction.prepare::<()>(format!(
                    "UPDATE {table} SET {assignments} WHERE {condition}"
                ))?;
                for (index, value) in updates
                    .into_iter()
                    .map(|(_, value)| input_cell_value(value))
                    .chain(key_values.iter().cloned().map(Ok))
                    .enumerate()
                {
                    statement.bind_value(index as i32, value?)?;
                }
                statement.next().transpose()?;
                let affected_rows = statement.execution_result().affected_rows;
                drop(statement);
                if affected_rows > 1 {
                    return Err(StatementError::new("The update matched more than one row"));
                }
                // MySQL reports zero affected rows when the value did not change, so check the row exists
                if affected_rows == 0 {
                    let mut exists = transaction.prepare::<i64>(format!(
                        "SELECT COUNT(*) FROM {table} WHERE {condition}"
                    ))?;
                    for (index, key) in key_values.into_iter().enumerate() {
                        exists.bind_value(index as i32, key)?;
                    }
                    if exists.next().transpose()?.unwrap_or(0) == 0 {
                        return Err(StatementError::new(
                            "The row was not updated; it may have changed or been deleted",
                        ));
                    }
                }
                Ok(())
            })?;
        Ok(Response::with_json(json!({ "ok": true })))
    })
}

pub(crate) fn db_table_delete(req: &Request, state: &State) -> Result<Response> {
    let body: TableDeleteBody = match parse_json_body(req) {
        Ok(body) => body,
        Err(response) => return Ok(response),
    };
    table_request(req, state, |name, context| {
        if body.rows.is_empty() {
            return Err(StatementError::new("No rows to delete"));
        }
        let sql = format!(
            "DELETE FROM {} WHERE {}",
            context.table(name),
            context.key_condition()
        );
        context
            .connection
            .transaction::<_, StatementError>(|transaction| {
                for keys in &body.rows {
                    let mut statement = transaction.prepare::<()>(&sql)?;
                    for (index, key) in context.key_values(keys)?.into_iter().enumerate() {
                        statement.bind_value(index as i32, key)?;
                    }
                    statement.next().transpose()?;
                    match statement.execution_result().affected_rows {
                        1 => {}
                        0 => return Err(StatementError::new("A selected row no longer exists")),
                        _ => return Err(StatementError::new("More than one row matched")),
                    }
                }
                Ok(())
            })?;
        Ok(Response::with_json(
            json!({ "ok": true, "deleted": body.rows.len() }),
        ))
    })
}

// MARK: Custom query
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
        return Ok(format!("SELECT * FROM ({sql}\n) AS {alias} LIMIT {limit}"));
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
    format!("SELECT {projection} FROM ({sql}\n) AS {alias} ({derived_columns}) LIMIT {limit}")
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
    let sql = validate_read_only_query(connection.backend, sql).map_err(str::to_string)?;
    execute_custom_query(
        &connection.connection,
        connection.backend,
        connection.database.as_deref(),
        &sql,
    )
    .map_err(|error| error.to_string())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawQueryResult {
    columns: Vec<ColumnInfo>,
    rows: Vec<Vec<CellValue>>,
    affected_rows: u64,
    statements: usize,
    truncated: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum RawQueryResponse {
    Success(RawQueryResult),
    Error { error: String },
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
        statements: 1,
        truncated,
    })
}

/// Error of a multi-statement run, keeps the statement number without a nested error prefix
struct RawQueryError(String);

impl From<StatementError> for RawQueryError {
    fn from(error: StatementError) -> Self {
        Self(error.to_string())
    }
}

impl std::fmt::Display for RawQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Runs one or more statements, multiple statements run in one transaction (MySQL DDL commits implicitly)
pub(crate) fn run_raw_query(
    connection: &QueryConnection,
    sql: &str,
) -> Result<RawQueryResult, String> {
    let QueryConnection {
        connection,
        backend,
        database,
    } = connection;
    let statements = split_statements(*backend, sql);
    if statements.is_empty() {
        return Err("Query is required".to_string());
    }
    if statements.iter().any(SqlStatement::is_transaction_control) {
        return Err(
            "Transaction statements are not supported, multiple statements already run in one transaction"
                .to_string(),
        );
    }
    if let [statement] = statements.as_slice() {
        return execute_raw_query(connection, *backend, database.as_deref(), &statement.sql)
            .map_err(|error| error.to_string());
    }
    // MySQL commits implicitly around DDL, so those statements cannot be rolled back
    let has_implicit_commit = *backend == DatabaseBackend::Mysql
        && statements.iter().any(|statement| {
            [
                "CREATE", "ALTER", "DROP", "RENAME", "TRUNCATE", "GRANT", "REVOKE",
            ]
            .iter()
            .any(|keyword| statement.starts_with(keyword))
        });
    connection
        .transaction::<_, RawQueryError>(|transaction| {
            let mut result: Option<RawQueryResult> = None;
            let mut affected_rows = 0;
            for (index, statement) in statements.iter().enumerate() {
                let statement_result =
                    execute_raw_query(transaction, *backend, database.as_deref(), &statement.sql)
                        .map_err(|error| {
                        let note = if has_implicit_commit {
                            ", earlier schema changes were already committed by MySQL"
                        } else {
                            ""
                        };
                        RawQueryError(format!("Statement {} failed: {error}{note}", index + 1))
                    })?;
                // SQLite keeps reporting the previous change count for row returning statements
                let is_write = ["INSERT", "UPDATE", "DELETE", "REPLACE"]
                    .iter()
                    .any(|keyword| statement.starts_with(keyword));
                if is_write || statement_result.columns.is_empty() {
                    affected_rows += statement_result.affected_rows;
                }
                // Show the last statement that returned rows, like a SQL console does
                if !statement_result.columns.is_empty() || result.is_none() {
                    result = Some(statement_result);
                }
            }
            let mut result = result.expect("at least one statement executed");
            result.affected_rows = affected_rows;
            result.statements = statements.len();
            Ok(result)
        })
        .map_err(|error| error.to_string())
}

/// Returns the single trimmed SELECT statement without its trailing semicolon
fn validate_read_only_query(backend: DatabaseBackend, sql: &str) -> Result<String, &'static str> {
    let mut statements = split_statements(backend, sql);
    let [statement] = statements.as_mut_slice() else {
        return Err(if statements.is_empty() {
            "Enter a SELECT query"
        } else {
            "Only one SELECT statement is allowed"
        });
    };
    if statement.has_executable_comment {
        return Err("MySQL executable comments are not allowed");
    }
    let words = &statement.words;
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
    Ok(std::mem::take(&mut statement.sql))
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
    fn mysql_passwords_are_escaped_literals() {
        assert_eq!(
            mysql_identified_by_literal("it's\\x", false),
            "IDENTIFIED BY 'it''s\\\\x'"
        );
        assert_eq!(
            mysql_identified_by_literal("it's\\x", true),
            "IDENTIFIED BY 'it''s\\x'"
        );
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
                "CREATE TABLE persons (id INTEGER PRIMARY KEY, name TEXT NOT NULL, score REAL);\
                 INSERT INTO persons (id, name, score) VALUES (1, 'Before', 2.5);",
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
                "values": {
                    "name": { "kind": "text", "value": "After" },
                    "score": { "kind": "null", "value": null }
                },
                "keys": { "id": { "kind": "integer", "value": "1" } }
            }))
            .unwrap(),
        );

        let response = db_table_update(&request, &state).unwrap();

        assert_eq!(response.body, br#"{"ok":true}"#);
        let guard = state.lock().unwrap();
        let row = guard
            .connection
            .as_ref()
            .unwrap()
            .query_some::<(String, Option<f64>)>("SELECT name, score FROM persons WHERE id = 1", ())
            .unwrap();
        assert_eq!(row, ("After".to_string(), None));
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
                "rows": [{ "id": { "kind": "text", "value": key } }]
            }))
            .unwrap(),
        );

        let response = db_table_delete(&request, &state).unwrap();

        assert_eq!(response.body, br#"{"deleted":1,"ok":true}"#);
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
                "rows": [{
                    "id": null,
                    "name": { "kind": "text", "value": "Created" },
                    "enabled": null
                }]
            }))
            .unwrap(),
        );

        let response = db_table_insert(&request, &state).unwrap();

        assert_eq!(response.body, br#"{"inserted":1,"ok":true}"#);
        let guard = state.lock().unwrap();
        let row = guard
            .connection
            .as_ref()
            .unwrap()
            .query_some::<(String, i64)>("SELECT name, enabled FROM items", ())
            .unwrap();
        assert_eq!(row, ("Created".to_string(), 1));
    }

    fn sqlite_state(script: &str) -> State {
        let connection = Connection::open_sqlite_memory().unwrap();
        connection.execute_script(script).unwrap();
        Arc::new(Mutex::new(DatabaseState::sqlite(connection)))
    }

    fn table_request_for(table: &str, url: &str, body: Option<serde_json::Value>) -> Request {
        let mut request = Request::default();
        request.params.insert("name".to_string(), table.to_string());
        request.url = url.parse().unwrap();
        request.body = body.map(|body| serde_json::to_vec(&body).unwrap());
        request
    }

    fn count_rows(state: &State, sql: &str) -> i64 {
        let guard = state.lock().unwrap();
        guard
            .connection
            .as_ref()
            .unwrap()
            .query_some::<i64>(sql, ())
            .unwrap()
    }

    #[test]
    fn table_delete_of_multiple_rows_is_atomic() {
        let state = sqlite_state(
            "CREATE TABLE items (id INTEGER PRIMARY KEY);
             INSERT INTO items VALUES (1), (2);",
        );
        let key = |id: &str| json!({ "id": { "kind": "integer", "value": id } });
        let request = table_request_for(
            "items",
            "http://localhost/",
            Some(json!({ "rows": [key("1"), key("3")] })),
        );
        let response = db_table_delete(&request, &state).unwrap();
        assert!(
            String::from_utf8_lossy(&response.body).contains("A selected row no longer exists")
        );
        assert_eq!(count_rows(&state, "SELECT COUNT(*) FROM items"), 2);

        let request = table_request_for(
            "items",
            "http://localhost/",
            Some(json!({ "rows": [key("1"), key("2")] })),
        );
        db_table_delete(&request, &state).unwrap();
        assert_eq!(count_rows(&state, "SELECT COUNT(*) FROM items"), 0);
    }

    #[test]
    fn table_insert_skips_generated_columns() {
        let state = sqlite_state(
            "CREATE TABLE items (
                 id INTEGER PRIMARY KEY,
                 price INTEGER NOT NULL,
                 doubled INTEGER GENERATED ALWAYS AS (price * 2)
             );",
        );
        let insert = |doubled: serde_json::Value| {
            let request = table_request_for(
                "items",
                "http://localhost/",
                Some(json!({ "rows": [{
                    "id": null,
                    "price": { "kind": "integer", "value": "4" },
                    "doubled": doubled,
                }] })),
            );
            serde_json::from_slice::<serde_json::Value>(
                &db_table_insert(&request, &state).unwrap().body,
            )
            .unwrap()
        };
        assert_eq!(insert(serde_json::Value::Null)["inserted"], 1);
        assert_eq!(count_rows(&state, "SELECT doubled FROM items"), 8);
        let rejected = insert(json!({ "kind": "integer", "value": "1" }));
        assert!(
            rejected["error"]
                .as_str()
                .unwrap()
                .ends_with("Generated column doubled cannot be set")
        );
    }

    #[test]
    fn table_data_sorts_and_filters_with_escaped_patterns() {
        let state = sqlite_state(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO items VALUES (1, '100% cotton'), (2, '100 percent'), (3, 'wool'), (4, NULL);",
        );
        let data = |query: &str| {
            let request = table_request_for(
                "items",
                &format!("http://localhost/api/table/items/data?{query}"),
                None,
            );
            serde_json::from_slice::<serde_json::Value>(
                &db_table_data(&request, &state).unwrap().body,
            )
            .unwrap()
        };

        let sorted = data("sort=id&desc=true");
        assert_eq!(sorted["rows"][0][0]["value"], "4");
        assert_eq!(sorted["total"], 4);

        let filtered = data("filter_column=name&filter_operator=contains&filter_value=0%25");
        assert_eq!(filtered["total"], 1);
        assert_eq!(filtered["rows"][0][1]["value"], "100% cotton");

        let missing = data("filter_column=name&filter_operator=isNull");
        assert_eq!(missing["total"], 1);
        assert_eq!(missing["rows"][0][0]["value"], "4");

        let invalid = data("sort=missing");
        assert!(
            invalid["error"]
                .as_str()
                .unwrap()
                .ends_with("Column missing does not exist")
        );
    }

    #[test]
    fn raw_query_runs_multiple_statements_atomically() {
        let state =
            sqlite_state("CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT NOT NULL);");
        let connection = query_connection(&state).unwrap();

        let error = run_raw_query(
            &connection,
            "INSERT INTO items (name) VALUES ('a'); INSERT INTO items (name) VALUES (NULL);",
        )
        .err()
        .expect("second statement should fail");
        assert!(error.starts_with("Statement 2 failed"), "{error}");
        assert_eq!(count_rows(&state, "SELECT COUNT(*) FROM items"), 0);

        let result = run_raw_query(
            &connection,
            "INSERT INTO items (name) VALUES ('a'); INSERT INTO items (name) VALUES ('b');
             SELECT name FROM items ORDER BY id; UPDATE items SET name = 'c'",
        )
        .unwrap();
        assert_eq!(result.statements, 4);
        assert_eq!(result.affected_rows, 4);
        assert_eq!(result.rows.len(), 2);

        assert!(run_raw_query(&connection, "BEGIN; DELETE FROM items; COMMIT").is_err());
        assert!(run_raw_query(&connection, "-- only a comment").is_err());
    }

    #[test]
    fn database_sql_validates_names_and_charsets() {
        let body = |name: &str, character_set: Option<&str>| DatabaseBody {
            name: name.to_string(),
            character_set: character_set.map(str::to_string),
            collation: Some("utf8mb4_unicode_ci".to_string()),
        };
        assert_eq!(
            create_database_sql(&body("shop`s", Some("utf8mb4"))).unwrap(),
            "CREATE DATABASE `shop``s` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"
        );
        assert!(create_database_sql(&body("", None)).is_err());
        assert!(create_database_sql(&body("a/b", None)).is_err());
        assert!(create_database_sql(&body("shop", Some("utf8; DROP"))).is_err());
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

    #[test]
    fn query_accepts_trailing_semicolon_and_comment() {
        let state = query_state();
        let connection = query_connection(&state).expect("capture query connection");
        for sql in ["SELECT * FROM children;", "SELECT * FROM children -- note"] {
            let result = run_query(&connection, sql).expect("query should run");
            assert_eq!(result.rows.len(), 1);
        }
    }

    #[test]
    fn foreign_keys_without_column_target_primary_key() {
        let connection = Connection::open_sqlite_memory().expect("open test database");
        connection
            .execute_script(
                "CREATE TABLE parents (id INTEGER PRIMARY KEY);
                 CREATE TABLE children (parent_id REFERENCES parents);",
            )
            .expect("create fixtures");
        let tables = HashSet::from(["children".to_string()]);
        let foreign_keys = load_foreign_keys(&connection, DatabaseBackend::Sqlite, None, &tables)
            .expect("load foreign keys");
        let foreign_key = &foreign_keys[&("children".to_string(), "parent_id".to_string())];
        assert_eq!(
            (foreign_key.table.as_str(), foreign_key.column.as_str()),
            ("parents", "id")
        );
    }
}
