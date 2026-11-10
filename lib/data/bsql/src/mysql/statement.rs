/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::connection::InnerConnection;
use crate::statement::PreparedStatement;
use crate::{ColumnType, StatementError, Value};

#[derive(Debug, Clone)]
pub(crate) struct Column {
    pub(crate) name: String,
    pub(crate) table: Option<String>,
    pub(crate) origin_name: Option<String>,
    pub(crate) type_code: u8,
    pub(crate) flags: super::ColumnFlags,
    pub(crate) decimals: u8,
    pub(crate) charset: u16,
}

impl Column {
    pub(crate) fn declared_type(&self) -> String {
        super::mysql_type_name(self.type_code, self.charset).to_string()
    }
}

pub(crate) struct Prepared {
    pub(crate) id: u32,
    pub(crate) query: String,
    pub(crate) parameter_names: Vec<Option<String>>,
    pub(crate) params: Vec<Option<Value>>,
    pub(crate) columns: Vec<Column>,
    pub(crate) rows: Vec<Vec<Value>>,
    pub(crate) row_index: usize,
    pub(crate) current_row: Option<usize>,
    pub(crate) executed: bool,
}

impl PreparedStatement for Prepared {
    fn reset(&mut self, _connection: &mut InnerConnection) -> Result<(), StatementError> {
        // Results are fully buffered and long data is never sent, so no COM_STMT_RESET is needed.
        self.rows.clear();
        self.row_index = 0;
        self.current_row = None;
        self.executed = false;
        Ok(())
    }

    fn bind_value(&mut self, index: i32, value: Value) -> Result<(), StatementError> {
        let index = usize::try_from(index)
            .map_err(|_| StatementError::new(format!("parameter index {index} is out of range")))?;
        let slot = self.params.get_mut(index).ok_or_else(|| {
            StatementError::new(format!("parameter index {index} is out of range"))
        })?;
        *slot = Some(value);
        self.executed = false;
        Ok(())
    }

    fn bind_named_value(&mut self, name: &str, value: Value) -> Result<(), StatementError> {
        let mut found = false;
        for (index, parameter_name) in self.parameter_names.iter().enumerate() {
            if parameter_name.as_deref() == Some(name) {
                self.params[index] = Some(value.clone());
                found = true;
            }
        }
        if !found {
            return Err(StatementError::new(format!(
                "Parameter '{name}' not found in statement"
            )));
        }
        self.executed = false;
        Ok(())
    }

    fn step(&mut self, connection: &mut InnerConnection) -> Result<Option<()>, StatementError> {
        let client = connection.mysql_mut()?;
        if !self.executed {
            client.execute_prepared(self)?;
        }
        if self.row_index < self.rows.len() {
            self.current_row = Some(self.row_index);
            self.row_index += 1;
            Ok(Some(()))
        } else {
            self.current_row = None;
            Ok(None)
        }
    }

    fn column_count(&self) -> i32 {
        self.columns.len() as i32
    }
    fn column_name(&self, index: i32) -> String {
        self.columns[index as usize].name.clone()
    }
    fn column_type(&self, index: i32) -> ColumnType {
        column_type(&self.rows[self.current_row.expect("current row checked")][index as usize])
    }
    fn column_declared_type(&self, index: i32) -> Option<String> {
        Some(self.columns[index as usize].declared_type())
    }
    fn column_table_name(&self, index: i32) -> Option<String> {
        self.columns[index as usize].table.clone()
    }
    fn column_origin_name(&self, index: i32) -> Option<String> {
        self.columns[index as usize].origin_name.clone()
    }
    fn column_value(&self, index: i32) -> Value {
        self.rows[self.current_row.expect("current row checked")][index as usize].clone()
    }
    fn close(&mut self, connection: &mut InnerConnection) -> Result<(), StatementError> {
        connection.mysql_mut()?.close(self.id)
    }
}

pub(crate) const fn column_type(value: &Value) -> ColumnType {
    match value {
        Value::Null => ColumnType::Null,
        Value::Integer(_) => ColumnType::Integer,
        Value::Float(_) => ColumnType::Float,
        Value::Text(_) => ColumnType::Text,
        #[cfg(feature = "chrono")]
        Value::Date(_) | Value::DateTime(_) => ColumnType::Text,
        Value::Blob(_) => ColumnType::Blob,
    }
}
