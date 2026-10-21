/*
 * Copyright (c) 2025-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use anyhow::{Context as _, Result};
use bsql::{Connection, PoolOptions, SqliteMode, run_migrations};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::models::{Note, Session, User, UserRole};
use crate::utils::password_hash;

// MARK: Context
#[derive(Clone)]
pub(crate) struct Context {
    // Shared, because the router clones the context for every request
    pub server_origin: Arc<str>,
    pub database: Connection,
    pub auth_session: Option<Session>,
    pub auth_user: Option<User>,
    pub update_target_user_id: Option<Uuid>,
    pub login_attempts: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
    pub maxminddb_reader: Arc<OnceLock<maxminddb::Reader<maxminddb::Mmap>>>,
    pub trusted_proxies: Arc<[IpAddr]>,
    pub is_e2e: bool,
}

impl Context {
    #[allow(dead_code)]
    pub(crate) fn with_database(path: impl AsRef<Path>, server_origin: String) -> Result<Self> {
        log::info!("Using database at {}", path.as_ref().display());
        let database =
            Connection::open_sqlite(path.as_ref(), SqliteMode::ReadWrite, PoolOptions::default())?;
        database.execute_script("PRAGMA foreign_keys = ON")?;
        database.enable_wal_logging()?;
        log::info!("Running database migrations...");
        run_migrations!(database, "src/migrations")?;
        database_seed(&database)?;
        Ok(Self {
            server_origin: server_origin.into(),
            database,
            auth_session: None,
            auth_user: None,
            update_target_user_id: None,
            login_attempts: Arc::new(Mutex::new(HashMap::new())),
            maxminddb_reader: Arc::new(OnceLock::new()),
            trusted_proxies: trusted_proxies_from_env()?,
            is_e2e: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_database() -> Result<Self> {
        let database = Connection::open_sqlite_memory()?;
        database.execute_script("PRAGMA foreign_keys = ON")?;
        log::info!("Running database migrations...");
        run_migrations!(database, "src/migrations")?;
        Ok(Self {
            server_origin: Arc::from("*"),
            database,
            auth_session: None,
            auth_user: None,
            update_target_user_id: None,
            login_attempts: Arc::new(Mutex::new(HashMap::new())),
            maxminddb_reader: Arc::new(OnceLock::new()),
            trusted_proxies: Arc::from([]),
            is_e2e: false,
        })
    }

    pub(crate) fn with_e2e_database() -> Result<Self> {
        let database = Connection::open_sqlite_memory()?;
        database.execute_script("PRAGMA foreign_keys = ON")?;
        log::info!("Running database migrations...");
        run_migrations!(database, "src/migrations")?;
        database_seed(&database)?;
        Ok(Self {
            server_origin: Arc::from("*"),
            database,
            auth_session: None,
            auth_user: None,
            update_target_user_id: None,
            login_attempts: Arc::new(Mutex::new(HashMap::new())),
            maxminddb_reader: Arc::new(OnceLock::new()),
            trusted_proxies: Arc::from([]),
            is_e2e: true,
        })
    }
}

fn trusted_proxies_from_env() -> Result<Arc<[IpAddr]>> {
    let Some(value) = std::env::var("TRUSTED_PROXIES").ok() else {
        return Ok(Arc::from([]));
    };
    value
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse()
                .with_context(|| format!("invalid trusted proxy IP address: {value}"))
        })
        .collect::<Result<Vec<_>>>()
        .map(Arc::from)
}

// MARK: Database helpers
pub(crate) trait DatabaseHelpers {
    fn insert_user(&self, user: User) -> Result<()>;
    fn insert_session(&self, session: Session) -> Result<()>;
    fn insert_note(&self, note: Note) -> Result<()>;
}

impl DatabaseHelpers for Connection {
    fn insert_user(&self, user: User) -> Result<()> {
        self.execute(
            format!(
                "INSERT INTO users ({}) VALUES ({})",
                User::columns(),
                User::values()
            ),
            user,
        )?;
        Ok(())
    }

    fn insert_session(&self, session: Session) -> Result<()> {
        self.execute(
            format!(
                "INSERT INTO sessions ({}) VALUES ({})",
                Session::columns(),
                Session::values()
            ),
            session,
        )?;
        Ok(())
    }

    fn insert_note(&self, note: Note) -> Result<()> {
        self.execute(
            format!(
                "INSERT INTO notes ({}) VALUES ({})",
                Note::columns(),
                Note::values()
            ),
            note,
        )?;
        Ok(())
    }
}

fn database_seed(database: &Connection) -> Result<()> {
    if database.query_some::<i64>("SELECT COUNT(id) FROM users", ())? == 0 {
        let admin_email =
            std::env::var("ADMIN_EMAIL").unwrap_or_else(|_| "admin@example.com".to_string());
        let admin_password = Zeroizing::new(
            std::env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "Password123!".to_string()),
        );
        let user = User {
            first_name: "Admin".to_string(),
            last_name: "Admin".to_string(),
            email: admin_email,
            password: password_hash(&admin_password),
            role: UserRole::Admin,
            ..Default::default()
        };
        database.insert_user(user)?;
        log::info!("Admin account created");
    }
    Ok(())
}
