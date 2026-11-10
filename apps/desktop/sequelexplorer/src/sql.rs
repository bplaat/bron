/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use bsql::DatabaseType;

// MARK: Quoting
pub(crate) fn quote_identifier(backend: DatabaseType, identifier: &str) -> String {
    match backend {
        DatabaseType::Sqlite => format!("\"{}\"", identifier.replace('"', "\"\"")),
        DatabaseType::Mysql => format!("`{}`", identifier.replace('`', "``")),
    }
}

pub(crate) fn quote_identifiers(backend: DatabaseType, identifiers: &[String]) -> String {
    identifiers
        .iter()
        .map(|identifier| quote_identifier(backend, identifier))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn quote_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(crate) fn quote_mysql_text(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}

// MARK: Statement splitting
pub(crate) struct SqlStatement {
    pub(crate) sql: String,
    pub(crate) words: Vec<String>,
    pub(crate) has_executable_comment: bool,
}

impl SqlStatement {
    pub(crate) fn starts_with(&self, keyword: &str) -> bool {
        self.words
            .first()
            .is_some_and(|word| word.eq_ignore_ascii_case(keyword))
    }

    pub(crate) fn is_transaction_control(&self) -> bool {
        [
            "BEGIN",
            "COMMIT",
            "ROLLBACK",
            "START",
            "SAVEPOINT",
            "RELEASE",
            "END",
        ]
        .iter()
        .any(|keyword| self.starts_with(keyword))
    }
}

/// Split a SQL script into statements, ignoring semicolons inside quotes and comments
pub(crate) fn split_statements(backend: DatabaseType, sql: &str) -> Vec<SqlStatement> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum State {
        Normal,
        SingleQuote,
        DoubleQuote,
        Backtick,
        LineComment,
        BlockComment,
    }

    let is_mysql = match backend {
        DatabaseType::Sqlite => false,
        DatabaseType::Mysql => true,
    };
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut words = Vec::new();
    let mut word = String::new();
    let mut has_executable_comment = false;
    let mut state = State::Normal;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        match state {
            State::Normal if byte.is_ascii_alphanumeric() || byte == b'_' => {
                word.push(char::from(byte));
            }
            State::Normal => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                match byte {
                    b';' => {
                        push_statement(
                            &mut statements,
                            &sql[start..index],
                            &mut words,
                            &mut has_executable_comment,
                        );
                        start = index + 1;
                    }
                    b'\'' => state = State::SingleQuote,
                    b'"' => state = State::DoubleQuote,
                    b'`' => state = State::Backtick,
                    b'#' if is_mysql => state = State::LineComment,
                    b'-' if bytes.get(index + 1) == Some(&b'-') => {
                        state = State::LineComment;
                        index += 1;
                    }
                    b'/' if bytes.get(index + 1) == Some(&b'*') => {
                        if is_mysql && bytes.get(index + 2) == Some(&b'!') {
                            has_executable_comment = true;
                        }
                        state = State::BlockComment;
                        index += 1;
                    }
                    _ => {}
                }
            }
            State::SingleQuote if is_mysql && byte == b'\\' => index += 1,
            State::SingleQuote | State::DoubleQuote | State::Backtick => {
                let delimiter = match state {
                    State::SingleQuote => b'\'',
                    State::DoubleQuote => b'"',
                    _ => b'`',
                };
                if byte == delimiter {
                    if bytes.get(index + 1) == Some(&delimiter) {
                        index += 1;
                    } else {
                        state = State::Normal;
                    }
                }
            }
            State::LineComment if byte == b'\n' => state = State::Normal,
            State::BlockComment if byte == b'*' && bytes.get(index + 1) == Some(&b'/') => {
                state = State::Normal;
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }
    if !word.is_empty() {
        words.push(word);
    }
    push_statement(
        &mut statements,
        &sql[start..],
        &mut words,
        &mut has_executable_comment,
    );
    statements
}

fn push_statement(
    statements: &mut Vec<SqlStatement>,
    sql: &str,
    words: &mut Vec<String>,
    has_executable_comment: &mut bool,
) {
    if !words.is_empty() {
        statements.push(SqlStatement {
            sql: sql.trim().to_string(),
            words: std::mem::take(words),
            has_executable_comment: *has_executable_comment,
        });
    }
    *has_executable_comment = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_ignores_semicolons_in_quotes_and_comments() {
        let statements = split_statements(
            DatabaseType::Sqlite,
            "SELECT ';' AS a; -- comment; here\nINSERT INTO \"t;\" VALUES (1); /* ; */ ;",
        );
        assert_eq!(statements.len(), 2);
        assert_eq!(statements[0].sql, "SELECT ';' AS a");
        assert!(statements[1].starts_with("insert"));
    }

    #[test]
    fn split_uses_backend_string_escapes() {
        // SQLite has no backslash escapes, so the string ends after the backslash
        let sqlite = split_statements(DatabaseType::Sqlite, r"SELECT 'C:\'; SELECT 2");
        assert_eq!(sqlite.len(), 2);
        let mysql = split_statements(DatabaseType::Mysql, r"SELECT 'it\'s; fine'; # x;");
        assert_eq!(mysql.len(), 1);
        assert!(
            split_statements(DatabaseType::Mysql, "SELECT /*! 1 */ 2")[0].has_executable_comment
        );
    }

    #[test]
    fn transaction_control_is_detected() {
        let statements = split_statements(DatabaseType::Sqlite, "BEGIN; select 1; COMMIT");
        assert!(statements[0].is_transaction_control());
        assert!(!statements[1].is_transaction_control());
        assert!(statements[2].is_transaction_control());
    }
}
