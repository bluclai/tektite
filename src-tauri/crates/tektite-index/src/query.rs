//! Read-only index queries.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{Index, IndexError, NoteId};

// ---------------------------------------------------------------------------
// Public query result types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: NoteId,
    pub path: String,
    pub mtime_secs: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkRecord {
    pub id: i64,
    pub source_id: NoteId,
    pub target: String,
    pub fragment: Option<String>,
    pub alias: Option<String>,
    pub resolved_target_id: Option<NoteId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadingRecord {
    pub id: i64,
    pub file_id: NoteId,
    pub level: u8,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRecord {
    pub id: i64,
    pub file_id: NoteId,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub id: i64,
    pub file_id: NoteId,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtsRow {
    pub path: String,
    pub title: String,
    pub snippet: String,
    pub rank: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FuzzyFileRow {
    pub path: String,
    pub name: String,
    pub score: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadingSearchRow {
    pub file_id: NoteId,
    pub file_path: String,
    pub level: u8,
    pub text: String,
}

// ---------------------------------------------------------------------------
// Query implementations
// ---------------------------------------------------------------------------

impl Index {
    /// Returns all files in the index.
    pub fn all_files(&self) -> Result<Vec<FileRecord>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, path, mtime_secs FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime_secs: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns the stored mtime for a path, or `None` if not indexed.
    pub fn get_mtime(&self, path: &str) -> Result<Option<i64>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT mtime_secs FROM files WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .ok())
    }

    /// Returns the `NoteId` for a path, or `None` if not indexed.
    pub fn id_for_path(&self, path: &str) -> Result<Option<NoteId>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT id FROM files WHERE path = ?1",
                params![path],
                |row| row.get(0),
            )
            .ok())
    }

    /// Returns the vault-relative path for a note ID, or `None` if not indexed.
    pub fn path_for_id(&self, note_id: &str) -> Result<Option<String>, IndexError> {
        Ok(self
            .conn
            .query_row(
                "SELECT path FROM files WHERE id = ?1",
                params![note_id],
                |row| row.get(0),
            )
            .ok())
    }

    /// Returns all headings for a file.
    pub fn get_headings(&self, file_id: &str) -> Result<Vec<HeadingRecord>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_id, level, text FROM headings WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok(HeadingRecord {
                id: row.get(0)?,
                file_id: row.get(1)?,
                level: row.get(2)?,
                text: row.get(3)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns all outgoing links from a file.
    pub fn get_links(&self, source_id: &str) -> Result<Vec<LinkRecord>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, target, fragment, alias, resolved_target_id
             FROM links WHERE source_id = ?1",
        )?;
        let rows = stmt.query_map(params![source_id], |row| {
            Ok(LinkRecord {
                id: row.get(0)?,
                source_id: row.get(1)?,
                target: row.get(2)?,
                fragment: row.get(3)?,
                alias: row.get(4)?,
                resolved_target_id: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns all incoming links (backlinks) pointing to a file.
    pub fn get_backlinks(&self, target_id: &str) -> Result<Vec<LinkRecord>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_id, target, fragment, alias, resolved_target_id
             FROM links WHERE resolved_target_id = ?1",
        )?;
        let rows = stmt.query_map(params![target_id], |row| {
            Ok(LinkRecord {
                id: row.get(0)?,
                source_id: row.get(1)?,
                target: row.get(2)?,
                fragment: row.get(3)?,
                alias: row.get(4)?,
                resolved_target_id: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns all tags for a file.
    pub fn get_tags(&self, file_id: &str) -> Result<Vec<TagRecord>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_id, name FROM tags WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok(TagRecord {
                id: row.get(0)?,
                file_id: row.get(1)?,
                name: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns all tasks for a file.
    pub fn get_tasks(&self, file_id: &str) -> Result<Vec<TaskRecord>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_id, text, done FROM tasks WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| {
            let done_int: i64 = row.get(3)?;
            Ok(TaskRecord {
                id: row.get(0)?,
                file_id: row.get(1)?,
                text: row.get(2)?,
                done: done_int != 0,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns all aliases for a file.
    pub fn get_aliases(&self, file_id: &str) -> Result<Vec<String>, IndexError> {
        let mut stmt = self
            .conn
            .prepare("SELECT alias FROM aliases WHERE file_id = ?1")?;
        let rows = stmt.query_map(params![file_id], |row| row.get(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns files whose filename stem matches `stem` (case-insensitive).
    ///
    /// Matches paths whose filename component is exactly `<stem>.md`
    /// (case-insensitive). The SQL LIKE pre-filter narrows candidates, then
    /// a Rust post-filter verifies the exact stem match to avoid false
    /// positives (e.g. `%/note.md` must not match `keynote.md`).
    pub fn files_by_stem(&self, stem: &str) -> Result<Vec<FileRecord>, IndexError> {
        let sub_pattern = format!("%/{}.md", stem.to_lowercase());
        let root_pattern = format!("{}.md", stem.to_lowercase());
        let mut stmt = self.conn.prepare(
            "SELECT id, path, mtime_secs FROM files
             WHERE LOWER(path) LIKE ?1
                OR LOWER(path) = ?2",
        )?;
        let rows = stmt.query_map(params![sub_pattern, root_pattern], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime_secs: row.get(2)?,
            })
        })?;
        let stem_lower = stem.to_lowercase();
        let candidates: Vec<FileRecord> = rows.collect::<Result<Vec<_>, _>>()?;
        Ok(candidates
            .into_iter()
            .filter(|f| {
                // Extract the actual filename stem from the path and verify
                // it matches exactly (not just as a suffix).
                let filename = f.path.rsplit('/').next().unwrap_or(&f.path);
                let actual_stem = filename.strip_suffix(".md").unwrap_or(filename);
                actual_stem.to_lowercase() == stem_lower
            })
            .collect())
    }

    /// Returns files whose alias matches `alias` (case-insensitive).
    pub fn files_by_alias(&self, alias: &str) -> Result<Vec<FileRecord>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT f.id, f.path, f.mtime_secs
             FROM files f
             JOIN aliases a ON a.file_id = f.id
             WHERE a.alias = ?1 COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![alias], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime_secs: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns files whose path starts with `prefix` (case-insensitive).
    pub fn files_by_path_prefix(&self, prefix: &str) -> Result<Vec<FileRecord>, IndexError> {
        let pattern = format!("{}%", prefix);
        let mut stmt = self.conn.prepare(
            "SELECT id, path, mtime_secs FROM files
             WHERE LOWER(path) LIKE LOWER(?1)",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime_secs: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns files whose vault-relative markdown path exactly matches a
    /// path-qualified wiki-link target (case-insensitive).
    ///
    /// `target` is the wiki-link target text without the `.md` extension,
    /// e.g. `folder/note`.
    pub fn files_by_link_target_path(&self, target: &str) -> Result<Vec<FileRecord>, IndexError> {
        let path = format!("{}.md", target.trim_matches('/'));
        let mut stmt = self.conn.prepare(
            "SELECT id, path, mtime_secs FROM files
             WHERE LOWER(path) = LOWER(?1)",
        )?;
        let rows = stmt.query_map(params![path], |row| {
            Ok(FileRecord {
                id: row.get(0)?,
                path: row.get(1)?,
                mtime_secs: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Full-text search using FTS5 MATCH with BM25 ranking and snippets.
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<FtsRow>, IndexError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(vec![]);
        }

        // Build FTS5 query: split into words, wrap each in quotes for phrase search,
        // add * for prefix matching. Join with space (AND by default in FTS5).
        let fts_query = trimmed
            .split_whitespace()
            .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" ");

        let mut stmt = self.conn.prepare(
            "SELECT path, title, snippet(fts, 2, '__MATCH__', '__ENDMATCH__', '...', 10), rank
             FROM fts
             WHERE fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![&fts_query, limit as i64], |row| {
            Ok(FtsRow {
                path: row.get(0)?,
                title: row.get(1)?,
                snippet: row.get(2)?,
                rank: row.get(3)?,
            })
        });

        match rows {
            Ok(mapped) => mapped.collect::<Result<Vec<_>, _>>().map_err(Into::into),
            // FTS MATCH syntax error → return empty results instead of propagating error
            Err(rusqlite::Error::SqliteFailure(_, _)) => Ok(vec![]),
            Err(e) => Err(IndexError::Sqlite(e)),
        }
    }

    /// Returns all headings matching a search term (case-insensitive LIKE).
    pub fn search_headings(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<HeadingSearchRow>, IndexError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(vec![]);
        }

        let pattern = format!("%{}%", trimmed);
        let mut stmt = self.conn.prepare(
            "SELECT h.file_id, f.path, h.level, h.text
             FROM headings h
             JOIN files f ON f.id = h.file_id
             WHERE LOWER(h.text) LIKE LOWER(?1)
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![pattern, limit as i64], |row| {
            Ok(HeadingSearchRow {
                file_id: row.get(0)?,
                file_path: row.get(1)?,
                level: row.get(2)?,
                text: row.get(3)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Fuzzy-matches files by name using a simple subsequence scorer.
    /// Returns files where all query characters appear in order, sorted by score.
    pub fn search_fuzzy_files(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<FuzzyFileRow>, IndexError> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            let all = self.all_files()?;
            return Ok(all
                .into_iter()
                .take(limit)
                .map(|f| {
                    let name = f
                        .path
                        .rsplit('/')
                        .next()
                        .unwrap_or(&f.path)
                        .strip_suffix(".md")
                        .unwrap_or(&f.path)
                        .to_string();
                    FuzzyFileRow {
                        path: f.path,
                        name,
                        score: 0,
                    }
                })
                .collect());
        }

        let all = self.all_files()?;
        let mut scored: Vec<_> = all
            .into_iter()
            .filter_map(|f| {
                // Extract the file name stem (filename without .md)
                let filename = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                let name_stem = filename
                    .strip_suffix(".md")
                    .unwrap_or(&filename)
                    .to_string();
                let score = fuzzy_score(trimmed, &name_stem)?;
                Some(FuzzyFileRow {
                    path: f.path,
                    name: name_stem,
                    score,
                })
            })
            .collect();

        scored.sort_by_key(|r| std::cmp::Reverse(r.score));
        scored.truncate(limit);
        Ok(scored)
    }
}

/// Scores how well `query` matches `target`.
/// Returns `None` if not all query characters are present in order.
/// Higher scores are better matches.
fn fuzzy_score(query: &str, target: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }

    let q = query.to_lowercase();
    let t = target.to_lowercase();
    let q_chars: Vec<char> = q.chars().collect();
    let t_chars: Vec<char> = t.chars().collect();

    // Verify all query chars appear in target in order
    let mut matches = Vec::new();
    let mut qi = 0;
    let mut ti = 0;

    while qi < q_chars.len() && ti < t_chars.len() {
        if q_chars[qi] == t_chars[ti] {
            matches.push(ti);
            qi += 1;
        }
        ti += 1;
    }

    if qi < q_chars.len() {
        return None; // not all chars found
    }

    // Score = base + bonuses - penalties
    let mut score = 100i64;

    // Substring match bonus
    if t.contains(&q) {
        score += 500;
        // Prefix match bonus
        if t.starts_with(&q) {
            score += 1000;
        }
    }

    // Consecutive match bonus: +50 per consecutive pair
    let consecutive = matches.windows(2).filter(|w| w[1] == w[0] + 1).count();
    score += consecutive as i64 * 50;

    // Spread penalty: if matches are far apart, reduce score
    if let (Some(&first), Some(&last)) = (matches.first(), matches.last()) {
        let spread = (last - first) as i64;
        score -= spread;
    }

    Some(score)
}
