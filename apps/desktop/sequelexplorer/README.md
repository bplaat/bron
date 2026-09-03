# Sequel Explorer

A lightweight SQLite and MySQL database explorer and editor built with
[bwebview](../../../lib/ui/bwebview) and [bsql](../../../lib/data/bsql). It combines fast table
browsing, direct row editing, a full read/write SQL workspace, and MySQL user management in a
compact native desktop application.

## Features

### Connections

- Open SQLite database files
- Connect to MySQL over TCP or a local Unix socket
- Save MySQL passwords in the operating system credential store and reconnect automatically
- Browse the MySQL databases available to the connected account

### Data browsing and editing

- Navigate tables from the keyboard-friendly sidebar
- Inspect rows, declared column types, primary keys, foreign keys, and CREATE TABLE statements
- Rename tables and edit columns, primary keys, and standard or unique indexes from the Schema tab
- Load large tables incrementally
- Add rows with an explicit value, NULL, or column default for every field
- Edit cells inline by double-clicking and save changes on blur
- Delete rows from a primary-key-safe right-click menu with confirmation
- Follow foreign-key references directly to their related rows
- Run convenient SELECT queries from the Data tab

### SQL and administration

- Execute arbitrary read and write statements in the split Query workspace
- View returned rows, affected-row counts, execution time, and database errors
- Import SQL scripts and export table schemas and rows, including SQLite indexes, triggers, and views
- Create, edit, and delete MySQL users and assign full direct database privileges

## Essential UX still missing

The most important remaining improvements are:

1. Row selection plus Copy value and Copy row actions in the right-click menu.
2. Explicit refresh controls and clear stale-data feedback after writes or external database changes.
3. Header-based sorting and structured per-column filters without writing SQL manually.
4. Column-aware editors for booleans, dates, JSON, enums, long text, and binary values.
5. Multi-row selection with bulk delete and bulk field updates.
6. Query history, saved queries, multiple query tabs, and optional multi-statement execution.
7. Transaction controls with review and confirmation for destructive statements.
8. Creating and deleting tables plus visual foreign-key editing.

## Screenshot

![Sequel Explorer Screenshot](docs/images/screenshot.png)

## macOS Entitlements

The `com.apple.security.app-sandbox` entitlement is not used because SQLite databases can require
access to accompanying `*-shm` and `*-wal` files next to the selected `.db` file.

For stable Keychain authorization across rebuilds, bundles use the first valid system code-signing
identity. Set `CODESIGN_IDENTITY` or `package.metadata.bundle.signing_identity` to select one
explicitly. Ad-hoc signing remains the fallback when no certificate is available.

## License

Copyright © 2026 [Bastiaan van der Plaat](https://github.com/bplaat)

Licensed under the [MIT](../../../LICENSE) license.
