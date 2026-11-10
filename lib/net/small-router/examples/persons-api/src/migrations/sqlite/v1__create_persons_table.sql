-- Datetime columns store UTC Unix milliseconds.
CREATE TABLE persons(
    id BLOB PRIMARY KEY,
    name TEXT NOT NULL,
    age INTEGER NOT NULL,
    relation INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
) STRICT;

-- bsql:create_fts5_table persons, name
