/*
 * Copyright (c) 2023-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use anyhow::Result;
use bsql::{Connection, DatabaseType, run_migrations};

use crate::models::{Person, Relation};

// MARK: Context
#[derive(Clone)]
pub(crate) struct Context {
    pub database: Connection,
}

impl Context {
    pub(crate) fn with_database(database: Connection) -> Result<Self> {
        match database.database_type() {
            DatabaseType::Sqlite => {
                database.execute_script("PRAGMA foreign_keys = ON")?;
                database.enable_wal_logging()?;
            }
            DatabaseType::Mysql => {}
        }
        log::info!("Running database migrations...");
        migrate(&database)?;
        database_seed(&database)?;
        Ok(Self { database })
    }

    #[cfg(test)]
    pub(crate) fn with_test_database() -> Result<Self> {
        let database = Connection::open_sqlite_memory()?;
        database.execute_script("PRAGMA foreign_keys = ON")?;
        log::info!("Running database migrations...");
        migrate(&database)?;
        Ok(Self { database })
    }
}

fn migrate(database: &Connection) -> Result<()> {
    match database.database_type() {
        DatabaseType::Sqlite => run_migrations!(database, "src/migrations/sqlite")?,
        DatabaseType::Mysql => run_migrations!(database, "src/migrations/mysql")?,
    }
    Ok(())
}

// MARK: Database Helpers
pub(crate) trait DatabaseHelpers {
    fn insert_person(&self, person: Person) -> Result<()>;
}

impl DatabaseHelpers for Connection {
    fn insert_person(&self, person: Person) -> Result<()> {
        self.execute(
            format!(
                "INSERT INTO persons ({}) VALUES ({})",
                Person::columns(),
                Person::values()
            ),
            person,
        )?;
        Ok(())
    }
}

fn database_seed(database: &Connection) -> Result<()> {
    database.transaction(|database| {
        // Insert persons
        if database.query_some::<i64>("SELECT COUNT(id) FROM persons", ())? == 0 {
            database.insert_person(Person {
                name: "Bastiaan".to_string(),
                age_in_years: 20,
                relation: Relation::Me,
                ..Default::default()
            })?;
            database.insert_person(Person {
                name: "Sander".to_string(),
                age_in_years: 19,
                relation: Relation::Brother,
                ..Default::default()
            })?;
            database.insert_person(Person {
                name: "Leonard".to_string(),
                age_in_years: 16,
                relation: Relation::Brother,
                ..Default::default()
            })?;
            database.insert_person(Person {
                name: "Jiska".to_string(),
                age_in_years: 14,
                relation: Relation::Sister,
                ..Default::default()
            })?;
        }
        Ok(())
    })
}
