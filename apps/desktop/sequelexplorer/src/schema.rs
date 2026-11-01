/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::collections::BTreeMap;
use std::fmt::Write;

use anyhow::Result;
use bsql::{Connection, StatementError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use small_http::{Request, Response};

use crate::database::{DatabaseBackend, State, error_response};
use crate::sql::{quote_identifier, quote_identifiers, quote_mysql_text, quote_text};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaColumn {
    name: String,
    r#type: String,
    nullable: bool,
    default_sql: Option<String>,
    primary_key: bool,
    primary_key_position: i64,
    auto_increment: bool,
    generated: bool,
    character_set: Option<String>,
    collation: Option<String>,
    comment: String,
    extra: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaIndex {
    name: String,
    columns: Vec<String>,
    unique: bool,
    primary: bool,
    read_only: bool,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SchemaForeignKey {
    name: String,
    columns: Vec<String>,
    referenced_table: String,
    referenced_columns: Vec<String>,
    on_update: String,
    on_delete: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableSchema {
    name: String,
    sql: String,
    columns: Vec<SchemaColumn>,
    indexes: Vec<SchemaIndex>,
    foreign_keys: Vec<SchemaForeignKey>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase")]
enum SchemaChange {
    RenameTable {
        new_name: String,
    },
    AddColumn {
        column: SchemaColumn,
    },
    UpdateColumn {
        old_name: String,
        column: SchemaColumn,
    },
    DropColumn {
        name: String,
    },
    AddIndex {
        index: SchemaIndex,
    },
    UpdateIndex {
        old_name: String,
        index: SchemaIndex,
    },
    DropIndex {
        name: String,
    },
    AddForeignKey {
        foreign_key: SchemaForeignKey,
    },
    DropForeignKey {
        name: String,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum TableOperation {
    Truncate,
    Drop,
    Duplicate { new_name: String, with_data: bool },
}

#[derive(Deserialize)]
struct CreateTableBody {
    name: String,
}

const FOREIGN_KEY_ACTIONS: [&str; 5] = [
    "NO ACTION",
    "RESTRICT",
    "CASCADE",
    "SET NULL",
    "SET DEFAULT",
];

pub(crate) fn db_table_schema(req: &Request, state: &State) -> Result<Response> {
    let table = req
        .params
        .get("name")
        .expect("table name should be present");
    let (connection, backend, database) =
        match state.lock().expect("mutex poisoned").connection_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => return Ok(error_response(error)),
        };
    let result = match (backend, database.as_deref()) {
        (DatabaseBackend::Sqlite, _) => load_sqlite_schema(&connection, table),
        (DatabaseBackend::Mysql, Some(database)) => load_mysql_schema(&connection, database, table),
        (DatabaseBackend::Mysql, None) => return Ok(error_response("No MySQL database selected")),
    };
    Ok(match result {
        Ok(schema) => Response::with_json(&schema),
        Err(error) => error_response(error.to_string()),
    })
}

pub(crate) fn db_table_schema_update(req: &Request, state: &State) -> Result<Response> {
    let table = req
        .params
        .get("name")
        .expect("table name should be present");
    let change = match serde_json::from_slice::<SchemaChange>(req.body.as_deref().unwrap_or(&[])) {
        Ok(change) => change,
        Err(error) => return Ok(error_response(format!("Invalid schema change: {error}"))),
    };
    let (connection, backend, database) =
        match state.lock().expect("mutex poisoned").connection_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => return Ok(error_response(error)),
        };
    let result = apply_schema_change(&connection, backend, database.as_deref(), table, change);
    state.lock().expect("mutex poisoned").clear_table_metadata();
    Ok(match result {
        Ok(table) => Response::with_json(json!({ "ok": true, "table": table })),
        Err(error) => error_response(error.to_string()),
    })
}

pub(crate) fn db_tables_create(req: &Request, state: &State) -> Result<Response> {
    let body = match serde_json::from_slice::<CreateTableBody>(req.body.as_deref().unwrap_or(&[])) {
        Ok(body) => body,
        Err(error) => return Ok(error_response(format!("Invalid table: {error}"))),
    };
    let (connection, backend, _) = match state.lock().expect("mutex poisoned").connection_snapshot()
    {
        Ok(snapshot) => snapshot,
        Err(error) => return Ok(error_response(error)),
    };
    Ok(match create_table(&connection, backend, &body.name) {
        Ok(()) => Response::with_json(json!({ "ok": true, "table": body.name })),
        Err(error) => error_response(error.to_string()),
    })
}

pub(crate) fn db_table_operation(req: &Request, state: &State) -> Result<Response> {
    let table = req
        .params
        .get("name")
        .expect("table name should be present");
    let operation =
        match serde_json::from_slice::<TableOperation>(req.body.as_deref().unwrap_or(&[])) {
            Ok(operation) => operation,
            Err(error) => return Ok(error_response(format!("Invalid table operation: {error}"))),
        };
    let (connection, backend, database) =
        match state.lock().expect("mutex poisoned").connection_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => return Ok(error_response(error)),
        };
    let result = apply_table_operation(&connection, backend, database.as_deref(), table, operation);
    state.lock().expect("mutex poisoned").clear_table_metadata();
    Ok(match result {
        Ok(table) => Response::with_json(json!({ "ok": true, "table": table })),
        Err(error) => error_response(error.to_string()),
    })
}

fn load_sqlite_schema(connection: &Connection, table: &str) -> Result<TableSchema, StatementError> {
    let sql = connection
        .query::<String>(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?",
            table.to_string(),
        )?
        .next()
        .transpose()?
        .ok_or_else(|| StatementError::new("Table not found"))?;
    let has_auto_increment = sql.to_ascii_uppercase().contains("AUTOINCREMENT");
    let columns = connection
        .query::<(String, String, i64, Option<String>, i64, i64)>(
            format!(
                "SELECT name, type, \"notnull\", dflt_value, pk, hidden
                 FROM pragma_table_xinfo({}) ORDER BY cid",
                quote_text(table)
            ),
            (),
        )?
        .map(|row| {
            row.map(
                |(name, r#type, not_null, default_sql, primary_key_position, hidden)| {
                    SchemaColumn {
                        name,
                        r#type,
                        nullable: not_null == 0,
                        default_sql,
                        primary_key: primary_key_position > 0,
                        primary_key_position,
                        auto_increment: has_auto_increment && primary_key_position > 0,
                        generated: hidden != 0,
                        character_set: None,
                        collation: None,
                        comment: String::new(),
                        extra: String::new(),
                    }
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let indexes = load_sqlite_indexes(connection, table)?;
    let foreign_keys = load_sqlite_foreign_keys(connection, table)?;
    Ok(TableSchema {
        name: table.to_string(),
        sql,
        columns,
        indexes,
        foreign_keys,
    })
}

fn load_sqlite_indexes(
    connection: &Connection,
    table: &str,
) -> Result<Vec<SchemaIndex>, StatementError> {
    let rows = connection
        .query::<(String, i64, String, i64)>(
            format!(
                "SELECT name, \"unique\", origin, partial FROM pragma_index_list({}) ORDER BY seq",
                quote_text(table)
            ),
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(name, unique, origin, partial)| {
            let columns = connection
                .query::<Option<String>>(
                    format!(
                        "SELECT name FROM pragma_index_info({}) ORDER BY seqno",
                        quote_text(&name)
                    ),
                    (),
                )?
                .collect::<Result<Option<Vec<_>>, _>>()?;
            Ok(SchemaIndex {
                name,
                columns: columns.clone().unwrap_or_default(),
                unique: unique != 0,
                primary: origin == "pk",
                read_only: origin != "c" || partial != 0 || columns.is_none(),
            })
        })
        .collect()
}

fn load_sqlite_foreign_keys(
    connection: &Connection,
    table: &str,
) -> Result<Vec<SchemaForeignKey>, StatementError> {
    let rows = connection
        .query::<(i64, String, String, Option<String>, String, String)>(
            format!(
                "SELECT id, \"table\", \"from\", \"to\", on_update, on_delete
                 FROM pragma_foreign_key_list({}) ORDER BY id, seq",
                quote_text(table)
            ),
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut foreign_keys = BTreeMap::<i64, SchemaForeignKey>::new();
    for (id, referenced_table, column, referenced_column, on_update, on_delete) in rows {
        let foreign_key = foreign_keys.entry(id).or_insert_with(|| SchemaForeignKey {
            name: format!("fk_{id}"),
            columns: Vec::new(),
            referenced_table,
            referenced_columns: Vec::new(),
            on_update,
            on_delete,
        });
        // A missing target column references the parent primary key
        foreign_key.referenced_columns.extend(referenced_column);
        foreign_key.columns.push(column);
    }
    Ok(foreign_keys.into_values().collect())
}

fn load_mysql_foreign_keys(
    connection: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<SchemaForeignKey>, StatementError> {
    let rows = connection
        .query::<(String, String, String, String, String, String)>(
            "SELECT k.CONSTRAINT_NAME, k.COLUMN_NAME, k.REFERENCED_TABLE_NAME,
                    k.REFERENCED_COLUMN_NAME, r.UPDATE_RULE, r.DELETE_RULE
             FROM information_schema.KEY_COLUMN_USAGE k
             JOIN information_schema.REFERENTIAL_CONSTRAINTS r
               ON r.CONSTRAINT_SCHEMA = k.CONSTRAINT_SCHEMA
              AND r.CONSTRAINT_NAME = k.CONSTRAINT_NAME
              AND r.TABLE_NAME = k.TABLE_NAME
             WHERE k.TABLE_SCHEMA = ? AND k.TABLE_NAME = ? AND k.REFERENCED_TABLE_NAME IS NOT NULL
             ORDER BY k.CONSTRAINT_NAME, k.ORDINAL_POSITION",
            (database.to_string(), table.to_string()),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut foreign_keys = BTreeMap::<String, SchemaForeignKey>::new();
    for (name, column, referenced_table, referenced_column, on_update, on_delete) in rows {
        let foreign_key = foreign_keys
            .entry(name.clone())
            .or_insert_with(|| SchemaForeignKey {
                name,
                columns: Vec::new(),
                referenced_table,
                referenced_columns: Vec::new(),
                on_update,
                on_delete,
            });
        foreign_key.columns.push(column);
        foreign_key.referenced_columns.push(referenced_column);
    }
    Ok(foreign_keys.into_values().collect())
}

fn load_mysql_schema(
    connection: &Connection,
    database: &str,
    table: &str,
) -> Result<TableSchema, StatementError> {
    let (_, sql) = connection.query_some::<(String, String)>(
        format!(
            "SHOW CREATE TABLE {}",
            quote_identifier(DatabaseBackend::Mysql, table)
        ),
        (),
    )?;
    let columns = connection
        .query::<(
            String,
            String,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            Option<String>,
            String,
        )>(
            "SELECT COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA,
                    GENERATION_EXPRESSION, CHARACTER_SET_NAME, COLLATION_NAME, COLUMN_COMMENT
             FROM information_schema.COLUMNS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY ORDINAL_POSITION",
            (database.to_string(), table.to_string()),
        )?
        .map(|row| {
            row.map(
                |(
                    name,
                    r#type,
                    nullable,
                    default,
                    extra,
                    generation_expression,
                    character_set,
                    collation,
                    comment,
                )| {
                    let default_sql = mysql_default_sql(default, &r#type, &extra);
                    SchemaColumn {
                        name,
                        r#type,
                        nullable: nullable == "YES",
                        default_sql,
                        primary_key: false,
                        primary_key_position: 0,
                        auto_increment: extra
                            .split_ascii_whitespace()
                            .any(|part| part.eq_ignore_ascii_case("auto_increment")),
                        generated: !generation_expression.is_empty(),
                        character_set,
                        collation,
                        comment,
                        extra,
                    }
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let indexes = load_mysql_indexes(connection, database, table)?;
    let primary_columns = indexes
        .iter()
        .find(|index| index.primary)
        .map_or(&[][..], |index| index.columns.as_slice());
    let columns = columns
        .into_iter()
        .map(|mut column| {
            if let Some(position) = primary_columns.iter().position(|name| name == &column.name) {
                column.primary_key = true;
                column.primary_key_position = position as i64 + 1;
            }
            column
        })
        .collect();
    let foreign_keys = load_mysql_foreign_keys(connection, database, table)?;
    Ok(TableSchema {
        name: table.to_string(),
        sql,
        columns,
        indexes,
        foreign_keys,
    })
}

fn load_mysql_indexes(
    connection: &Connection,
    database: &str,
    table: &str,
) -> Result<Vec<SchemaIndex>, StatementError> {
    let rows = connection
        .query::<(String, i64, Option<String>)>(
            "SELECT INDEX_NAME, NON_UNIQUE, COLUMN_NAME FROM information_schema.STATISTICS
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ? ORDER BY INDEX_NAME, SEQ_IN_INDEX",
            (database.to_string(), table.to_string()),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut indexes = BTreeMap::<String, SchemaIndex>::new();
    for (name, non_unique, column) in rows {
        let index = indexes.entry(name.clone()).or_insert_with(|| SchemaIndex {
            primary: name == "PRIMARY",
            name,
            columns: Vec::new(),
            unique: non_unique == 0,
            read_only: false,
        });
        if let Some(column) = column {
            index.columns.push(column);
        } else {
            index.read_only = true;
        }
    }
    Ok(indexes.into_values().collect())
}

fn mysql_default_sql(default: Option<String>, column_type: &str, extra: &str) -> Option<String> {
    default.map(|default| {
        let base_type = column_type
            .split_once('(')
            .map_or(column_type, |(base, _)| base)
            .to_ascii_lowercase();
        if extra.to_ascii_lowercase().contains("default_generated")
            || matches!(
                base_type.as_str(),
                "tinyint"
                    | "smallint"
                    | "mediumint"
                    | "int"
                    | "integer"
                    | "bigint"
                    | "decimal"
                    | "numeric"
                    | "float"
                    | "double"
                    | "real"
                    | "bit"
                    | "bool"
                    | "boolean"
            )
        {
            default
        } else {
            quote_mysql_text(&default)
        }
    })
}

fn apply_schema_change(
    connection: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    table: &str,
    change: SchemaChange,
) -> Result<String, StatementError> {
    validate_identifier(table, "Table name")?;
    match change {
        SchemaChange::RenameTable { new_name } => {
            validate_identifier(&new_name, "Table name")?;
            connection.execute(
                format!(
                    "ALTER TABLE {} RENAME TO {}",
                    quote_identifier(backend, table),
                    quote_identifier(backend, &new_name)
                ),
                (),
            )?;
            Ok(new_name)
        }
        SchemaChange::AddColumn { column } => {
            validate_column(&column)?;
            if column.primary_key {
                return Err(StatementError::new(
                    "Add the column first, then manage its primary key separately",
                ));
            }
            let definition = column_definition(backend, &column, false)?;
            connection.execute(
                format!(
                    "ALTER TABLE {} ADD COLUMN {definition}",
                    quote_identifier(backend, table)
                ),
                (),
            )?;
            Ok(table.to_string())
        }
        SchemaChange::UpdateColumn { old_name, column } => {
            validate_identifier(&old_name, "Column name")?;
            validate_column(&column)?;
            update_column(connection, backend, database, table, &old_name, &column)?;
            Ok(table.to_string())
        }
        SchemaChange::DropColumn { name } => {
            validate_identifier(&name, "Column name")?;
            connection.execute(
                format!(
                    "ALTER TABLE {} DROP COLUMN {}",
                    quote_identifier(backend, table),
                    quote_identifier(backend, &name)
                ),
                (),
            )?;
            Ok(table.to_string())
        }
        SchemaChange::AddIndex { index } => {
            validate_index(&index)?;
            connection.execute(create_index_sql(backend, table, &index), ())?;
            Ok(table.to_string())
        }
        SchemaChange::UpdateIndex { old_name, index } => {
            validate_identifier(&old_name, "Index name")?;
            validate_index(&index)?;
            match backend {
                DatabaseBackend::Sqlite => {
                    connection.transaction::<_, StatementError>(|transaction| {
                        transaction.execute(
                            format!(
                                "DROP INDEX {}",
                                quote_identifier(DatabaseBackend::Sqlite, &old_name)
                            ),
                            (),
                        )?;
                        transaction.execute(create_index_sql(backend, table, &index), ())?;
                        Ok(())
                    })?;
                }
                DatabaseBackend::Mysql => {
                    let unique = if index.unique { "UNIQUE " } else { "" };
                    connection.execute(
                        format!(
                            "ALTER TABLE {} DROP INDEX {}, ADD {unique}INDEX {} ({})",
                            quote_identifier(backend, table),
                            quote_identifier(backend, &old_name),
                            quote_identifier(backend, &index.name),
                            quote_identifiers(backend, &index.columns)
                        ),
                        (),
                    )?;
                }
            }
            Ok(table.to_string())
        }
        SchemaChange::DropIndex { name } => {
            validate_identifier(&name, "Index name")?;
            let sql = match backend {
                DatabaseBackend::Sqlite => {
                    format!("DROP INDEX {}", quote_identifier(backend, &name))
                }
                DatabaseBackend::Mysql => format!(
                    "ALTER TABLE {} DROP INDEX {}",
                    quote_identifier(backend, table),
                    quote_identifier(backend, &name)
                ),
            };
            connection.execute(sql, ())?;
            Ok(table.to_string())
        }
        SchemaChange::AddForeignKey { foreign_key } => {
            validate_foreign_key(&foreign_key)?;
            match backend {
                DatabaseBackend::Sqlite => {
                    let schema = load_sqlite_schema(connection, table)?;
                    let mut foreign_keys = schema.foreign_keys.clone();
                    foreign_keys.push(foreign_key);
                    rebuild_sqlite_table(
                        connection,
                        table,
                        &schema,
                        &schema.columns,
                        &foreign_keys,
                        true,
                    )?;
                }
                DatabaseBackend::Mysql => {
                    let constraint = if foreign_key.name.trim().is_empty() {
                        String::new()
                    } else {
                        format!(
                            "CONSTRAINT {} ",
                            quote_identifier(backend, &foreign_key.name)
                        )
                    };
                    connection.execute(
                        format!(
                            "ALTER TABLE {} ADD {constraint}{}",
                            quote_identifier(backend, table),
                            foreign_key_sql(backend, &foreign_key)
                        ),
                        (),
                    )?;
                }
            }
            Ok(table.to_string())
        }
        SchemaChange::DropForeignKey { name } => {
            validate_identifier(&name, "Foreign key name")?;
            match backend {
                DatabaseBackend::Sqlite => {
                    let schema = load_sqlite_schema(connection, table)?;
                    let foreign_keys = schema
                        .foreign_keys
                        .iter()
                        .filter(|foreign_key| foreign_key.name != name)
                        .cloned()
                        .collect::<Vec<_>>();
                    if foreign_keys.len() == schema.foreign_keys.len() {
                        return Err(StatementError::new("Foreign key not found"));
                    }
                    rebuild_sqlite_table(
                        connection,
                        table,
                        &schema,
                        &schema.columns,
                        &foreign_keys,
                        false,
                    )?;
                }
                DatabaseBackend::Mysql => {
                    connection.execute(
                        format!(
                            "ALTER TABLE {} DROP FOREIGN KEY {}",
                            quote_identifier(backend, table),
                            quote_identifier(backend, &name)
                        ),
                        (),
                    )?;
                }
            }
            Ok(table.to_string())
        }
    }
}

// MARK: Table operations
fn create_table(
    connection: &Connection,
    backend: DatabaseBackend,
    table: &str,
) -> Result<(), StatementError> {
    validate_identifier(table, "Table name")?;
    let table = quote_identifier(backend, table);
    let sql = match backend {
        DatabaseBackend::Sqlite => {
            format!("CREATE TABLE {table} (\"id\" INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL)")
        }
        DatabaseBackend::Mysql => format!(
            "CREATE TABLE {table} (`id` INT UNSIGNED NOT NULL AUTO_INCREMENT, PRIMARY KEY (`id`))"
        ),
    };
    connection.execute(sql, ())?;
    Ok(())
}

fn apply_table_operation(
    connection: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    table: &str,
    operation: TableOperation,
) -> Result<String, StatementError> {
    validate_identifier(table, "Table name")?;
    let quoted_table = quote_identifier(backend, table);
    match operation {
        TableOperation::Drop => {
            connection.execute(format!("DROP TABLE {quoted_table}"), ())?;
            Ok(table.to_string())
        }
        TableOperation::Truncate => {
            match backend {
                DatabaseBackend::Mysql => {
                    connection.execute(format!("TRUNCATE TABLE {quoted_table}"), ())?;
                }
                DatabaseBackend::Sqlite => {
                    connection.transaction::<_, StatementError>(|transaction| {
                        transaction.execute(format!("DELETE FROM {quoted_table}"), ())?;
                        // Reset AUTOINCREMENT counters like TRUNCATE does on MySQL
                        if transaction.query_some::<i64>(
                            "SELECT COUNT(*) FROM sqlite_master WHERE name = 'sqlite_sequence'",
                            (),
                        )? > 0
                        {
                            transaction.execute(
                                "DELETE FROM sqlite_sequence WHERE name = ?",
                                table.to_string(),
                            )?;
                        }
                        Ok(())
                    })?;
                }
            }
            Ok(table.to_string())
        }
        TableOperation::Duplicate {
            new_name,
            with_data,
        } => {
            validate_identifier(&new_name, "Table name")?;
            duplicate_table(connection, backend, database, table, &new_name, with_data)?;
            Ok(new_name)
        }
    }
}

fn duplicate_table(
    connection: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    table: &str,
    new_name: &str,
    with_data: bool,
) -> Result<(), StatementError> {
    let source = quote_identifier(backend, table);
    let target = quote_identifier(backend, new_name);
    let schema = match backend {
        DatabaseBackend::Sqlite => load_sqlite_schema(connection, table)?,
        DatabaseBackend::Mysql => load_mysql_schema(
            connection,
            database.ok_or_else(|| StatementError::new("No MySQL database selected"))?,
            table,
        )?,
    };
    let columns = schema
        .columns
        .iter()
        .filter(|column| !column.generated)
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let copy_sql = format!(
        "INSERT INTO {target} ({0}) SELECT {0} FROM {source}",
        quote_identifiers(backend, &columns)
    );
    match backend {
        DatabaseBackend::Sqlite => {
            let body = sqlite_table_body(&schema.sql)
                .ok_or_else(|| StatementError::new("Could not read the table definition"))?;
            connection.transaction::<_, StatementError>(|transaction| {
                transaction.execute(format!("CREATE TABLE {target} {body}"), ())?;
                for index in schema.indexes.iter().filter(|index| !index.read_only) {
                    let index = SchemaIndex {
                        name: format!("{new_name}_{}", index.name),
                        ..index.clone()
                    };
                    transaction.execute(create_index_sql(backend, new_name, &index), ())?;
                }
                if with_data {
                    transaction.execute(&copy_sql, ())?;
                }
                Ok(())
            })?;
        }
        DatabaseBackend::Mysql => {
            // MySQL DDL commits implicitly, so remove the copy again when the rows fail to copy
            connection.execute(format!("CREATE TABLE {target} LIKE {source}"), ())?;
            if with_data && let Err(error) = connection.execute(&copy_sql, ()) {
                let _ = connection.execute(format!("DROP TABLE {target}"), ());
                return Err(error);
            }
        }
    }
    Ok(())
}

/// Returns the column definition part of a SQLite CREATE TABLE statement, starting at its
/// first parenthesis outside the (possibly quoted) table name
fn sqlite_table_body(sql: &str) -> Option<&str> {
    let mut quote = None;
    for (index, character) in sql.char_indices() {
        match (quote, character) {
            (None, '"' | '`' | '\'') => quote = Some(character),
            (None, '[') => quote = Some(']'),
            (None, '(') => return Some(&sql[index..]),
            (Some(delimiter), _) if character == delimiter => quote = None,
            _ => {}
        }
    }
    None
}

fn foreign_key_sql(backend: DatabaseBackend, foreign_key: &SchemaForeignKey) -> String {
    let mut sql = format!(
        "FOREIGN KEY ({}) REFERENCES {}",
        quote_identifiers(backend, &foreign_key.columns),
        quote_identifier(backend, &foreign_key.referenced_table)
    );
    if !foreign_key.referenced_columns.is_empty() {
        write!(
            sql,
            " ({})",
            quote_identifiers(backend, &foreign_key.referenced_columns)
        )
        .expect("writing to a string cannot fail");
    }
    for (clause, action) in [
        ("ON UPDATE", &foreign_key.on_update),
        ("ON DELETE", &foreign_key.on_delete),
    ] {
        if !action.eq_ignore_ascii_case("NO ACTION") {
            write!(sql, " {clause} {}", action.to_ascii_uppercase())
                .expect("writing to a string cannot fail");
        }
    }
    sql
}

fn validate_foreign_key(foreign_key: &SchemaForeignKey) -> Result<(), StatementError> {
    if !foreign_key.name.trim().is_empty() {
        validate_identifier(&foreign_key.name, "Foreign key name")?;
    }
    validate_identifier(&foreign_key.referenced_table, "Referenced table")?;
    if foreign_key.columns.is_empty() {
        return Err(StatementError::new(
            "Select at least one foreign key column",
        ));
    }
    if foreign_key.referenced_columns.len() != foreign_key.columns.len() {
        return Err(StatementError::new(
            "Select a referenced column for every foreign key column",
        ));
    }
    for column in foreign_key
        .columns
        .iter()
        .chain(&foreign_key.referenced_columns)
    {
        validate_identifier(column, "Foreign key column")?;
    }
    for action in [&foreign_key.on_update, &foreign_key.on_delete] {
        if !FOREIGN_KEY_ACTIONS
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(action))
        {
            return Err(StatementError::new(format!(
                "Unsupported foreign key action {action}"
            )));
        }
    }
    Ok(())
}

fn update_column(
    connection: &Connection,
    backend: DatabaseBackend,
    database: Option<&str>,
    table: &str,
    old_name: &str,
    column: &SchemaColumn,
) -> Result<(), StatementError> {
    match backend {
        DatabaseBackend::Mysql => {
            let definition = column_definition(backend, column, true)?;
            let schema = load_mysql_schema(
                connection,
                database.ok_or_else(|| StatementError::new("No MySQL database selected"))?,
                table,
            )?;
            let mut primary = schema
                .columns
                .iter()
                .filter(|candidate| candidate.primary_key && candidate.name != old_name)
                .map(|candidate| (candidate.primary_key_position, candidate.name.clone()))
                .collect::<Vec<_>>();
            if column.primary_key {
                primary.push((
                    if column.primary_key_position > 0 {
                        column.primary_key_position
                    } else {
                        i64::MAX
                    },
                    column.name.clone(),
                ));
            }
            primary.sort_by_key(|(position, _)| *position);
            let old_primary = schema.columns.iter().any(|candidate| candidate.primary_key);
            let primary_changed = schema
                .columns
                .iter()
                .find(|candidate| candidate.name == old_name)
                .is_some_and(|candidate| candidate.primary_key != column.primary_key)
                || old_name != column.name && column.primary_key;
            let mut clauses = vec![format!(
                "CHANGE COLUMN {} {definition}",
                quote_identifier(backend, old_name)
            )];
            if primary_changed {
                if old_primary {
                    clauses.push("DROP PRIMARY KEY".to_string());
                }
                if !primary.is_empty() {
                    clauses.push(format!(
                        "ADD PRIMARY KEY ({})",
                        primary
                            .iter()
                            .map(|(_, name)| quote_identifier(backend, name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
            connection.execute(
                format!(
                    "ALTER TABLE {} {}",
                    quote_identifier(backend, table),
                    clauses.join(", ")
                ),
                (),
            )?;
        }
        DatabaseBackend::Sqlite => {
            let schema = load_sqlite_schema(connection, table)?;
            let old = schema
                .columns
                .iter()
                .find(|candidate| candidate.name == old_name)
                .ok_or_else(|| StatementError::new("Column not found"))?;
            if old.generated {
                return Err(StatementError::new("Generated columns are read-only"));
            }
            let definition_changed = old.r#type != column.r#type
                || old.nullable != column.nullable
                || old.default_sql != column.default_sql
                || old.auto_increment != column.auto_increment
                || old.primary_key != column.primary_key;
            if definition_changed && old_name != column.name {
                return Err(StatementError::new(
                    "Save the SQLite column rename before changing its definition",
                ));
            }
            if old_name != column.name {
                connection.execute(
                    format!(
                        "ALTER TABLE {} RENAME COLUMN {} TO {}",
                        quote_identifier(backend, table),
                        quote_identifier(backend, old_name),
                        quote_identifier(backend, &column.name)
                    ),
                    (),
                )?;
            }
            if definition_changed {
                let columns = schema
                    .columns
                    .iter()
                    .map(|candidate| {
                        if candidate.name == old_name {
                            column.clone()
                        } else {
                            candidate.clone()
                        }
                    })
                    .collect::<Vec<_>>();
                rebuild_sqlite_table(
                    connection,
                    table,
                    &schema,
                    &columns,
                    &schema.foreign_keys,
                    false,
                )?;
            }
        }
    }
    Ok(())
}

/// Recreates a SQLite table with new column and foreign key definitions, keeping its rows,
/// indexes, and triggers. The columns must keep the names and order of the current schema.
fn rebuild_sqlite_table(
    connection: &Connection,
    table: &str,
    schema: &TableSchema,
    columns: &[SchemaColumn],
    foreign_keys: &[SchemaForeignKey],
    check_foreign_keys: bool,
) -> Result<(), StatementError> {
    let upper_sql = schema.sql.to_ascii_uppercase();
    if sql_contains_word(&upper_sql, "CHECK") || sql_contains_word(&upper_sql, "COLLATE") {
        return Err(StatementError::new(
            "Changing this SQLite table is not supported because it contains CHECK or COLLATE clauses",
        ));
    }
    if schema.columns.iter().any(|column| column.generated) {
        return Err(StatementError::new(
            "Changing SQLite tables with generated columns is not supported",
        ));
    }
    let temp_table = format!("__sequel_explorer_{table}");
    let mut definitions = columns
        .iter()
        .map(|column| column_definition(DatabaseBackend::Sqlite, column, false))
        .collect::<Result<Vec<_>, _>>()?;
    let mut primary = columns
        .iter()
        .filter(|column| column.primary_key)
        .collect::<Vec<_>>();
    primary.sort_by_key(|column| column.primary_key_position);
    if !primary.is_empty() && !primary.iter().any(|column| column.auto_increment) {
        let primary = primary
            .iter()
            .map(|column| column.name.clone())
            .collect::<Vec<_>>();
        definitions.push(format!(
            "PRIMARY KEY ({})",
            quote_identifiers(DatabaseBackend::Sqlite, &primary)
        ));
    }
    for index in &schema.indexes {
        if index.unique && !index.primary && index.read_only && !index.columns.is_empty() {
            definitions.push(format!(
                "UNIQUE ({})",
                quote_identifiers(DatabaseBackend::Sqlite, &index.columns)
            ));
        }
    }
    definitions.extend(
        foreign_keys
            .iter()
            .map(|foreign_key| foreign_key_sql(DatabaseBackend::Sqlite, foreign_key)),
    );
    let suffix = sqlite_table_suffix(&schema.sql);
    let quoted_table = quote_identifier(DatabaseBackend::Sqlite, table);
    let quoted_temp = quote_identifier(DatabaseBackend::Sqlite, &temp_table);
    let create_sql = format!(
        "CREATE TABLE {quoted_temp} (\n    {}\n) {suffix}",
        definitions.join(",\n    ")
    );
    let column_names = schema
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let column_list = quote_identifiers(DatabaseBackend::Sqlite, &column_names);
    let objects = connection
        .query::<String>(
            "SELECT sql FROM sqlite_master
             WHERE tbl_name = ? AND type IN ('index', 'trigger') AND sql IS NOT NULL ORDER BY type, name",
            table.to_string(),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let foreign_keys_enabled = connection.query_some::<i64>("PRAGMA foreign_keys", ())? != 0;
    if foreign_keys_enabled {
        connection.execute("PRAGMA foreign_keys=OFF", ())?;
    }
    let result = connection.transaction::<_, StatementError>(|transaction| {
        transaction.execute(&create_sql, ())?;
        transaction.execute(
            format!(
                "INSERT INTO {quoted_temp} ({column_list}) SELECT {column_list} FROM {quoted_table}"
            ),
            (),
        )?;
        transaction.execute(format!("DROP TABLE {quoted_table}"), ())?;
        transaction.execute(
            format!("ALTER TABLE {quoted_temp} RENAME TO {quoted_table}"),
            (),
        )?;
        for object in &objects {
            transaction.execute(object, ())?;
        }
        // New foreign keys must hold for the existing rows
        if check_foreign_keys
            && transaction
                .query::<String>(
                    format!(
                        "SELECT \"table\" FROM pragma_foreign_key_check({})",
                        quote_text(table)
                    ),
                    (),
                )?
                .next()
                .transpose()?
                .is_some()
        {
            return Err(StatementError::new(
                "Existing rows violate the foreign key constraints",
            ));
        }
        Ok(())
    });
    let restore_result = if foreign_keys_enabled {
        connection.execute("PRAGMA foreign_keys=ON", ()).map(|_| ())
    } else {
        Ok(())
    };
    result?;
    restore_result?;
    Ok(())
}

fn column_definition(
    backend: DatabaseBackend,
    column: &SchemaColumn,
    include_mysql_metadata: bool,
) -> Result<String, StatementError> {
    validate_column(column)?;
    let mut definition = format!(
        "{} {}",
        quote_identifier(backend, &column.name),
        column.r#type.trim()
    );
    if backend == DatabaseBackend::Mysql && include_mysql_metadata {
        if let Some(character_set) = &column.character_set {
            validate_fragment(character_set, "Character set")?;
            write!(definition, " CHARACTER SET {character_set}")
                .expect("writing to a string cannot fail");
        }
        if let Some(collation) = &column.collation {
            validate_fragment(collation, "Collation")?;
            write!(definition, " COLLATE {collation}").expect("writing to a string cannot fail");
        }
    }
    definition.push_str(if column.nullable {
        " NULL"
    } else {
        " NOT NULL"
    });
    if let Some(default_sql) = column.default_sql.as_deref() {
        validate_fragment(default_sql, "Default value")?;
        write!(definition, " DEFAULT {default_sql}").expect("writing to a string cannot fail");
    }
    if column.auto_increment {
        match backend {
            DatabaseBackend::Sqlite => {
                if !column.primary_key || !column.r#type.eq_ignore_ascii_case("INTEGER") {
                    return Err(StatementError::new(
                        "SQLite AUTOINCREMENT requires an INTEGER primary key",
                    ));
                }
                definition.push_str(" PRIMARY KEY AUTOINCREMENT");
            }
            DatabaseBackend::Mysql => definition.push_str(" AUTO_INCREMENT"),
        }
    }
    if backend == DatabaseBackend::Mysql && include_mysql_metadata {
        let extra = column.extra.to_ascii_lowercase();
        if let Some(on_update) = extra
            .split_once("on update ")
            .and_then(|(_, value)| value.split_ascii_whitespace().next())
        {
            validate_fragment(on_update, "ON UPDATE value")?;
            write!(definition, " ON UPDATE {on_update}").expect("writing to a string cannot fail");
        }
        if extra
            .split_ascii_whitespace()
            .any(|part| part == "invisible")
        {
            definition.push_str(" INVISIBLE");
        }
        if !column.comment.is_empty() {
            write!(definition, " COMMENT {}", quote_mysql_text(&column.comment))
                .expect("writing to a string cannot fail");
        }
    }
    Ok(definition)
}

fn validate_column(column: &SchemaColumn) -> Result<(), StatementError> {
    validate_identifier(&column.name, "Column name")?;
    validate_type_fragment(&column.r#type)?;
    if column.generated {
        return Err(StatementError::new("Generated columns are read-only"));
    }
    if let Some(default_sql) = &column.default_sql {
        validate_fragment(default_sql, "Default value")?;
    }
    Ok(())
}

fn validate_index(index: &SchemaIndex) -> Result<(), StatementError> {
    validate_identifier(&index.name, "Index name")?;
    if index.primary || index.read_only {
        return Err(StatementError::new("This index cannot be created manually"));
    }
    if index.columns.is_empty() {
        return Err(StatementError::new("Select at least one index column"));
    }
    for column in &index.columns {
        validate_identifier(column, "Index column")?;
    }
    Ok(())
}

fn create_index_sql(backend: DatabaseBackend, table: &str, index: &SchemaIndex) -> String {
    let unique = if index.unique { "UNIQUE " } else { "" };
    format!(
        "CREATE {unique}INDEX {} ON {} ({})",
        quote_identifier(backend, &index.name),
        quote_identifier(backend, table),
        quote_identifiers(backend, &index.columns)
    )
}

fn validate_identifier(value: &str, label: &str) -> Result<(), StatementError> {
    if value.trim().is_empty() {
        return Err(StatementError::new(format!("{label} is required")));
    }
    if value.contains(['\0', '\n', '\r']) {
        return Err(StatementError::new(format!(
            "{label} contains invalid characters"
        )));
    }
    Ok(())
}

fn validate_fragment(value: &str, label: &str) -> Result<(), StatementError> {
    if value.trim().is_empty() {
        return Err(StatementError::new(format!("{label} is required")));
    }
    if value.contains(';') || value.contains("--") || value.contains("/*") || value.contains("*/") {
        return Err(StatementError::new(format!(
            "{label} must be one SQL fragment"
        )));
    }
    Ok(())
}

fn validate_type_fragment(value: &str) -> Result<(), StatementError> {
    validate_fragment(value, "Column type")?;
    let mut depth = 0_i32;
    let mut quote = None;
    let mut characters = value.chars().peekable();
    while let Some(character) = characters.next() {
        if let Some(delimiter) = quote {
            if character == '\\' {
                characters.next();
            } else if character == delimiter {
                if characters.peek() == Some(&delimiter) {
                    characters.next();
                } else {
                    quote = None;
                }
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            ')' => {
                return Err(StatementError::new(
                    "Column type has an unmatched parenthesis",
                ));
            }
            ',' if depth == 0 => {
                return Err(StatementError::new("Column type must be one SQL type"));
            }
            _ => {}
        }
    }
    if depth != 0 || quote.is_some() {
        return Err(StatementError::new(
            "Column type has an unterminated quote or parenthesis",
        ));
    }
    Ok(())
}

fn sql_contains_word(sql: &str, word: &str) -> bool {
    sql.split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|part| part == word)
}

fn sqlite_table_suffix(sql: &str) -> &str {
    sql.rfind(')')
        .map_or("", |position| sql[position + 1..].trim())
}

#[cfg(test)]
mod tests {
    use bsql::{Connection, PoolOptions, SqliteMode};

    use super::{
        DatabaseBackend, SchemaChange, SchemaForeignKey, SchemaIndex, TableOperation,
        apply_schema_change, apply_table_operation, create_table, load_sqlite_schema,
        sqlite_table_body,
    };

    fn database() -> Connection {
        Connection::open_sqlite(
            ":memory:",
            SqliteMode::ReadWrite,
            PoolOptions::single_connection(),
        )
        .expect("database should open")
    }

    #[test]
    fn sqlite_column_rebuild_preserves_schema_objects_and_data() {
        let connection = database();
        connection
            .execute_script(
                "PRAGMA foreign_keys=ON;
                 CREATE TABLE parents (id INTEGER PRIMARY KEY);
                 CREATE TABLE audit (name TEXT);
                 CREATE TABLE items (
                     id INTEGER PRIMARY KEY AUTOINCREMENT,
                     parent_id INTEGER,
                     name TEXT NOT NULL DEFAULT 'original',
                     UNIQUE (name),
                     FOREIGN KEY (parent_id) REFERENCES parents (id) ON DELETE CASCADE
                 );
                 CREATE INDEX items_parent ON items (parent_id);
                 CREATE TRIGGER items_audit AFTER UPDATE ON items
                 BEGIN INSERT INTO audit (name) VALUES (NEW.name); END;
                 INSERT INTO parents VALUES (1);
                 INSERT INTO items (parent_id, name) VALUES (1, 'saved');",
            )
            .expect("fixture should be created");
        let schema = load_sqlite_schema(&connection, "items").expect("schema should load");
        let mut name = schema
            .columns
            .iter()
            .find(|column| column.name == "name")
            .expect("name column should exist")
            .clone();
        name.r#type = "VARCHAR(100)".to_string();
        name.nullable = true;
        name.default_sql = Some("'changed'".to_string());

        apply_schema_change(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "items",
            SchemaChange::UpdateColumn {
                old_name: "name".to_string(),
                column: name,
            },
        )
        .expect("column should update");

        let updated = load_sqlite_schema(&connection, "items").expect("updated schema should load");
        let name = updated
            .columns
            .iter()
            .find(|column| column.name == "name")
            .expect("name column should remain");
        assert_eq!(name.r#type, "VARCHAR(100)");
        assert!(name.nullable);
        assert_eq!(name.default_sql.as_deref(), Some("'changed'"));
        assert!(
            updated
                .indexes
                .iter()
                .any(|index| index.name == "items_parent")
        );
        assert!(updated.indexes.iter().any(|index| index.unique));
        assert_eq!(
            connection
                .query_some::<String>("SELECT name FROM items WHERE id = 1", ())
                .expect("row should remain"),
            "saved"
        );
        connection
            .execute("UPDATE items SET name = 'updated' WHERE id = 1", ())
            .expect("trigger should remain");
        assert_eq!(
            connection
                .query_some::<String>("SELECT name FROM audit", ())
                .expect("trigger should write audit row"),
            "updated"
        );
        connection
            .execute("DELETE FROM parents WHERE id = 1", ())
            .expect("foreign-key cascade should remain");
        assert_eq!(
            connection
                .query_some::<i64>("SELECT COUNT(*) FROM items", ())
                .expect("row count should load"),
            0
        );
    }

    #[test]
    fn sqlite_indexes_can_be_added_and_removed() {
        let connection = database();
        connection
            .execute("CREATE TABLE items (id INTEGER, name TEXT)", ())
            .expect("fixture should be created");
        apply_schema_change(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "items",
            SchemaChange::AddIndex {
                index: SchemaIndex {
                    name: "items_name".to_string(),
                    columns: vec!["name".to_string()],
                    unique: true,
                    primary: false,
                    read_only: false,
                },
            },
        )
        .expect("index should be added");
        assert!(
            load_sqlite_schema(&connection, "items")
                .expect("schema should load")
                .indexes
                .iter()
                .any(|index| index.name == "items_name" && index.unique)
        );
        apply_schema_change(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "items",
            SchemaChange::DropIndex {
                name: "items_name".to_string(),
            },
        )
        .expect("index should be removed");
        assert!(
            load_sqlite_schema(&connection, "items")
                .expect("schema should load")
                .indexes
                .is_empty()
        );
    }

    #[test]
    fn sqlite_index_can_be_edited_atomically() {
        let connection = database();
        connection
            .execute_script(
                "CREATE TABLE items (id INTEGER, name TEXT);
                 CREATE INDEX old_index ON items (id);",
            )
            .expect("fixture should be created");
        apply_schema_change(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "items",
            SchemaChange::UpdateIndex {
                old_name: "old_index".to_string(),
                index: SchemaIndex {
                    name: "new_index".to_string(),
                    columns: vec!["name".to_string()],
                    unique: true,
                    primary: false,
                    read_only: false,
                },
            },
        )
        .expect("index should update");
        let indexes = load_sqlite_schema(&connection, "items")
            .expect("schema should load")
            .indexes;
        assert_eq!(indexes.len(), 1);
        assert_eq!(indexes[0].name, "new_index");
        assert_eq!(indexes[0].columns, ["name"]);
        assert!(indexes[0].unique);
    }

    #[test]
    fn sqlite_primary_key_can_be_changed() {
        let connection = database();
        connection
            .execute_script(
                "CREATE TABLE items (old_id INTEGER PRIMARY KEY, new_id INTEGER NOT NULL UNIQUE);
                 INSERT INTO items VALUES (1, 10);",
            )
            .expect("fixture should be created");
        let schema = load_sqlite_schema(&connection, "items").expect("schema should load");
        let mut old_id = schema.columns[0].clone();
        old_id.primary_key = false;
        old_id.primary_key_position = 0;
        apply_schema_change(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "items",
            SchemaChange::UpdateColumn {
                old_name: "old_id".to_string(),
                column: old_id,
            },
        )
        .expect("old primary key should be removed");
        let schema = load_sqlite_schema(&connection, "items").expect("schema should reload");
        let mut new_id = schema.columns[1].clone();
        new_id.primary_key = true;
        new_id.primary_key_position = 1;
        apply_schema_change(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "items",
            SchemaChange::UpdateColumn {
                old_name: "new_id".to_string(),
                column: new_id,
            },
        )
        .expect("new primary key should be added");
        let schema = load_sqlite_schema(&connection, "items").expect("schema should reload");
        assert!(!schema.columns[0].primary_key);
        assert!(schema.columns[1].primary_key);
        assert_eq!(
            connection
                .query_some::<(i64, i64)>("SELECT old_id, new_id FROM items", ())
                .expect("data should remain"),
            (1, 10)
        );
    }

    #[test]
    fn sqlite_table_body_skips_quoted_names() {
        assert_eq!(
            sqlite_table_body(r#"CREATE TABLE "odd (name" (id INTEGER)"#),
            Some("(id INTEGER)")
        );
        assert_eq!(sqlite_table_body("CREATE TABLE [a(b] (x)"), Some("(x)"));
    }

    #[test]
    fn sqlite_tables_can_be_created_duplicated_truncated_and_dropped() {
        let connection = database();
        create_table(&connection, DatabaseBackend::Sqlite, "items")
            .expect("table should be created");
        connection
            .execute_script(
                "ALTER TABLE items ADD COLUMN name TEXT;
                 CREATE INDEX items_name ON items (name);
                 INSERT INTO items (name) VALUES ('a'), ('b');",
            )
            .expect("fixture should be created");
        let run = |table: &str, operation| {
            apply_table_operation(&connection, DatabaseBackend::Sqlite, None, table, operation)
        };

        let copy = run(
            "items",
            TableOperation::Duplicate {
                new_name: "items_copy".to_string(),
                with_data: true,
            },
        )
        .expect("table should be duplicated");
        assert_eq!(copy, "items_copy");
        let schema = load_sqlite_schema(&connection, "items_copy").expect("copy should exist");
        assert!(schema.columns[0].auto_increment);
        assert!(
            schema
                .indexes
                .iter()
                .any(|index| index.name == "items_copy_items_name")
        );
        let count = |table: &str| {
            connection
                .query_some::<i64>(format!("SELECT COUNT(*) FROM {table}"), ())
                .expect("count should load")
        };
        assert_eq!(count("items_copy"), 2);

        run("items", TableOperation::Truncate).expect("table should be truncated");
        assert_eq!(count("items"), 0);
        connection
            .execute("INSERT INTO items (name) VALUES ('c')", ())
            .expect("row should insert");
        assert_eq!(
            connection
                .query_some::<i64>("SELECT id FROM items", ())
                .expect("id should load"),
            1,
            "truncate should reset the AUTOINCREMENT counter"
        );

        run("items_copy", TableOperation::Drop).expect("table should be dropped");
        assert!(load_sqlite_schema(&connection, "items_copy").is_err());
    }

    #[test]
    fn sqlite_foreign_keys_can_be_added_and_removed() {
        let connection = database();
        connection
            .execute_script(
                "CREATE TABLE parents (id INTEGER PRIMARY KEY);
                 CREATE TABLE items (id INTEGER PRIMARY KEY, parent_id INTEGER);
                 CREATE INDEX items_parent ON items (parent_id);
                 INSERT INTO parents VALUES (1);
                 INSERT INTO items VALUES (1, 1), (2, 2);",
            )
            .expect("fixture should be created");
        let foreign_key = SchemaForeignKey {
            name: String::new(),
            columns: vec!["parent_id".to_string()],
            referenced_table: "parents".to_string(),
            referenced_columns: vec!["id".to_string()],
            on_update: "NO ACTION".to_string(),
            on_delete: "CASCADE".to_string(),
        };
        let add = || {
            apply_schema_change(
                &connection,
                DatabaseBackend::Sqlite,
                None,
                "items",
                SchemaChange::AddForeignKey {
                    foreign_key: foreign_key.clone(),
                },
            )
        };

        let error = add().expect_err("orphaned rows should be rejected");
        assert!(error.to_string().contains("violate"), "{error}");
        assert!(
            load_sqlite_schema(&connection, "items")
                .expect("schema should load")
                .foreign_keys
                .is_empty()
        );

        connection
            .execute("DELETE FROM items WHERE id = 2", ())
            .expect("orphan should be deleted");
        add().expect("foreign key should be added");
        let schema = load_sqlite_schema(&connection, "items").expect("schema should load");
        assert_eq!(schema.foreign_keys.len(), 1);
        assert_eq!(schema.foreign_keys[0].on_delete, "CASCADE");
        assert!(
            schema
                .indexes
                .iter()
                .any(|index| index.name == "items_parent")
        );

        apply_schema_change(
            &connection,
            DatabaseBackend::Sqlite,
            None,
            "items",
            SchemaChange::DropForeignKey {
                name: schema.foreign_keys[0].name.clone(),
            },
        )
        .expect("foreign key should be removed");
        assert!(
            load_sqlite_schema(&connection, "items")
                .expect("schema should load")
                .foreign_keys
                .is_empty()
        );
        assert_eq!(
            connection
                .query_some::<i64>("SELECT COUNT(*) FROM items", ())
                .expect("rows should remain"),
            1
        );
    }
}
