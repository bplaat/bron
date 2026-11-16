# Bassie SQL crate

A simple and minimal Rust SQLite and MySQL library with an ergonomic API.

## Features

- `sqlite` (default): SQLite backend using the system library
- `sqlite-bundled`: compile the bundled SQLite source instead
- `sqlite-fts5`: enable full-text search in bundled SQLite; system SQLite must already provide FTS5
- `mysql`: MySQL backend, including Unix socket transports on Unix
- `mysql-tls`: verified TLS for MySQL over TCP
- `mysql-native-password`: legacy `mysql_native_password` authentication
- `derive` (default): `FromRow` and `FromValue` derive macros
- `chrono`, `uuid`: value conversions for these crates

At least one backend must be enabled.

## Connection pool

Connections use a thread-safe, lazily grown pool sized for `small-http`'s worker pool. Pass
`PoolOptions` to set a custom limit, or `PoolOptions::single_connection()` for serialized
applications. A transaction holds one connection for its whole closure.

## SQLite example

An example that inserts and reads rows to and from structs:

```rs
use bsql::{Connection, FromRow};

#[derive(FromRow)]
struct NewPerson {
    name: String,
    age: i64,
}

#[derive(Debug, FromRow)]
struct Person {
    id: i64,
    name: String,
    age: i64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Connect and create table
    let db = Connection::open_sqlite_memory()?;
    db.execute(
        "CREATE TABLE IF NOT EXISTS persons (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            age INTEGER NOT NULL
        ) STRICT",
        (),
    )?;

    // Insert rows
    let persons = [
        NewPerson {
            name: "Alice".to_string(),
            age: 30,
        },
        NewPerson {
            name: "Bob".to_string(),
            age: 40,
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

    // Group related writes atomically
    db.transaction(|transaction| -> Result<(), bsql::StatementError> {
        transaction.execute(
            "UPDATE persons SET age = age + 1 WHERE name = ?",
            "Alice".to_string(),
        )?;
        transaction.execute(
            "UPDATE persons SET age = age + 1 WHERE name = ?",
            "Bob".to_string(),
        )?;
        Ok(())
    })
    .expect("Can't update persons");

    // Read rows back
    let persons = db.query::<Person>(format!("SELECT {} FROM persons", Person::columns()), ())?;
    for person in persons {
        let person = person?;
        println!("{person:?}"); // -> Person { id: 1, name: "Alice", age: 31 }
    }
    Ok(())
}
```

See the [examples](examples/) for many more examples.

File-backed SQLite reserves one connection for writes and transactions; the others serve reads.
Call `enable_wal_logging` to enable WAL. In-memory SQLite uses a single connection, because each
`:memory:` connection is a separate database.

## MySQL example

The MySQL backend implements the MySQL classic protocol directly and does not use another
database client crate:

```rs
use bsql::{Connection, MysqlTransport, PoolOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = Connection::open_mysql(
        MysqlTransport::tcp("localhost", 3306, true),
        "app",
        "secret",
        Some("app"),
        PoolOptions::default(),
    )?;
    let names = database
        .query::<String>("SELECT name FROM persons WHERE age >= ?", 18_i64)?
        .collect::<Result<Vec<_>, _>>()?;
    println!("{names:?}");
    Ok(())
}
```

Use `MysqlTransport::unix("/tmp/mysql.sock")` for a local socket. All MySQL pool connections
handle reads and writes.

Supported authentication: empty passwords, `caching_sha2_password`, `auth_socket`/`unix_socket`
and, with `mysql-native-password`, `mysql_native_password`. Full `caching_sha2_password`
authentication over TCP requires TLS; RSA password exchange is not implemented.

## Design goals

- Connect to SQLite or MySQL through one API
- Implement the MySQL protocol without depending on a MySQL client crate
- Bind and read portable `Value` types through server-side prepared statements
- Have `FromRow` and `FromValue` derive macros for typed application models
- Work well and efficient with popular crates like `uuid` and `chrono`
- Have helpful error messages on query errors

## License

Copyright © 2024-2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
