//! Link resolution with proximity-based tiebreaking.
//!
//! Resolution order:
//! 1. Filename stem (case-insensitive)
//! 2. Frontmatter alias (case-insensitive, via `aliases` table)
//! 3. Path-qualified target (case-insensitive)
//!
//! At each tier, exactly 1 match resolves. 0 or 2+ falls through to the next
//! tier. After all tiers fail the link is `Unresolved`.
//!
//! Proximity tiebreaking (on by default): when multiple candidates exist at
//! the same tier, the candidate with the shortest path relative to the
//! source file wins. Disable via [`Index::proximity_enabled`].

use rusqlite::params;

use crate::{Index, IndexError, NoteId};

// ---------------------------------------------------------------------------
// Public resolution result
// ---------------------------------------------------------------------------

/// The result of resolving a wiki-link target string.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkResolution {
    /// Exactly one candidate found.
    Resolved(NoteId),
    /// No matching file found.
    Unresolved,
    /// Multiple candidates remain after all tiebreaking.
    Ambiguous(Vec<NoteId>),
}

// ---------------------------------------------------------------------------
// Resolution implementation
// ---------------------------------------------------------------------------

impl Index {
    /// Resolve a wiki-link `target` string to a [`LinkResolution`].
    ///
    /// `source_path` is the vault-relative path of the file containing the
    /// link. It is used for proximity tiebreaking and may be `None` (in which
    /// case proximity has no effect).
    pub fn resolve_link(
        &self,
        target: &str,
        source_path: Option<&str>,
    ) -> Result<LinkResolution, IndexError> {
        // Strip path qualifier to get the stem for tier-1 and tier-2 checks.
        let stem = stem_from_target(target);

        // Tier 1: filename stem.
        let stem_matches = self.files_by_stem(stem)?;
        if let Some(res) = self.resolve_candidates(stem_matches, source_path) {
            return Ok(res);
        }

        // Tier 2: frontmatter alias.
        let alias_matches = self.files_by_alias(stem)?;
        if let Some(res) = self.resolve_candidates(alias_matches, source_path) {
            return Ok(res);
        }

        // Tier 3: path-qualified (case-insensitive).
        // Only attempt this if the target looks path-qualified (contains '/').
        if target.contains('/') {
            let path_matches = self.files_by_path_prefix(target)?;
            if let Some(res) = self.resolve_candidates(path_matches, source_path) {
                return Ok(res);
            }
        }

        Ok(LinkResolution::Unresolved)
    }

    /// Populates `resolved_target_id` for all outgoing links from `source_id`.
    ///
    /// Called after `upsert()` so the cache is always current for freshly
    /// indexed files.
    pub(crate) fn resolve_outgoing_links(&mut self, source_id: &str) -> Result<(), IndexError> {
        // Get the source file's path for proximity tiebreaking.
        let source_path: Option<String> = self
            .conn
            .query_row(
                "SELECT path FROM files WHERE id = ?1",
                params![source_id],
                |row| row.get(0),
            )
            .ok();

        // Collect all links from this source (ends statement borrow before loop).
        let links: Vec<(i64, String)> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id, target FROM links WHERE source_id = ?1")?;
            let rows = stmt.query_map(params![source_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        // Resolve each link and write back the result.
        for (link_id, target) in &links {
            let resolved_id = match self.resolve_link(target, source_path.as_deref())? {
                LinkResolution::Resolved(id) => Some(id),
                _ => None,
            };
            self.conn.execute(
                "UPDATE links SET resolved_target_id = ?1 WHERE id = ?2",
                params![resolved_id, link_id],
            )?;
        }

        Ok(())
    }

    /// Re-resolves links whose `target` text matches `stem` (case-insensitive)
    /// and whose `resolved_target_id` is currently `NULL`.
    ///
    /// Call after inserting a new file: it might satisfy previously unresolved
    /// or ambiguous links. Also call after removing a file: previously
    /// resolved links have been SET NULL by the FK constraint and may now
    /// resolve to a surviving file with the same stem.
    pub(crate) fn re_resolve_links_matching_stem(&mut self, stem: &str) -> Result<(), IndexError> {
        // Collect affected links (source path needed for proximity).
        // Match both plain targets (`note`) and path-qualified targets
        // whose final component matches (`folder/note`).
        let path_suffix = format!("/{}", stem.to_lowercase());
        let links: Vec<(i64, String, Option<String>)> = {
            let mut stmt = self.conn.prepare(
                "SELECT l.id, l.target, f.path
                 FROM links l
                 JOIN files f ON f.id = l.source_id
                 WHERE (LOWER(l.target) = LOWER(?1)
                        OR LOWER(l.target) LIKE '%' || ?2)
                   AND l.resolved_target_id IS NULL",
            )?;
            let rows = stmt.query_map(params![stem, path_suffix], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        for (link_id, target, source_path) in &links {
            let resolved_id = match self.resolve_link(target, source_path.as_deref())? {
                LinkResolution::Resolved(id) => Some(id),
                _ => None,
            };
            if resolved_id.is_some() {
                self.conn.execute(
                    "UPDATE links SET resolved_target_id = ?1 WHERE id = ?2",
                    params![resolved_id, link_id],
                )?;
            }
        }

        Ok(())
    }

    /// Given a list of candidate [`crate::query::FileRecord`]s, reduce to a
    /// single [`LinkResolution`] using proximity tiebreaking when enabled.
    fn resolve_candidates(
        &self,
        candidates: Vec<crate::query::FileRecord>,
        source_path: Option<&str>,
    ) -> Option<LinkResolution> {
        match candidates.len() {
            0 => None,
            1 => Some(LinkResolution::Resolved(
                candidates.into_iter().next().unwrap().id,
            )),
            _ => {
                // Multiple candidates: attempt proximity tiebreaking.
                if self.proximity_enabled {
                    if let Some(src) = source_path {
                        let best = proximity_winner(&candidates, src);
                        if let Some(winner) = best {
                            return Some(LinkResolution::Resolved(winner));
                        }
                    }
                }
                // No tiebreaker resolved it: ambiguous.
                Some(LinkResolution::Ambiguous(
                    candidates.into_iter().map(|f| f.id).collect(),
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extracts the filename stem from a target string.
///
/// For path-qualified targets like `"folder/note"`, returns `"note"`.
/// For plain targets like `"note"`, returns `"note"`.
fn stem_from_target(target: &str) -> &str {
    target.rsplit('/').next().unwrap_or(target)
}

/// Returns the `NoteId` of the candidate with the shortest path distance
/// from `source_path`, or `None` if there is a tie at the shortest distance.
fn proximity_winner(candidates: &[crate::query::FileRecord], source_path: &str) -> Option<NoteId> {
    // Compute the path depth difference between source and each candidate.
    // Lower is closer (same directory = 0 directory components to traverse).
    let source_dir = parent_dir(source_path);

    let distances: Vec<usize> = candidates
        .iter()
        .map(|f| path_distance(source_dir, parent_dir(&f.path)))
        .collect();

    let min_dist = *distances.iter().min().unwrap();
    let winners: Vec<&crate::query::FileRecord> = candidates
        .iter()
        .zip(&distances)
        .filter(|(_, &d)| d == min_dist)
        .map(|(f, _)| f)
        .collect();

    if winners.len() == 1 {
        Some(winners[0].id.clone())
    } else {
        None // Tie even after proximity — leave as Ambiguous.
    }
}

/// Returns the directory portion of a path (everything before the last `/`).
fn parent_dir(path: &str) -> &str {
    path.rfind('/').map(|i| &path[..i]).unwrap_or("")
}

/// Computes a simple path distance metric: the number of directory components
/// that differ between two directory paths.
fn path_distance(a: &str, b: &str) -> usize {
    // Count how many components of `a` and `b` are NOT shared.
    let a_parts: Vec<&str> = if a.is_empty() {
        vec![]
    } else {
        a.split('/').collect()
    };
    let b_parts: Vec<&str> = if b.is_empty() {
        vec![]
    } else {
        b.split('/').collect()
    };

    let common = a_parts
        .iter()
        .zip(b_parts.iter())
        .take_while(|(x, y)| x.eq_ignore_ascii_case(y))
        .count();

    (a_parts.len() - common) + (b_parts.len() - common)
}
