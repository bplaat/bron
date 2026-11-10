/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

use crate::{Connection, DatabaseType};

/// Preprocess a search string into a SQLite FTS5 expression.
/// Words are prefixes, with uppercase AND, OR and binary NOT operators.
/// SQLite column filters such as `title:Alice` are preserved.
/// Use `Connection::preprocess_fts_query` to target the connection's database.
pub fn preprocess_fts_query(query: &str) -> String {
    compile(query, DatabaseType::Sqlite, true)
}

impl Connection {
    /// Translate the shared word-prefix search syntax to this database's syntax.
    /// Whitespace between words requires both. Uppercase AND, OR and binary NOT
    /// are supported. Precedence is whitespace, NOT, AND, then OR.
    /// Quotes and parentheses are ignored; other punctuation separates words.
    /// Leading, trailing and consecutive operators are ignored.
    pub fn preprocess_fts_query(&self, query: &str) -> String {
        compile(query, self.database_type(), false)
    }
}

// Ordered from lowest to highest precedence, matching FTS5.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Operator {
    Or,
    And,
    Not,
    ImplicitAnd,
}

fn compile(query: &str, database_type: DatabaseType, column_filters: bool) -> String {
    let mut values = Vec::new();
    let mut operators = Vec::new();
    let mut pending = None;
    for token in query.split_whitespace() {
        let operator = match token {
            "AND" => Some(Operator::And),
            "OR" => Some(Operator::Or),
            "NOT" => Some(Operator::Not),
            _ => None,
        };
        if let Some(operator) = operator {
            if !values.is_empty() && pending.is_none() {
                pending = Some(operator);
            }
            continue;
        }
        let clean = token.replace(['"', '(', ')'], "");
        for word in clean
            .split(|c: char| !c.is_alphanumeric() && c != '_' && !(column_filters && c == ':'))
            .filter(|word| !word.is_empty())
        {
            if !values.is_empty() {
                let operator = pending.take().unwrap_or(Operator::ImplicitAnd);
                while operators
                    .last()
                    .is_some_and(|previous| *previous >= operator)
                {
                    reduce(
                        &mut values,
                        operators.pop().expect("operator exists"),
                        database_type,
                    );
                }
                operators.push(operator);
            }
            let text = match database_type {
                DatabaseType::Sqlite if matches!(word, "AND" | "OR" | "NOT") => {
                    format!("\"{word}\"*")
                }
                DatabaseType::Sqlite | DatabaseType::Mysql => format!("{word}*"),
            };
            values.push(Expression {
                text,
                operator: None,
            });
        }
    }
    while let Some(operator) = operators.pop() {
        reduce(&mut values, operator, database_type);
    }
    values
        .pop()
        .map(|value| value.text)
        .unwrap_or_else(|| match database_type {
            DatabaseType::Sqlite => "\"\"".to_string(),
            DatabaseType::Mysql => String::new(),
        })
}

struct Expression {
    text: String,
    operator: Option<Operator>,
}

impl Expression {
    fn sqlite_operand(self, parent: Operator, right: bool) -> String {
        if self.operator.is_some_and(|operator| {
            operator < parent || (right && operator == Operator::Not && parent == Operator::Not)
        }) {
            format!("({})", self.text)
        } else {
            self.text
        }
    }

    fn mysql_required(self) -> String {
        match self.operator {
            Some(Operator::And | Operator::ImplicitAnd | Operator::Not) => self.text,
            Some(Operator::Or) => format!("+({})", self.text),
            None => format!("+{}", self.text),
        }
    }

    fn mysql_alternative(self) -> String {
        match self.operator {
            Some(Operator::Or) | None => self.text,
            Some(Operator::And | Operator::ImplicitAnd | Operator::Not) => {
                format!("({})", self.text)
            }
        }
    }
}

fn reduce(values: &mut Vec<Expression>, operator: Operator, database_type: DatabaseType) {
    let right = values.pop().expect("right search operand exists");
    let left = values.pop().expect("left search operand exists");
    // Implicit conjunctions are rendered explicitly after parsing their precedence.
    let operator = match operator {
        Operator::ImplicitAnd => Operator::And,
        operator => operator,
    };
    let text = match database_type {
        DatabaseType::Sqlite => {
            let left = left.sqlite_operand(operator, false);
            let right = right.sqlite_operand(operator, true);
            let operator = match operator {
                Operator::And | Operator::ImplicitAnd => "AND",
                Operator::Or => "OR",
                Operator::Not => "NOT",
            };
            format!("{left} {operator} {right}")
        }
        DatabaseType::Mysql => match operator {
            Operator::And | Operator::ImplicitAnd => {
                format!("{} {}", left.mysql_required(), right.mysql_required())
            }
            Operator::Or => {
                format!("{} {}", left.mysql_alternative(), right.mysql_alternative())
            }
            Operator::Not => {
                let right = match right.operator {
                    Some(_) => format!("-({})", right.text),
                    None => format!("-{}", right.text),
                };
                format!("{} {right}", left.mysql_required())
            }
        },
    };
    values.push(Expression {
        text,
        operator: Some(operator),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "sqlite")]
    #[test]
    fn long_queries_do_not_exhaust_the_fts5_parser_stack() {
        let db = Connection::open_sqlite_memory().unwrap();
        db.execute("CREATE VIRTUAL TABLE docs USING fts5(body)", ())
            .unwrap();
        db.execute("INSERT INTO docs VALUES ('Alice Smith')", ())
            .unwrap();
        for separator in [" ", " AND ", " OR ", " NOT Missing OR "] {
            let query = vec!["Alice"; 200].join(separator);
            let count = db
                .query_some::<i64>(
                    "SELECT COUNT(*) FROM docs WHERE docs MATCH ?",
                    db.preprocess_fts_query(&query),
                )
                .unwrap();
            assert_eq!(count, 1, "{separator}");
        }
    }

    #[test]
    fn sqlite_column_filters_are_preserved() {
        assert_eq!(preprocess_fts_query("email:carol"), "email:carol*");
    }

    #[test]
    fn translates_prefixes_and_operators() {
        for (input, sqlite, mysql) in [
            ("Alice", "Alice*", "Alice*"),
            ("Al*", "Al*", "Al*"),
            ("AND*", "\"AND\"*", "AND*"),
            ("Alice Smith", "Alice* AND Smith*", "+Alice* +Smith*"),
            ("Alice AND Smith", "Alice* AND Smith*", "+Alice* +Smith*"),
            ("Alice OR Bob", "Alice* OR Bob*", "Alice* Bob*"),
            ("Alice NOT Smith", "Alice* NOT Smith*", "+Alice* -Smith*"),
            (
                "Alice OR Bob AND Smith",
                "Alice* OR Bob* AND Smith*",
                "Alice* (+Bob* +Smith*)",
            ),
            (
                "Alice NOT Bob Smith",
                "Alice* NOT (Bob* AND Smith*)",
                "+Alice* -(+Bob* +Smith*)",
            ),
            (
                "Alice AND Bob NOT Smith",
                "Alice* AND Bob* NOT Smith*",
                "+Alice* +Bob* -Smith*",
            ),
            (
                "Alice NOT Bob NOT Smith",
                "Alice* NOT Bob* NOT Smith*",
                "+Alice* -Bob* -Smith*",
            ),
            (
                "AND Alice AND OR Bob NOT",
                "Alice* AND Bob*",
                "+Alice* +Bob*",
            ),
            ("(Alice) \"Smith\"", "Alice* AND Smith*", "+Alice* +Smith*"),
            ("Alice-Smith", "Alice* AND Smith*", "+Alice* +Smith*"),
            ("OR NOT () * + - @", "\"\"", ""),
            ("", "\"\"", ""),
        ] {
            assert_eq!(
                compile(input, DatabaseType::Sqlite, false),
                sqlite,
                "{input}"
            );
            assert_eq!(compile(input, DatabaseType::Mysql, false), mysql, "{input}");
        }
    }
}
