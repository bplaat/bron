/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};

use crate::{Connection, DatabaseType, StatementError};

// MARK: MigrationError
/// A migration error
#[derive(Debug)]
pub struct MigrationError {
    msg: String,
}

impl MigrationError {
    fn new(msg: impl Into<String>) -> Self {
        Self { msg: msg.into() }
    }
}

impl Display for MigrationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "Migration error: {}", self.msg)
    }
}

impl Error for MigrationError {}

impl From<StatementError> for MigrationError {
    fn from(e: StatementError) -> Self {
        Self { msg: e.to_string() }
    }
}

// MARK: Migration
/// A versioned SQL migration in Flyway style: `V<version>__<description>`
pub struct Migration {
    /// Migration name, e.g. `v1__create_base_tables`
    pub name: &'static str,
    /// SQL to execute; may contain multiple statements separated by semicolons
    pub sql: &'static str,
}

// MARK: Connection::migration
impl Connection {
    /// Apply all pending migrations in version order, tracking applied versions in
    /// the `schema_migrations` table.
    ///
    /// SQLite rolls back failed migrations atomically. MySQL DDL implicitly commits,
    /// so a failed migration may leave schema changes that require manual recovery.
    pub fn migration(&self, migrations: &[Migration]) -> Result<(), MigrationError> {
        if self.is_transaction() {
            return Err(MigrationError::new(
                "migrations cannot run inside a transaction",
            ));
        }
        let mut sorted: Vec<_> = migrations.iter().collect();
        sorted.sort_by_key(|migration| parse_version(migration.name));
        for (index, migration) in sorted.iter().enumerate() {
            let version = parse_version(migration.name);
            if version == 0 {
                return Err(MigrationError::new(format!(
                    "migration '{}' has no valid version prefix (expected V<n>__...)",
                    migration.name
                )));
            }
            if index > 0 && parse_version(sorted[index - 1].name) == version {
                return Err(MigrationError::new(format!(
                    "duplicate migration version {version}"
                )));
            }
        }
        self.execute(
            match self.database_type() {
                DatabaseType::Sqlite => {
                    "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    applied_at INTEGER NOT NULL
                ) STRICT"
                }
                DatabaseType::Mysql => {
                    "CREATE TABLE IF NOT EXISTS schema_migrations (
                    version BIGINT PRIMARY KEY,
                    name VARCHAR(255) NOT NULL,
                    applied_at BIGINT NOT NULL
                ) ENGINE=InnoDB"
                }
            },
            (),
        )?;

        let applied = self
            .query::<i64>("SELECT version FROM schema_migrations", ())?
            .collect::<Result<HashSet<_>, _>>()?;

        for migration in sorted {
            let version = i64::from(parse_version(migration.name));
            if applied.contains(&version) {
                continue;
            }
            self.transaction(|db| -> Result<(), MigrationError> {
                let sql = match self.database_type() {
                    DatabaseType::Sqlite => preprocess_migration_sql(migration.sql)?,
                    DatabaseType::Mysql => migration.sql.to_string(),
                };
                db.execute_script(&sql)?;
                db.execute(
                    "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?, ?, ?)",
                    (version, migration.name.to_string(), applied_at()?),
                )?;
                Ok(())
            })?;
        }
        Ok(())
    }
}

fn applied_at() -> Result<i64, MigrationError> {
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| MigrationError::new(error.to_string()))?;
    i64::try_from(duration.as_secs()).map_err(|error| MigrationError::new(error.to_string()))
}

fn parse_version(name: &str) -> u32 {
    let Some(version) = name.strip_prefix(['V', 'v']) else {
        return 0;
    };
    let Some((version, description)) = version.split_once("__") else {
        return 0;
    };
    if description.is_empty() || !version.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    version.parse().unwrap_or(0)
}

fn preprocess_migration_sql(sql: &str) -> Result<String, MigrationError> {
    let mut output = String::new();
    for line in sql.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("-- bsql:create_fts5_table") {
            output.push_str(&expand_fts5_directive(rest.trim())?);
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    Ok(output)
}

fn expand_fts5_directive(rest: &str) -> Result<String, MigrationError> {
    let parts = rest
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.len() < 2 {
        return Err(MigrationError::new(
            "bsql:create_fts5_table expects a table name and at least one indexed column",
        ));
    }

    let table = parts[0];
    validate_identifier(table)?;
    let columns = &parts[1..];
    for column in columns {
        validate_identifier(column)?;
        if *column == "id" {
            return Err(MigrationError::new(
                "bsql:create_fts5_table adds id automatically; do not list it as an indexed column",
            ));
        }
    }

    let fts_table = format!("{table}_fts");
    let indexed_columns = columns.join(", ");
    let all_columns = format!("{indexed_columns}, id");
    let insert_values = columns
        .iter()
        .map(|column| format!("new.{column}"))
        .chain(["new.id".to_string()])
        .collect::<Vec<_>>()
        .join(", ");
    let update_assignments = columns
        .iter()
        .map(|column| format!("{column} = new.{column}"))
        .collect::<Vec<_>>()
        .join(", ");

    let mut sql = String::new();
    _ = writeln!(
        sql,
        "CREATE VIRTUAL TABLE IF NOT EXISTS {fts_table} USING fts5({indexed_columns}, id UNINDEXED);"
    );
    _ = writeln!(
        sql,
        "CREATE TRIGGER IF NOT EXISTS {table}_ai AFTER INSERT ON {table} BEGIN"
    );
    _ = writeln!(
        sql,
        "    INSERT INTO {fts_table}({all_columns}) VALUES ({insert_values});"
    );
    _ = writeln!(sql, "END;");
    _ = writeln!(
        sql,
        "CREATE TRIGGER IF NOT EXISTS {table}_au AFTER UPDATE ON {table} BEGIN"
    );
    _ = writeln!(
        sql,
        "    UPDATE {fts_table} SET {update_assignments} WHERE id = old.id;"
    );
    _ = writeln!(sql, "END;");
    _ = writeln!(
        sql,
        "CREATE TRIGGER IF NOT EXISTS {table}_ad BEFORE DELETE ON {table} BEGIN"
    );
    _ = writeln!(sql, "    DELETE FROM {fts_table} WHERE id = old.id;");
    _ = writeln!(sql, "END;");
    _ = writeln!(
        sql,
        "INSERT INTO {fts_table}({all_columns}) SELECT {all_columns} FROM {table};"
    );
    Ok(sql)
}

fn validate_identifier(identifier: &str) -> Result<(), MigrationError> {
    let mut chars = identifier.chars();
    let Some(first) = chars.next() else {
        return Err(MigrationError::new(
            "bsql:create_fts5_table identifier can't be empty",
        ));
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(MigrationError::new(format!(
            "bsql:create_fts5_table invalid identifier '{identifier}'"
        )));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(MigrationError::new(format!(
            "bsql:create_fts5_table invalid identifier '{identifier}'"
        )));
    }
    Ok(())
}

// MARK: Tests
#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::Connection;

    #[test]
    fn migration_validates_all_versions_before_changing_schema() {
        for name in [
            "v1__duplicate",
            "1__missing_prefix",
            "vv2__bad_prefix",
            "v2suffix",
            "v2__",
        ] {
            let db = Connection::open_sqlite_memory().unwrap();
            let result = db.migration(&[
                Migration {
                    name: "v1__create_table",
                    sql: "CREATE TABLE t (id INTEGER PRIMARY KEY) STRICT",
                },
                Migration {
                    name,
                    sql: "CREATE TABLE other (id INTEGER PRIMARY KEY) STRICT",
                },
            ]);
            assert!(result.is_err(), "{name}");
            assert_eq!(
                db.query_some::<i64>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
                    ()
                )
                .unwrap(),
                0,
                "{name}"
            );
        }
    }

    #[test]
    fn migration_applies_missing_versions_below_the_latest_version() {
        let db = Connection::open_sqlite_memory().unwrap();
        db.migration(&[Migration {
            name: "v2__create_b",
            sql: "CREATE TABLE b (id INTEGER PRIMARY KEY) STRICT",
        }])
        .unwrap();
        db.migration(&[
            Migration {
                name: "v1__create_a",
                sql: "CREATE TABLE a (id INTEGER PRIMARY KEY) STRICT",
            },
            Migration {
                name: "v2__create_b",
                sql: "CREATE TABLE b (id INTEGER PRIMARY KEY) STRICT",
            },
        ])
        .unwrap();
        assert_eq!(
            db.query_some::<i64>("SELECT COUNT(*) FROM schema_migrations", ())
                .unwrap(),
            2
        );
        db.execute("INSERT INTO a VALUES (1)", ()).unwrap();
    }

    #[test]
    fn migration_rejects_transaction_connections_before_changing_schema() {
        let db = Connection::open_sqlite_memory().unwrap();
        db.transaction(|transaction| -> Result<(), MigrationError> {
            assert!(transaction.migration(&[]).is_err());
            assert_eq!(
                transaction.query_some::<i64>(
                    "SELECT COUNT(*) FROM sqlite_master WHERE name = 'schema_migrations'",
                    ()
                )?,
                0
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn test_migration_applies_in_order() {
        let db = Connection::open_sqlite_memory().unwrap();
        db.migration(&[
            Migration {
                name: "v2__add_column",
                sql: "ALTER TABLE t ADD COLUMN b INTEGER NOT NULL DEFAULT 0",
            },
            Migration {
                name: "v1__create_table",
                sql: "CREATE TABLE t (a INTEGER PRIMARY KEY) STRICT",
            },
        ])
        .unwrap();
        let count = db
            .query_some::<i64>("SELECT COUNT(*) FROM schema_migrations", ())
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_migration_skips_applied() {
        let db = Connection::open_sqlite_memory().unwrap();
        let migrations = &[Migration {
            name: "v1__create_table",
            sql: "CREATE TABLE t (a INTEGER PRIMARY KEY) STRICT",
        }];
        db.migration(migrations).unwrap();
        db.migration(migrations).unwrap();
        let count = db
            .query_some::<i64>("SELECT COUNT(*) FROM schema_migrations", ())
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_migration_multi_statement_sql() {
        let db = Connection::open_sqlite_memory().unwrap();
        db.migration(&[Migration {
            name: "v1__create_tables",
            sql: "CREATE TABLE a (id INTEGER PRIMARY KEY) STRICT;
                  CREATE TABLE b (id INTEGER PRIMARY KEY) STRICT",
        }])
        .unwrap();
        db.execute("INSERT INTO a VALUES (1)", ()).unwrap();
        db.execute("INSERT INTO b VALUES (1)", ()).unwrap();
    }

    #[test]
    fn test_migration_rolls_back_failed_migration() {
        let db = Connection::open_sqlite_memory().unwrap();
        let result = db.migration(&[Migration {
            name: "v1__partial_failure",
            sql: "CREATE TABLE a (id INTEGER PRIMARY KEY) STRICT;
                  CREATE TABLE a (id INTEGER PRIMARY KEY) STRICT",
        }]);
        assert!(result.is_err());
        let table_count = db
            .query_some::<i64>(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'a'",
                (),
            )
            .unwrap();
        assert_eq!(table_count, 0);
        let migration_count = db
            .query_some::<i64>("SELECT COUNT(*) FROM schema_migrations", ())
            .unwrap();
        assert_eq!(migration_count, 0);
    }

    #[test]
    fn test_migration_can_retry_after_failed_migration() {
        let db = Connection::open_sqlite_memory().unwrap();
        let result = db.migration(&[Migration {
            name: "v1__create_table",
            sql: "CREATE TABLE t (id INTEGER PRIMARY KEY) STRICT;
                  CREATE TABLE t (id INTEGER PRIMARY KEY) STRICT",
        }]);
        assert!(result.is_err());

        db.migration(&[Migration {
            name: "v1__create_table",
            sql: "CREATE TABLE t (id INTEGER PRIMARY KEY) STRICT",
        }])
        .unwrap();

        db.execute("INSERT INTO t VALUES (1)", ()).unwrap();
        let migration_count = db
            .query_some::<i64>("SELECT COUNT(*) FROM schema_migrations", ())
            .unwrap();
        assert_eq!(migration_count, 1);
    }

    #[test]
    fn test_migration_invalid_version_errors() {
        let db = Connection::open_sqlite_memory().unwrap();
        let result = db.migration(&[Migration {
            name: "no_version_prefix",
            sql: "SELECT 1",
        }]);
        assert!(result.is_err());
    }

    #[test]
    fn test_migration_fts5_directive_creates_synced_search_table() {
        let db = Connection::open_sqlite_memory().unwrap();
        db.migration(&[Migration {
            name: "v1__create_sessions",
            sql: "CREATE TABLE sessions (
                      id INTEGER PRIMARY KEY,
                      ip_address TEXT NOT NULL,
                      client_name TEXT NOT NULL
                  ) STRICT;
                  INSERT INTO sessions (id, ip_address, client_name) VALUES (1, '127.0.0.1', 'Firefox');
                  -- bsql:create_fts5_table sessions, ip_address, client_name",
        }])
        .unwrap();

        let count = db
            .query_some::<i64>(
                "SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH 'Firefox'",
                (),
            )
            .unwrap();
        assert_eq!(count, 1);

        db.execute(
            "INSERT INTO sessions (id, ip_address, client_name) VALUES (2, '127.0.0.2', 'Safari')",
            (),
        )
        .unwrap();
        let count = db
            .query_some::<i64>(
                "SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH 'Safari'",
                (),
            )
            .unwrap();
        assert_eq!(count, 1);

        db.execute(
            "UPDATE sessions SET client_name = 'Chrome' WHERE id = 2",
            (),
        )
        .unwrap();
        let count = db
            .query_some::<i64>(
                "SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH 'Chrome'",
                (),
            )
            .unwrap();
        assert_eq!(count, 1);
        let count = db
            .query_some::<i64>(
                "SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH 'Safari'",
                (),
            )
            .unwrap();
        assert_eq!(count, 0);

        db.execute("DELETE FROM sessions WHERE id = 2", ()).unwrap();
        let count = db
            .query_some::<i64>(
                "SELECT COUNT(*) FROM sessions_fts WHERE sessions_fts MATCH 'Chrome'",
                (),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_migration_fts5_directive_rejects_invalid_columns() {
        let db = Connection::open_sqlite_memory().unwrap();
        let result = db.migration(&[Migration {
            name: "v1__create_sessions",
            sql: "CREATE TABLE sessions (id INTEGER PRIMARY KEY) STRICT;
                  -- bsql:create_fts5_table sessions, id",
        }]);
        assert!(result.is_err());
    }
}
