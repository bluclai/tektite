//! Schema creation and version-stepped migrations.
//!
//! A tiny migration runner applies ordered `(version, sql)` pairs on top of
//! whatever version is stored in the `meta` table. If any migration fails —
//! syntax error, corrupt DB, anything — the runner falls back to a nuclear
//! rebuild: drop every table and recreate the schema from scratch.

use crate::IndexError;
use rusqlite::Connection;

/// Latest schema version. Bump whenever a new migration is added.
const SCHEMA_VERSION: i64 = 2;

/// Ordered migrations applied above the current stored version.
///
/// Each entry is `(target_version, sql)`. `sql` may contain multiple
/// statements — it is run via `execute_batch`.
fn migrations() -> &'static [(i64, &'static str)] {
    &[
        // v1 — initial schema. Applied to fresh databases via the nuclear
        // rebuild path below; kept here so the migration list is self-contained
        // and new features can build on top of it.
        (1, V1_SCHEMA_SQL),
        // v2 — semantic chunks table for tektite-embed.
        (2, V2_CHUNKS_SQL),
    ]
}

/// Ensures the schema is up to date.
///
/// - Fresh DB → applies every migration in order.
/// - Existing DB at version `v < SCHEMA_VERSION` → applies migrations above `v`.
/// - Existing DB at `v == SCHEMA_VERSION` → no-op.
/// - Existing DB at `v > SCHEMA_VERSION` or on any migration failure → nuclear
///   rebuild (drop every table, apply all migrations from scratch).
pub(crate) fn ensure_schema(conn: &mut Connection) -> Result<(), IndexError> {
    // Meta table is the one piece of state the migration runner depends on,
    // so create it unconditionally before reading the stored version.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;

    let stored_version: i64 = read_version(conn).unwrap_or(0);

    if stored_version == SCHEMA_VERSION {
        return Ok(());
    }

    if stored_version > SCHEMA_VERSION {
        // Downgrade from a newer version we don't understand — safest path is
        // a clean rebuild.
        return nuclear_rebuild(conn);
    }

    match apply_migrations_from(conn, stored_version) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("schema migration failed: {e}; falling back to nuclear rebuild");
            nuclear_rebuild(conn)
        }
    }
}

fn apply_migrations_from(conn: &mut Connection, current: i64) -> Result<(), IndexError> {
    for (target, sql) in migrations() {
        if *target <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
            [target.to_string()],
        )?;
        tx.commit()?;
    }
    Ok(())
}

fn nuclear_rebuild(conn: &mut Connection) -> Result<(), IndexError> {
    drop_all_tables(conn)?;
    // Re-create the meta table (drop_all_tables clears it) and apply every
    // migration from zero.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );",
    )?;
    apply_migrations_from(conn, 0)
}

fn read_version(conn: &Connection) -> Option<i64> {
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version'",
        [],
        |row| row.get(0),
    )
    .ok()
}

fn drop_all_tables(conn: &mut Connection) -> Result<(), IndexError> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS chunks;
         DROP TABLE IF EXISTS fts;
         DROP TABLE IF EXISTS tasks;
         DROP TABLE IF EXISTS tags;
         DROP TABLE IF EXISTS links;
         DROP TABLE IF EXISTS headings;
         DROP TABLE IF EXISTS aliases;
         DROP TABLE IF EXISTS frontmatter;
         DROP TABLE IF EXISTS files;
         DROP TABLE IF EXISTS meta;",
    )?;
    Ok(())
}

const V1_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS files (
    id         TEXT PRIMARY KEY,
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
    data    TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
    path, title, body,
    tokenize='porter unicode61'
);
";

/// v1 → v2 — tektite-embed's `chunks` table.
///
/// The `embedding` column stores raw little-endian f32 bytes (256 dims = 1024
/// bytes for Matryoshka-truncated nomic-embed-text-v1.5). The schema is
/// additive: existing FTS / links / tags data is untouched.
const V2_CHUNKS_SQL: &str = "
CREATE TABLE IF NOT EXISTS chunks (
    id            TEXT PRIMARY KEY,
    file_id       TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    chunk_index   INTEGER NOT NULL,
    heading_path  TEXT,
    content       TEXT NOT NULL,
    content_hash  TEXT NOT NULL,
    token_count   INTEGER NOT NULL,
    embedding     BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_id);
CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(content_hash);
";
