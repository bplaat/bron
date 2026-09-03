/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write;

use bsql::{Connection, StatementError, Value};

use crate::database::{DatabaseBackend, State};

const INSERT_BATCH_SIZE: usize = 100;

pub(crate) fn import_sql(state: &State, sql: &str) -> Result<(), String> {
    let sql = sql.strip_prefix('\u{feff}').unwrap_or(sql);
    if sql.trim().is_empty() {
        return Err("The selected SQL file is empty".to_string());
    }

    let connection = state
        .lock()
        .expect("mutex poisoned")
        .connection_snapshot()?
        .0;
    let result = connection
        .execute_script(sql)
        .map_err(|error| error.to_string());
    state.lock().expect("mutex poisoned").clear_table_metadata();
    result.map(|_| ())
}

pub(crate) fn export_sql(state: &State) -> Result<String, String> {
    let (connection, backend, database) = state
        .lock()
        .expect("mutex poisoned")
        .connection_snapshot()?;
    match backend {
        DatabaseBackend::Sqlite => export_sqlite(&connection),
        DatabaseBackend::Mysql => export_mysql(
            &connection,
            database
                .as_deref()
                .ok_or_else(|| "No MySQL database selected".to_string())?,
        ),
    }
    .map_err(|error| error.to_string())
}

fn export_sqlite(connection: &Connection) -> Result<String, StatementError> {
    let tables = connection
        .query::<(String, String)>(
            "SELECT name, sql FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND sql IS NOT NULL
             ORDER BY name",
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;

    let mut dump = String::from(
        "-- Sequel Explorer SQLite export\nPRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\n\n",
    );
    let views = connection
        .query::<String>(
            "SELECT name FROM sqlite_master WHERE type = 'view' ORDER BY name",
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    for view in views {
        writeln!(
            dump,
            "DROP VIEW IF EXISTS {};",
            quote_identifier(DatabaseBackend::Sqlite, &view)
        )
        .expect("writing to a string cannot fail");
    }
    if !tables.is_empty() {
        dump.push('\n');
    }
    for (table, schema) in &tables {
        writeln!(
            dump,
            "DROP TABLE IF EXISTS {};",
            quote_identifier(DatabaseBackend::Sqlite, table)
        )
        .expect("writing to a string cannot fail");
        writeln!(dump, "{};\n", schema.trim_end_matches(';'))
            .expect("writing to a string cannot fail");
    }
    for (table, _) in &tables {
        let columns = connection
            .query::<(String, i64)>(
                format!(
                    "SELECT name, hidden FROM pragma_table_xinfo({}) ORDER BY cid",
                    quote_text(table)
                ),
                (),
            )?
            .filter_map(|row| match row {
                Ok((name, 0)) => Some(Ok(name)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        export_table_rows(
            connection,
            DatabaseBackend::Sqlite,
            table,
            &columns,
            &mut dump,
        )?;
    }

    let objects = connection
        .query::<String>(
            "SELECT sql FROM sqlite_master
             WHERE type IN ('index', 'trigger', 'view') AND sql IS NOT NULL
             ORDER BY CASE type WHEN 'index' THEN 0 WHEN 'view' THEN 1 ELSE 2 END, name",
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    for schema in objects {
        writeln!(dump, "{};", schema.trim_end_matches(';'))
            .expect("writing to a string cannot fail");
    }
    dump.push_str("\nCOMMIT;\nPRAGMA foreign_keys=ON;\n");
    Ok(dump)
}

fn export_mysql(connection: &Connection, database: &str) -> Result<String, StatementError> {
    let tables = connection
        .query::<String>(
            "SELECT TABLE_NAME FROM information_schema.TABLES
             WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
             ORDER BY TABLE_NAME",
            database.to_string(),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut dump = format!(
        "-- Sequel Explorer MySQL export\n-- Database: {}\nSET FOREIGN_KEY_CHECKS=0;\n\n",
        database.replace(['\n', '\r'], " ")
    );

    for table in &tables {
        let quoted_table = quote_identifier(DatabaseBackend::Mysql, table);
        let (_, schema) = connection
            .query_some::<(String, String)>(format!("SHOW CREATE TABLE {quoted_table}"), ())?;
        writeln!(dump, "DROP TABLE IF EXISTS {quoted_table};")
            .expect("writing to a string cannot fail");
        writeln!(dump, "{};\n", schema.trim_end_matches(';'))
            .expect("writing to a string cannot fail");
    }
    for table in &tables {
        let columns = connection
            .query::<String>(
                "SELECT COLUMN_NAME FROM information_schema.COLUMNS
                 WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? AND EXTRA NOT LIKE '%GENERATED%'
                 ORDER BY ORDINAL_POSITION",
                (database.to_string(), table.clone()),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        export_table_rows(
            connection,
            DatabaseBackend::Mysql,
            table,
            &columns,
            &mut dump,
        )?;
    }
    dump.push_str("SET FOREIGN_KEY_CHECKS=1;\n");
    Ok(dump)
}

fn export_table_rows(
    connection: &Connection,
    backend: DatabaseBackend,
    table: &str,
    columns: &[String],
    dump: &mut String,
) -> Result<(), StatementError> {
    let quoted_table = quote_identifier(backend, table);
    if columns.is_empty() {
        let count =
            connection.query_some::<i64>(format!("SELECT COUNT(*) FROM {quoted_table}"), ())?;
        for _ in 0..count {
            match backend {
                DatabaseBackend::Sqlite => {
                    writeln!(dump, "INSERT INTO {quoted_table} DEFAULT VALUES;")
                }
                DatabaseBackend::Mysql => {
                    writeln!(dump, "INSERT INTO {quoted_table} () VALUES ();")
                }
            }
            .expect("writing to a string cannot fail");
        }
        return Ok(());
    }

    let quoted_columns = columns
        .iter()
        .map(|column| quote_identifier(backend, column))
        .collect::<Vec<_>>()
        .join(", ");
    let mut statement =
        connection.prepare::<()>(format!("SELECT {quoted_columns} FROM {quoted_table}"))?;
    let mut row_index = 0;
    while statement.step()?.is_some() {
        if row_index % INSERT_BATCH_SIZE == 0 {
            if row_index > 0 {
                dump.push_str(";\n");
            }
            writeln!(dump, "INSERT INTO {quoted_table} ({quoted_columns}) VALUES")
                .expect("writing to a string cannot fail");
        } else {
            dump.push_str(",\n");
        }
        dump.push('(');
        for index in 0..statement.column_count() {
            if index > 0 {
                dump.push_str(", ");
            }
            dump.push_str(&quote_value(backend, statement.column_value(index)));
        }
        dump.push(')');
        row_index += 1;
    }
    if row_index > 0 {
        dump.push_str(";\n\n");
    }
    Ok(())
}

fn quote_identifier(backend: DatabaseBackend, identifier: &str) -> String {
    match backend {
        DatabaseBackend::Sqlite => format!("\"{}\"", identifier.replace('"', "\"\"")),
        DatabaseBackend::Mysql => format!("`{}`", identifier.replace('`', "``")),
    }
}

fn quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn quote_value(backend: DatabaseBackend, value: Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Integer(value) => value.to_string(),
        Value::Float(value) if value.is_nan() => "NULL".to_string(),
        Value::Float(value) if value.is_finite() => value.to_string(),
        Value::Float(value) if value.is_sign_positive() => "9e999".to_string(),
        Value::Float(_) => "-9e999".to_string(),
        Value::Text(value) if backend == DatabaseBackend::Mysql => hex_literal(value.as_bytes()),
        Value::Text(value) if value.contains('\0') => {
            format!("CAST({} AS TEXT)", hex_literal(value.as_bytes()))
        }
        Value::Text(value) => quote_text(&value),
        Value::Blob(value) => hex_literal(&value),
    }
}

fn hex_literal(bytes: &[u8]) -> String {
    let mut literal = String::with_capacity(bytes.len() * 2 + 3);
    literal.push_str("X'");
    for byte in bytes {
        write!(literal, "{byte:02X}").expect("writing to a string cannot fail");
    }
    literal.push('\'');
    literal
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use bsql::{Connection, PoolOptions, SqliteMode};

    use super::{export_sql, import_sql};
    use crate::database::DatabaseState;

    #[test]
    fn sqlite_export_import_round_trip() {
        let source = Connection::open_sqlite(
            ":memory:",
            SqliteMode::ReadWrite,
            PoolOptions::single_connection(),
        )
        .expect("source database should open");
        source
            .execute_script(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, payload BLOB, generated TEXT AS (name || '!'));\n\
                 CREATE INDEX items_name ON items (name);\n\
                 CREATE VIEW item_names AS SELECT name FROM items;\n\
                 INSERT INTO items (id, name, payload) VALUES (1, 'O''Brien', X'00FF');",
            )
            .expect("fixture should be created");
        let state = Arc::new(Mutex::new(DatabaseState::sqlite(source)));

        let dump = export_sql(&state).expect("database should export");
        assert!(dump.contains("CREATE TABLE items"));
        assert!(dump.contains("CREATE INDEX items_name"));
        assert!(dump.contains("DROP VIEW IF EXISTS \"item_names\""));
        assert!(!dump.contains("\"generated\") VALUES"));

        let target = Connection::open_sqlite(
            ":memory:",
            SqliteMode::ReadWrite,
            PoolOptions::single_connection(),
        )
        .expect("target database should open");
        target
            .execute("CREATE VIEW item_names AS SELECT 'old' AS name", ())
            .expect("conflicting view should be created");
        let target_state = Arc::new(Mutex::new(DatabaseState::sqlite(target.clone())));
        import_sql(&target_state, &dump).expect("database should import");
        let row = target
            .query_some::<(String, Vec<u8>, String)>(
                "SELECT name, payload, generated FROM items WHERE id = 1",
                (),
            )
            .expect("exported row should exist");
        assert_eq!(
            row,
            ("O'Brien".to_string(), vec![0, 255], "O'Brien!".to_string())
        );
        assert_eq!(
            target
                .query_some::<String>("SELECT name FROM item_names", ())
                .expect("exported view should work"),
            "O'Brien"
        );
    }

    #[test]
    fn import_rejects_empty_files() {
        let connection = Connection::open_sqlite(
            ":memory:",
            SqliteMode::ReadWrite,
            PoolOptions::single_connection(),
        )
        .expect("database should open");
        let state = Arc::new(Mutex::new(DatabaseState::sqlite(connection)));
        assert_eq!(
            import_sql(&state, "\u{feff}  \n").unwrap_err(),
            "The selected SQL file is empty"
        );
    }
}
