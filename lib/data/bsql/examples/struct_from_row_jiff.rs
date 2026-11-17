/*
 * Copyright (c) 2025 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A example that inserts and reads rows with structs that derive [FromRow] and have [jiff::Timestamp] fields.

use std::fmt::Debug;

use bsql::{Connection, FromRow};
use jiff::Timestamp;

#[derive(FromRow)]
struct NewPerson {
    name: String,
    age: i64,
    created_at: Timestamp,
}

#[derive(Debug, FromRow)]
struct Person {
    id: i64,
    name: String,
    age: i64,
    created_at: Timestamp,
}

fn main() -> anyhow::Result<()> {
    // Connect and create table
    let db = Connection::open_sqlite_memory().expect("Can't open database");
    db.execute(
        "CREATE TABLE IF NOT EXISTS persons (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            age INTEGER NOT NULL,
            created_at INTEGER NOT NULL
        ) STRICT",
        (),
    )?;

    // Insert a rows
    let persons = [
        NewPerson {
            name: "Alice".to_string(),
            age: 30,
            created_at: Timestamp::now(),
        },
        NewPerson {
            name: "Bob".to_string(),
            age: 40,
            created_at: Timestamp::now(),
        },
    ];
    for person in persons {
        db.execute(
            format!(
                "INSERT INTO persons ({}) VALUES ({})",
                NewPerson::columns(),
                NewPerson::values()
            ),
            person,
        )?;
    }

    // Read rows back
    for person in db.query::<Person>(format!("SELECT {} FROM persons", Person::columns()), ())? {
        println!("{:?}", person?);
    }
    Ok(())
}
