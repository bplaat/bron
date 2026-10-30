/*
 * Copyright (c) 2024-2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use std::ffi::{CStr, CString};

use libsqlite3_sys::*;

use super::utils::optional_string;
#[cfg(test)]
use crate::connection::Connection as GenericConnection;
use crate::connection::InnerConnection;
use crate::statement::PreparedStatement;
use crate::{ColumnType, StatementError, Value};

pub(crate) struct Prepared(pub(super) *mut sqlite3_stmt);

impl Prepared {
    fn finalize(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: self.0 is exclusively owned and finalized exactly once.
        unsafe { sqlite3_finalize(self.0) };
        self.0 = std::ptr::null_mut();
    }

    pub(crate) fn is_read_only(&self) -> bool {
        // SAFETY: self.0 is a valid prepared statement.
        unsafe { sqlite3_stmt_readonly(self.0) != 0 }
    }

    pub(crate) fn reset(&mut self) {
        // SAFETY: self.0 is a valid prepared statement.
        unsafe { sqlite3_reset(self.0) };
    }

    pub(crate) fn bind_value(&mut self, index: i32, value: Value) -> Result<(), StatementError> {
        let index = index + 1;
        let result = match value {
            // SAFETY: self.0 is valid and index is a SQLite one-based parameter index.
            Value::Null => unsafe { sqlite3_bind_null(self.0, index) },
            // SAFETY: self.0 is valid and index is a SQLite one-based parameter index.
            Value::Integer(value) => unsafe { sqlite3_bind_int64(self.0, index, value) },
            // SAFETY: self.0 is valid and index is a SQLite one-based parameter index.
            Value::Float(value) => unsafe { sqlite3_bind_double(self.0, index, value) },
            Value::Text(value) => {
                let len = i32::try_from(value.len())
                    .map_err(|_| StatementError::new("text value is too large for SQLite"))?;
                // SAFETY: SQLite copies the valid value bytes because SQLITE_TRANSIENT is used.
                unsafe {
                    sqlite3_bind_text(
                        self.0,
                        index,
                        value.as_ptr().cast(),
                        len,
                        SQLITE_TRANSIENT(),
                    )
                }
            }
            Value::Blob(value) => {
                let len = i32::try_from(value.len())
                    .map_err(|_| StatementError::new("blob value is too large for SQLite"))?;
                // SAFETY: SQLite copies the valid value bytes because SQLITE_TRANSIENT is used.
                unsafe {
                    sqlite3_bind_blob(
                        self.0,
                        index,
                        value.as_ptr().cast(),
                        len,
                        SQLITE_TRANSIENT(),
                    )
                }
            }
        };
        if result == SQLITE_OK {
            Ok(())
        } else {
            Err(self.error("bind value to"))
        }
    }

    pub(crate) fn bind_named_value(
        &mut self,
        name: &str,
        value: Value,
    ) -> Result<(), StatementError> {
        let c_name = CString::new(name)
            .map_err(|_| StatementError::new("SQLite parameter name contains a null byte"))?;
        // SAFETY: self.0 and c_name are valid.
        let index = unsafe { sqlite3_bind_parameter_index(self.0, c_name.as_ptr()) };
        if index == 0 {
            return Err(StatementError::new(format!(
                "Parameter '{name}' not found in statement"
            )));
        }
        self.bind_value(index - 1, value)
    }

    pub(crate) fn step(&mut self) -> Result<Option<()>, StatementError> {
        // SAFETY: self.0 is a valid prepared statement.
        match unsafe { sqlite3_step(self.0) } {
            SQLITE_ROW => Ok(Some(())),
            SQLITE_DONE => Ok(None),
            _ => Err(self.error("step")),
        }
    }

    pub(crate) fn column_count(&self) -> i32 {
        // SAFETY: self.0 is a valid prepared statement.
        unsafe { sqlite3_column_count(self.0) }
    }

    pub(crate) fn column_name(&self, index: i32) -> String {
        // SAFETY: self.0 is valid and the caller checked index.
        let name = unsafe { sqlite3_column_name(self.0, index) };
        assert!(!name.is_null(), "SQLite returned a null column name");
        // SAFETY: name is non-null and SQLite owns it for the statement lifetime.
        unsafe { CStr::from_ptr(name) }
            .to_string_lossy()
            .into_owned()
    }

    pub(crate) fn column_type(&self, index: i32) -> ColumnType {
        // SAFETY: self.0 is valid and the caller checked index.
        match unsafe { sqlite3_column_type(self.0, index) } {
            SQLITE_NULL => ColumnType::Null,
            SQLITE_INTEGER => ColumnType::Integer,
            SQLITE_FLOAT => ColumnType::Float,
            SQLITE_TEXT => ColumnType::Text,
            SQLITE_BLOB => ColumnType::Blob,
            value_type => unreachable!("unknown SQLite column type: {value_type}"),
        }
    }

    pub(crate) fn column_declared_type(&self, index: i32) -> Option<String> {
        // SAFETY: self.0 is valid and the caller checked index.
        optional_string(unsafe { sqlite3_column_decltype(self.0, index) })
    }

    pub(crate) fn column_table_name(&self, index: i32) -> Option<String> {
        // SAFETY: self.0 is valid and the caller checked index.
        optional_string(unsafe { sqlite3_column_table_name(self.0, index) })
    }

    pub(crate) fn column_origin_name(&self, index: i32) -> Option<String> {
        // SAFETY: self.0 is valid and the caller checked index.
        optional_string(unsafe { sqlite3_column_origin_name(self.0, index) })
    }

    pub(crate) fn column_value(&self, index: i32) -> Value {
        // SAFETY: self.0 points to the current row and the caller checked index.
        match unsafe { sqlite3_column_type(self.0, index) } {
            SQLITE_NULL => Value::Null,
            // SAFETY: the current column is an integer.
            SQLITE_INTEGER => Value::Integer(unsafe { sqlite3_column_int64(self.0, index) }),
            // SAFETY: the current column is a float.
            SQLITE_FLOAT => Value::Float(unsafe { sqlite3_column_double(self.0, index) }),
            SQLITE_TEXT => {
                // SAFETY: this reads the byte size of the same current text column.
                let len = unsafe { sqlite3_column_bytes(self.0, index) } as usize;
                if len == 0 {
                    return Value::Text(String::new());
                }
                // SAFETY: the current column is text.
                let text = unsafe { sqlite3_column_text(self.0, index) };
                assert!(!text.is_null(), "SQLite returned a null text value");
                // SAFETY: text is non-null and points to len readable bytes until the next call.
                let bytes = unsafe { std::slice::from_raw_parts(text, len) };
                Value::Text(String::from_utf8_lossy(bytes).into_owned())
            }
            SQLITE_BLOB => {
                // SAFETY: the current column is a blob.
                let blob = unsafe { sqlite3_column_blob(self.0, index) };
                if blob.is_null() {
                    return Value::Blob(Vec::new());
                }
                // SAFETY: this reads the size of the same current blob column.
                let len = unsafe { sqlite3_column_bytes(self.0, index) } as usize;
                // SAFETY: blob is non-null and points to len readable bytes.
                Value::Blob(unsafe { std::slice::from_raw_parts(blob.cast::<u8>(), len) }.to_vec())
            }
            value_type => unreachable!("unknown SQLite column type: {value_type}"),
        }
    }

    fn error(&self, operation: &str) -> StatementError {
        // SAFETY: self.0 is a valid prepared statement and SQLite owns the returned string.
        let query = unsafe { CStr::from_ptr(sqlite3_sql(self.0)) }.to_string_lossy();
        // SAFETY: self.0 has a valid database handle and SQLite owns the returned string.
        let message =
            unsafe { CStr::from_ptr(sqlite3_errmsg(sqlite3_db_handle(self.0))) }.to_string_lossy();
        StatementError::new(format!(
            "failed to {operation} SQLite statement '{query}': {message}"
        ))
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        self.finalize();
    }
}

impl PreparedStatement for Prepared {
    fn reset(&mut self, _connection: &mut InnerConnection) -> Result<(), StatementError> {
        self.reset();
        Ok(())
    }
    fn bind_value(&mut self, index: i32, value: Value) -> Result<(), StatementError> {
        self.bind_value(index, value)
    }
    fn bind_named_value(&mut self, name: &str, value: Value) -> Result<(), StatementError> {
        self.bind_named_value(name, value)
    }
    fn step(&mut self, _connection: &mut InnerConnection) -> Result<Option<()>, StatementError> {
        self.step()
    }
    fn column_count(&self) -> i32 {
        self.column_count()
    }
    fn column_name(&self, index: i32) -> String {
        self.column_name(index)
    }
    fn column_type(&self, index: i32) -> ColumnType {
        self.column_type(index)
    }
    fn column_declared_type(&self, index: i32) -> Option<String> {
        self.column_declared_type(index)
    }
    fn column_table_name(&self, index: i32) -> Option<String> {
        self.column_table_name(index)
    }
    fn column_origin_name(&self, index: i32) -> Option<String> {
        self.column_origin_name(index)
    }
    fn column_value(&self, index: i32) -> Value {
        self.column_value(index)
    }
    fn close(&mut self, _connection: &mut InnerConnection) -> Result<(), StatementError> {
        self.finalize();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_crud_and_statement_metadata() -> Result<(), StatementError> {
        let database = GenericConnection::open_sqlite_memory().expect("open SQLite");
        database.execute(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT, payload BLOB) STRICT",
            (),
        )?;
        database.execute(
            "INSERT INTO items (id, name, payload) VALUES (?, ?, ?)",
            (1_i64, "item".to_string(), vec![1_u8, 2, 3]),
        )?;
        let mut statement = database.prepare::<()>("SELECT id, name, payload FROM items")?;
        assert_eq!(statement.column_count(), 3);
        assert_eq!(statement.column_name(0), "id");
        assert_eq!(statement.step()?, Some(()));
        assert_eq!(statement.column_type(0), ColumnType::Integer);
        assert_eq!(statement.column_value(1), Value::Text("item".to_string()));
        drop(statement);

        let text_with_null = "before\0after".to_string();
        database.execute("CREATE TABLE texts (value TEXT) STRICT", ())?;
        database.execute("INSERT INTO texts VALUES (?)", text_with_null.clone())?;
        assert_eq!(
            database.query_some::<String>("SELECT value FROM texts", ())?,
            text_with_null
        );
        assert!(database.prepare::<()>("-- no statement").is_err());
        Ok(())
    }
}
