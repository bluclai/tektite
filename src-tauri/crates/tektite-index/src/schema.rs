//! Schema creation and version management.
//!
//! On schema version mismatch the caller is expected to drop and recreate
//! the database. No partial migrations are performed for v1.

use crate::IndexError;
use rusqlite::Connection;

/// Current schema version. Bump whenever DDL changes.
const SCHEMA_VERSION: i64 = 1;

/// Ensures the schema is up to date, creating it if the database is new.
///
/// If the stored version doesn't match [`SCHEMA_VERSION`], returns an error
/// so the caller can delete and recreate the database.
pub(crate) fn ensure_schema(conn: &mut Connection) -> Result<(), IndexError> {
    // Create the meta table first so we can always read/write the version.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    let stored_version: Option<i64> = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();

    match stored_version {
        Some(v) if v == SCHEMA_VERSION => {
            // Schema is current — nothing to do.
        }
        Some(_) => {
            // Version mismatch: drop all tables and recreate.
            drop_all_tables(conn)?;
            create_tables(conn)?;
            set_version(conn)?;
        }
        None => {
            // Fresh database: create schema.
            create_tables(conn)?;
            set_version(conn)?;
        }
    }

    Ok(())
}

fn set_version(conn: &Connection) -> Result<(), IndexError> {
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
        [SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

fn drop_all_tables(conn: &mut Connection) -> Result<(), IndexError> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS fts;
         DROP TABLE IF EXISTS tasks;
         DROP TABLE IF EXISTS tags;
         DROP TABLE IF EXISTS links;
         DROP TABLE IF EXISTS headings;
         DROP TABLE IF EXISTS aliases;
         DROP TABLE IF EXISTS frontmatter;
         DROP TABLE IF EXISTS files;",
    )?;
    Ok(())
}

fn create_tables(conn: &mut Connection) -> Result<(), IndexError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS files (
            id         TEXT PRIMARY KEY,   -- UUID v4
            path       TEXT UNIQUE NOT NULL,
            mtime_secs INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS aliases (
            id      INTEGER PRIMARY KEY,
            file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            alias   TEXT NOT NULL COLLATE NOCASE
        );
        CREATE INDEX IF NOT EXISTS idx_aliases_alias ON aliases(alias COLLATE NOCASE);
        CREATE INDEX IF NOT EXISTS idx_aliases_file  ON aliases(file_id);

        CREATE TABLE IF NOT EXISTS headings (
            id      INTEGER PRIMARY KEY,
            file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            level   INTEGER NOT NULL,
            text    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_headings_file ON headings(file_id);

        CREATE TABLE IF NOT EXISTS links (
            id                 INTEGER PRIMARY KEY,
            source_id          TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            target             TEXT NOT NULL,
            fragment           TEXT,
            alias              TEXT,
            resolved_target_id TEXT REFERENCES files(id) ON DELETE SET NULL
        );
        CREATE INDEX IF NOT EXISTS idx_links_source   ON links(source_id);
        CREATE INDEX IF NOT EXISTS idx_links_target   ON links(target);
        CREATE INDEX IF NOT EXISTS idx_links_resolved ON links(resolved_target_id);

        CREATE TABLE IF NOT EXISTS tags (
            id      INTEGER PRIMARY KEY,
            file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            name    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_tags_name  ON tags(name);
        CREATE INDEX IF NOT EXISTS idx_tags_file  ON tags(file_id);

        CREATE TABLE IF NOT EXISTS tasks (
            id      INTEGER PRIMARY KEY,
            file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            text    TEXT NOT NULL,
            done    INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_tasks_file ON tasks(file_id);

        CREATE TABLE IF NOT EXISTS frontmatter (
            file_id TEXT PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
            data    TEXT NOT NULL   -- JSON blob for non-normalized fields
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
            path, title, body,
            tokenize='porter unicode61'
        );",
    )?;
    Ok(())
}
