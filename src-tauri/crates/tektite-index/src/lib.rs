//! `tektite-index` — SQLite-backed note index.
//!
//! Manages schema creation, file ingest, link/tag/heading/alias queries,
//! link resolution, and rename planning over a vault's SQLite index.
//!
//! Internal modules:
//! - [`schema`]  — DDL and schema versioning
//! - [`ingest`]  — `upsert()` and `remove_file()`
//! - [`query`]   — read-only queries (files, links, tags, headings, aliases)
//! - [`resolve`] — link resolution with proximity tiebreaking
//! - [`rename`]  — rename planning and application

mod ingest;
mod link_health;
mod query;
mod rename;
mod resolve;
mod schema;

pub use link_health::{
    UnresolvedReport, UnresolvedSourceRef, UnresolvedTargetKind, UnresolvedTargetRow,
};
pub use query::{BacklinkRow, FtsRow, FuzzyFileRow, HeadingSearchRow};
pub use rename::{rewrite_content, RenameEdit, RenamePlan};
pub use resolve::LinkResolution;

use rusqlite::Connection;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("UUID parse error: {0}")]
    Uuid(#[from] uuid::Error),
}

// ---------------------------------------------------------------------------
// Core type aliases
// ---------------------------------------------------------------------------

/// Opaque stable note identifier (UUID v4 as a string).
pub type NoteId = String;

// ---------------------------------------------------------------------------
// Index struct
// ---------------------------------------------------------------------------

/// The live SQLite index for a single vault.
///
/// Constructed via [`Index::open`] for an on-disk database or
/// [`Index::open_in_memory`] for tests.
pub struct Index {
    conn: Connection,
    /// When `true`, proximity tiebreaking is used during link resolution.
    /// v0.1 keeps this off by default so ambiguous links surface intentionally.
    pub proximity_enabled: bool,
}

impl Index {
    /// Opens (or creates) an on-disk index at the given path.
    ///
    /// On schema version mismatch the database is deleted and recreated.
    pub fn open(path: &std::path::Path) -> Result<Self, IndexError> {
        let conn = Connection::open(path)?;
        let mut index = Self {
            conn,
            proximity_enabled: false,
        };
        // Foreign-key enforcement must be enabled per-connection in SQLite.
        index.conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        schema::ensure_schema(&mut index.conn)?;
        Ok(index)
    }

    /// Opens an in-memory index — intended for unit and integration tests.
    pub fn open_in_memory() -> Result<Self, IndexError> {
        let conn = Connection::open_in_memory()?;
        let mut index = Self {
            conn,
            proximity_enabled: false,
        };
        // Foreign-key enforcement must be enabled per-connection in SQLite.
        index.conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        schema::ensure_schema(&mut index.conn)?;
        Ok(index)
    }
}
