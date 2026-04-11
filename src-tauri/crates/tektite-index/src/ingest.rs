//! File ingest: upsert and removal.
//!
//! `upsert()` is the primary write path. It accepts a file path, its mtime,
//! and the fully parsed [`tektite_parser::ParsedNote`], and atomically
//! replaces all indexed data for that file. After committing, it populates
//! `resolved_target_id` for all outgoing links and re-resolves any previously
//! unresolved links that may now match this file.

use rusqlite::params;
use uuid::Uuid;

use tektite_parser::ParsedNote;

use crate::{rename::stem_from_path, Index, IndexError, NoteId};

impl Index {
    /// Insert or replace all indexed data for a file.
    ///
    /// If the file's path already exists in the index its `id` is preserved;
    /// otherwise a new UUID v4 is minted.
    ///
    /// After the transaction commits, `resolved_target_id` is populated for
    /// all links from this file, and any previously unresolved links from
    /// other files that match this file's stem are re-resolved.
    pub fn upsert(
        &mut self,
        path: &str,
        mtime_secs: i64,
        note: &ParsedNote,
    ) -> Result<NoteId, IndexError> {
        let tx = self.conn.transaction()?;

        // Resolve or mint the file ID.
        let id: NoteId = {
            let existing: Option<String> = tx
                .query_row(
                    "SELECT id FROM files WHERE path = ?1",
                    params![path],
                    |row| row.get(0),
                )
                .ok();
            existing.unwrap_or_else(|| Uuid::new_v4().to_string())
        };

        // Upsert the file record.
        tx.execute(
            "INSERT INTO files (id, path, mtime_secs)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(path) DO UPDATE SET mtime_secs = excluded.mtime_secs",
            params![id, path, mtime_secs],
        )?;

        // Replace child records (delete explicitly for clarity).
        tx.execute("DELETE FROM aliases   WHERE file_id = ?1", params![id])?;
        tx.execute("DELETE FROM headings  WHERE file_id = ?1", params![id])?;
        tx.execute("DELETE FROM links     WHERE source_id = ?1", params![id])?;
        tx.execute("DELETE FROM tags      WHERE file_id = ?1", params![id])?;
        tx.execute("DELETE FROM tasks     WHERE file_id = ?1", params![id])?;
        tx.execute("DELETE FROM frontmatter WHERE file_id = ?1", params![id])?;

        // Insert aliases from frontmatter.
        if let Some(aliases_val) = note.frontmatter.get("aliases") {
            if let Some(seq) = aliases_val.as_sequence() {
                for item in seq {
                    if let Some(alias) = item.as_str() {
                        tx.execute(
                            "INSERT INTO aliases (file_id, alias) VALUES (?1, ?2)",
                            params![id, alias],
                        )?;
                    }
                }
            }
        }

        // Insert headings.
        for heading in &note.headings {
            tx.execute(
                "INSERT INTO headings (file_id, level, text) VALUES (?1, ?2, ?3)",
                params![id, heading.level, heading.text],
            )?;
        }

        // Insert links (resolved_target_id is populated after commit).
        for link in &note.links {
            tx.execute(
                "INSERT INTO links (source_id, target, fragment, alias)
                 VALUES (?1, ?2, ?3, ?4)",
                params![id, link.target, link.fragment, link.alias],
            )?;
        }

        // Insert tags.
        for tag in &note.tags {
            tx.execute(
                "INSERT INTO tags (file_id, name) VALUES (?1, ?2)",
                params![id, tag],
            )?;
        }

        // Insert tasks.
        for task in &note.tasks {
            tx.execute(
                "INSERT INTO tasks (file_id, text, done) VALUES (?1, ?2, ?3)",
                params![id, task.text, task.done as i64],
            )?;
        }

        // Insert frontmatter JSON blob.
        let fm_json = serde_json::to_string(&note.frontmatter).unwrap_or_else(|_| "{}".to_string());
        tx.execute(
            "INSERT OR REPLACE INTO frontmatter (file_id, data) VALUES (?1, ?2)",
            params![id, fm_json],
        )?;

        // Update FTS index.
        let title = note
            .frontmatter
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        tx.execute("DELETE FROM fts WHERE path = ?1", params![path])?;
        tx.execute(
            "INSERT INTO fts (path, title, body) VALUES (?1, ?2, ?3)",
            params![path, title, note.body],
        )?;

        tx.commit()?;

        // Post-commit: populate resolved_target_id for links FROM this file.
        self.resolve_outgoing_links(&id)?;

        // Post-commit: re-resolve links in OTHER files that target this file's
        // stem — they may have been previously unresolved or ambiguous.
        let stem = stem_from_path(path);
        self.re_resolve_links_matching_stem(stem)?;

        Ok(id)
    }

    /// Remove a file and all its child records from the index.
    ///
    /// After deletion, re-resolves links targeting the removed file's stem —
    /// a previously ambiguous link may now resolve to a surviving file, and
    /// previously resolved links (now SET NULL by the FK) need re-evaluation.
    pub fn remove_file(&mut self, path: &str) -> Result<(), IndexError> {
        // Capture stem before deletion so we can re-resolve after.
        let stem = stem_from_path(path).to_string();

        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        tx.execute("DELETE FROM fts   WHERE path = ?1", params![path])?;
        tx.commit()?;

        // Re-resolve links that targeted this file's stem.
        self.re_resolve_links_matching_stem(&stem)?;

        Ok(())
    }
}
