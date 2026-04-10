//! `tektite-search` — Search ranking and fuzzy matching over indexed vault data.
//!
//! Provides full-text search via FTS5, fuzzy file-name matching, and heading
//! search. This crate is a thin query layer over the SQLite index managed by
//! `tektite-index`.
//!
//! The full implementation lands in Phase 8. This module defines the public
//! result types so downstream crates can depend on them.

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use tektite_index::NoteId;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("Index error: {0}")]
    Index(#[from] tektite_index::IndexError),
}

// ---------------------------------------------------------------------------
// Public result types
// ---------------------------------------------------------------------------

/// A single full-text search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The note's stable ID.
    pub id: NoteId,
    /// Vault-relative path.
    pub path: String,
    /// Note title (from frontmatter or first heading).
    pub title: String,
    /// Contextual snippet with match highlights.
    pub snippet: String,
    /// BM25 relevance rank (lower = more relevant in SQLite FTS5 convention).
    pub rank: f64,
}

/// A single fuzzy file-name match result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzyFileResult {
    pub id: NoteId,
    pub path: String,
    pub name: String,
    pub score: f64,
}

/// A heading search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadingResult {
    pub file_id: NoteId,
    pub file_path: String,
    pub level: u8,
    pub text: String,
}
