/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::fmt::Write;

use bsql::{Connection, DatabaseType, StatementError, Value};

use crate::database::State;
use crate::sql::{quote_identifier, quote_text};

const INSERT_BATCH_SIZE: usize = 100;

pub(crate) fn import_sql(state: &State, sql: &str) -> Result<(), String> {
    let sql = sql.strip_prefix('\u{feff}').unwrap_or(sql);
    if sql.trim().is_empty() {
        return Err("The selected SQL file is empty".to_string());
    }

    let (connection, backend, _) = state
        .lock()
        .expect("mutex poisoned")
        .connection_snapshot()?;
    // Dumps turn foreign key checks off, restore them even when the script fails halfway
    let foreign_keys_query = match backend {
        DatabaseType::Sqlite => "PRAGMA foreign_keys",
        DatabaseType::Mysql => "SELECT @@FOREIGN_KEY_CHECKS",
    };
    let foreign_keys = connection
        .query_some::<i64>(foreign_keys_query, ())
        .map_err(|error| error.to_string())?;
    let result = connection
        .execute_script(sql)
        .map(|_| ())
        .map_err(|error| error.to_string());
    let restore = connection
        .execute(
            match backend {
                DatabaseType::Sqlite => format!("PRAGMA foreign_keys = {foreign_keys}"),
                DatabaseType::Mysql => format!("SET FOREIGN_KEY_CHECKS = {foreign_keys}"),
            },
            (),
        )
        .map(|_| ())
        .map_err(|error| error.to_string());
    state.lock().expect("mutex poisoned").clear_table_metadata();
    result.and(restore)
}

pub(crate) fn export_sql(state: &State) -> Result<String, String> {
    let (connection, backend, database) = state
        .lock()
        .expect("mutex poisoned")
        .connection_snapshot()?;
    match backend {
        DatabaseType::Sqlite => export_sqlite(&connection),
        DatabaseType::Mysql => export_mysql(
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
            quote_identifier(DatabaseType::Sqlite, &view)
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
            quote_identifier(DatabaseType::Sqlite, table)
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
        export_table_rows(connection, DatabaseType::Sqlite, table, &columns, &mut dump)?;
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
    let names = |sql: &str| {
        connection
            .query::<String>(sql, database.to_string())?
            .collect::<Result<Vec<_>, _>>()
    };
    let tables = names(
        "SELECT TABLE_NAME FROM information_schema.TABLES
         WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'BASE TABLE'
         ORDER BY TABLE_NAME",
    )?;
    let views = names(
        "SELECT TABLE_NAME FROM information_schema.TABLES
         WHERE TABLE_SCHEMA = ? AND TABLE_TYPE = 'VIEW'
         ORDER BY TABLE_NAME",
    )?;
    let triggers = names(
        "SELECT TRIGGER_NAME FROM information_schema.TRIGGERS
         WHERE TRIGGER_SCHEMA = ? ORDER BY EVENT_OBJECT_TABLE, ACTION_ORDER",
    )?;
    let events = names(
        "SELECT EVENT_NAME FROM information_schema.EVENTS WHERE EVENT_SCHEMA = ? ORDER BY EVENT_NAME",
    )?;
    let routines = connection
        .query::<(String, String)>(
            "SELECT ROUTINE_TYPE, ROUTINE_NAME FROM information_schema.ROUTINES
             WHERE ROUTINE_SCHEMA = ? ORDER BY ROUTINE_TYPE, ROUTINE_NAME",
            database.to_string(),
        )?
        .collect::<Result<Vec<_>, _>>()?;

    let mut dump = format!(
        "-- Sequel Explorer MySQL export\n-- Database: {}\nSET FOREIGN_KEY_CHECKS=0;\n\n",
        database.replace(['\n', '\r'], " ")
    );
    for view in &views {
        writeln!(
            dump,
            "DROP VIEW IF EXISTS {};",
            quote_identifier(DatabaseType::Mysql, view)
        )
        .expect("writing to a string cannot fail");
    }
    if !views.is_empty() {
        dump.push('\n');
    }

    for table in &tables {
        let quoted_table = quote_identifier(DatabaseType::Mysql, table);
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
        export_table_rows(connection, DatabaseType::Mysql, table, &columns, &mut dump)?;
    }

    // Create views after the views they select from
    let mut views = views
        .iter()
        .map(|view| {
            let quoted_view = quote_identifier(DatabaseType::Mysql, view);
            show_create(connection, &format!("SHOW CREATE VIEW {quoted_view}"), 1)
                .map(|schema| (format!(".{quoted_view}"), schema))
        })
        .collect::<Result<Vec<_>, _>>()?;
    while !views.is_empty() {
        // View bodies reference other views as `database`.`view`
        let index = views
            .iter()
            .position(|(name, schema)| {
                !views
                    .iter()
                    .any(|(other, _)| other != name && schema.contains(other.as_str()))
            })
            .unwrap_or(0);
        let (_, schema) = views.remove(index);
        writeln!(dump, "{schema};\n").expect("writing to a string cannot fail");
    }

    for trigger in &triggers {
        let quoted_trigger = quote_identifier(DatabaseType::Mysql, trigger);
        let schema = show_create(
            connection,
            &format!("SHOW CREATE TRIGGER {quoted_trigger}"),
            2,
        )?;
        writeln!(
            dump,
            "DROP TRIGGER IF EXISTS {quoted_trigger};\n{schema};\n"
        )
        .expect("writing to a string cannot fail");
    }
    for (kind, routine) in &routines {
        let quoted_routine = quote_identifier(DatabaseType::Mysql, routine);
        let kind = if kind == "FUNCTION" {
            "FUNCTION"
        } else {
            "PROCEDURE"
        };
        let schema = show_create(
            connection,
            &format!("SHOW CREATE {kind} {quoted_routine}"),
            2,
        )?;
        writeln!(dump, "DROP {kind} IF EXISTS {quoted_routine};\n{schema};\n")
            .expect("writing to a string cannot fail");
    }
    for event in &events {
        let quoted_event = quote_identifier(DatabaseType::Mysql, event);
        let schema = show_create(connection, &format!("SHOW CREATE EVENT {quoted_event}"), 3)?;
        writeln!(dump, "DROP EVENT IF EXISTS {quoted_event};\n{schema};\n")
            .expect("writing to a string cannot fail");
    }
    dump.push_str("SET FOREIGN_KEY_CHECKS=1;\n");
    Ok(dump)
}

/// Reads the definition column of a MySQL SHOW CREATE statement
fn show_create(connection: &Connection, sql: &str, column: i32) -> Result<String, StatementError> {
    let mut statement = connection.prepare::<()>(sql)?;
    if statement.step()?.is_none() {
        return Err(StatementError::new(format!("{sql} returned no rows")));
    }
    match statement.column_value(column) {
        Value::Text(schema) => Ok(schema.trim_end_matches(';').to_string()),
        _ => Err(StatementError::new(format!(
            "{sql} returned no definition, the user may lack privileges to read it"
        ))),
    }
}

fn export_table_rows(
    connection: &Connection,
    backend: DatabaseType,
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
                DatabaseType::Sqlite => {
                    writeln!(dump, "INSERT INTO {quoted_table} DEFAULT VALUES;")
                }
                DatabaseType::Mysql => {
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

fn quote_value(backend: DatabaseType, value: Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Integer(value) => value.to_string(),
        Value::Float(value) if value.is_nan() => "NULL".to_string(),
        Value::Float(value) if value.is_finite() => value.to_string(),
        Value::Float(value) if value.is_sign_positive() => "9e999".to_string(),
        Value::Float(_) => "-9e999".to_string(),
        // Hex keeps any text intact, the conversion lets JSON columns accept it
        Value::Text(value) if backend == DatabaseType::Mysql => {
            format!("CONVERT({} USING utf8mb4)", hex_literal(value.as_bytes()))
        }
        Value::Text(value) if value.contains('\0') => {
            format!("CAST({} AS TEXT)", hex_literal(value.as_bytes()))
        }
        Value::Text(value) => quote_text(&value),
        Value::Date(value) => quote_text(&value.to_string()),
        Value::DateTime(value) => quote_text(&value.naive_utc().to_string()),
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

    use bsql::{Connection, DatabaseType, PoolOptions, SqliteMode, Value};

    use super::{export_sql, import_sql, quote_value};
    use crate::database::DatabaseState;

    #[test]
    fn temporal_values_export_as_sql_literals() {
        assert_eq!(
            quote_value(
                DatabaseType::Mysql,
                Value::Date("2024-08-30".parse().unwrap())
            ),
            "'2024-08-30'"
        );
        assert_eq!(
            quote_value(
                DatabaseType::Mysql,
                Value::DateTime("2024-08-30T01:02:03.123456Z".parse().unwrap())
            ),
            "'2024-08-30 01:02:03.123456'"
        );
    }

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
    fn failed_import_restores_foreign_keys_and_transaction_state() {
        let connection = Connection::open_sqlite(
            ":memory:",
            SqliteMode::ReadWrite,
            PoolOptions::single_connection(),
        )
        .expect("database should open");
        connection
            .execute_script("PRAGMA foreign_keys=ON; CREATE TABLE items (id INTEGER PRIMARY KEY);")
            .expect("fixture should be created");
        let state = Arc::new(Mutex::new(DatabaseState::sqlite(connection.clone())));
        assert!(
            import_sql(
                &state,
                "PRAGMA foreign_keys=OFF;\nBEGIN TRANSACTION;\nINSERT INTO items VALUES (1);\nINSERT INTO items VALUES (1);\nCOMMIT;"
            )
            .is_err()
        );
        assert_eq!(
            connection
                .query_some::<i64>("PRAGMA foreign_keys", ())
                .expect("pragma should be readable"),
            1
        );
        connection
            .transaction::<_, bsql::StatementError>(|transaction| {
                transaction.execute("INSERT INTO items VALUES (2)", ())
            })
            .expect("a new transaction should start");
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
