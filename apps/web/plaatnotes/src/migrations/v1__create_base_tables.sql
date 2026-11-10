-- Datetime columns store UTC Unix milliseconds.
CREATE TABLE users(
    id BLOB PRIMARY KEY,
    first_name TEXT NOT NULL,
    last_name TEXT NOT NULL,
    email TEXT NOT NULL UNIQUE,
    password TEXT NOT NULL,
    theme INTEGER NOT NULL,
    language TEXT NOT NULL,
    role INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

CREATE TABLE sessions(
    id BLOB PRIMARY KEY,
    user_id BLOB NOT NULL,
    token TEXT NOT NULL,
    ip_address TEXT NOT NULL,
    ip_latitude REAL,
    ip_longitude REAL,
    ip_country TEXT,
    ip_city TEXT,
    client_name TEXT,
    client_version TEXT,
    client_os TEXT,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) STRICT;

CREATE TABLE notes(
    id BLOB PRIMARY KEY,
    user_id BLOB NOT NULL,
    title TEXT NULL,
    body TEXT NOT NULL,
    is_pinned INTEGER NOT NULL,
    is_archived INTEGER NOT NULL,
    is_trashed INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
) STRICT;
