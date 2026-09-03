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

use crate::database::{DatabaseBackend, State};

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TableSchema {
    name: String,
    sql: String,
    columns: Vec<SchemaColumn>,
    indexes: Vec<SchemaIndex>,
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
}

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
    let result = match backend {
        DatabaseBackend::Sqlite => load_sqlite_schema(&connection, table),
        DatabaseBackend::Mysql => load_mysql_schema(
            &connection,
            database
                .as_deref()
                .expect("MySQL database should be selected"),
            table,
        ),
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

fn error_response(error: impl Into<String>) -> Response {
    Response::with_json(json!({ "error": error.into() }))
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
    Ok(TableSchema {
        name: table.to_string(),
        sql,
        columns,
        indexes,
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
    Ok(TableSchema {
        name: table.to_string(),
        sql,
        columns,
        indexes,
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
                            quoted_columns(backend, &index.columns)
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
    }
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
                rebuild_sqlite_table(connection, table, &schema, &column.name, column)?;
            }
        }
    }
    Ok(())
}

fn rebuild_sqlite_table(
    connection: &Connection,
    table: &str,
    schema: &TableSchema,
    old_name: &str,
    replacement: &SchemaColumn,
) -> Result<(), StatementError> {
    let upper_sql = schema.sql.to_ascii_uppercase();
    if sql_contains_word(&upper_sql, "CHECK") || sql_contains_word(&upper_sql, "COLLATE") {
        return Err(StatementError::new(
            "Changing this SQLite column is not supported because the table contains CHECK or COLLATE clauses",
        ));
    }
    if schema.columns.iter().any(|column| column.generated) {
        return Err(StatementError::new(
            "Changing column definitions on SQLite tables with generated columns is not supported",
        ));
    }
    let temp_table = format!("__sequel_explorer_{table}");
    let mut definitions = schema
        .columns
        .iter()
        .map(|column| {
            column_definition(
                DatabaseBackend::Sqlite,
                if column.name == old_name {
                    replacement
                } else {
                    column
                },
                false,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut primary = schema
        .columns
        .iter()
        .map(|column| {
            if column.name == old_name {
                replacement
            } else {
                column
            }
        })
        .filter(|column| column.primary_key)
        .collect::<Vec<_>>();
    primary.sort_by_key(|column| column.primary_key_position);
    if !primary.is_empty() && !primary.iter().any(|column| column.auto_increment) {
        definitions.push(format!(
            "PRIMARY KEY ({})",
            primary
                .iter()
                .map(|column| quote_identifier(DatabaseBackend::Sqlite, &column.name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    append_sqlite_unique_constraints(connection, table, old_name, replacement, &mut definitions)?;
    append_sqlite_foreign_keys(connection, table, old_name, replacement, &mut definitions)?;
    let suffix = sqlite_table_suffix(&schema.sql);
    let create_sql = format!(
        "CREATE TABLE {} (\n    {}\n) {suffix}",
        quote_identifier(DatabaseBackend::Sqlite, &temp_table),
        definitions.join(",\n    ")
    );
    let source_columns = schema
        .columns
        .iter()
        .map(|column| quote_identifier(DatabaseBackend::Sqlite, &column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let target_columns = schema
        .columns
        .iter()
        .map(|column| {
            quote_identifier(
                DatabaseBackend::Sqlite,
                if column.name == old_name {
                    &replacement.name
                } else {
                    &column.name
                },
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let objects = connection
        .query::<String>(
            "SELECT sql FROM sqlite_master
             WHERE tbl_name = ? AND type IN ('index', 'trigger') AND sql IS NOT NULL ORDER BY type, name",
            table.to_string(),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let quoted_table = quote_identifier(DatabaseBackend::Sqlite, table);
    let quoted_temp = quote_identifier(DatabaseBackend::Sqlite, &temp_table);
    let foreign_keys_enabled = connection.query_some::<i64>("PRAGMA foreign_keys", ())? != 0;
    if foreign_keys_enabled {
        connection.execute("PRAGMA foreign_keys=OFF", ())?;
    }
    let result = connection.transaction::<_, StatementError>(|transaction| {
        transaction.execute(&create_sql, ())?;
        transaction.execute(
            format!(
                "INSERT INTO {quoted_temp} ({target_columns}) SELECT {source_columns} FROM {quoted_table}"
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

fn append_sqlite_unique_constraints(
    connection: &Connection,
    table: &str,
    old_name: &str,
    replacement: &SchemaColumn,
    definitions: &mut Vec<String>,
) -> Result<(), StatementError> {
    for index in load_sqlite_indexes(connection, table)? {
        if !index.unique || index.primary || !index.read_only || index.columns.is_empty() {
            continue;
        }
        definitions.push(format!(
            "UNIQUE ({})",
            index
                .columns
                .iter()
                .map(|column| {
                    quote_identifier(
                        DatabaseBackend::Sqlite,
                        if column == old_name {
                            &replacement.name
                        } else {
                            column
                        },
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    Ok(())
}

fn append_sqlite_foreign_keys(
    connection: &Connection,
    table: &str,
    old_name: &str,
    replacement: &SchemaColumn,
    definitions: &mut Vec<String>,
) -> Result<(), StatementError> {
    let rows = connection
        .query::<(i64, i64, String, String, String, String, String, String)>(
            format!(
                "SELECT id, seq, \"table\", \"from\", \"to\", on_update, on_delete, \"match\"
                 FROM pragma_foreign_key_list({}) ORDER BY id, seq",
                quote_text(table)
            ),
            (),
        )?
        .collect::<Result<Vec<_>, _>>()?;
    let mut groups = BTreeMap::<i64, Vec<_>>::new();
    for row in rows {
        groups.entry(row.0).or_default().push(row);
    }
    for rows in groups.into_values() {
        let local = rows
            .iter()
            .map(|row| {
                quote_identifier(
                    DatabaseBackend::Sqlite,
                    if row.3 == old_name {
                        &replacement.name
                    } else {
                        &row.3
                    },
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let foreign = rows
            .iter()
            .map(|row| quote_identifier(DatabaseBackend::Sqlite, &row.4))
            .collect::<Vec<_>>()
            .join(", ");
        let mut definition = format!(
            "FOREIGN KEY ({local}) REFERENCES {} ({foreign})",
            quote_identifier(DatabaseBackend::Sqlite, &rows[0].2)
        );
        if rows[0].5 != "NO ACTION" {
            write!(definition, " ON UPDATE {}", rows[0].5)
                .expect("writing to a string cannot fail");
        }
        if rows[0].6 != "NO ACTION" {
            write!(definition, " ON DELETE {}", rows[0].6)
                .expect("writing to a string cannot fail");
        }
        if rows[0].7 != "NONE" {
            write!(definition, " MATCH {}", rows[0].7).expect("writing to a string cannot fail");
        }
        definitions.push(definition);
    }
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
        quoted_columns(backend, &index.columns)
    )
}

fn quoted_columns(backend: DatabaseBackend, columns: &[String]) -> String {
    columns
        .iter()
        .map(|column| quote_identifier(backend, column))
        .collect::<Vec<_>>()
        .join(", ")
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

fn quote_identifier(backend: DatabaseBackend, identifier: &str) -> String {
    match backend {
        DatabaseBackend::Sqlite => format!("\"{}\"", identifier.replace('"', "\"\"")),
        DatabaseBackend::Mysql => format!("`{}`", identifier.replace('`', "``")),
    }
}

fn quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn quote_mysql_text(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use bsql::{Connection, PoolOptions, SqliteMode};

    use super::{
        DatabaseBackend, SchemaChange, SchemaIndex, apply_schema_change, load_sqlite_schema,
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
}
