//! Rename planning and application.
//!
//! Rename is a two-step preview/apply workflow:
//! 1. [`Index::plan_rename`] — computes all link text rewrites with no side effects.
//! 2. [`Index::apply_rename_index`] — updates paths and re-indexes in the SQLite layer
//!    after the Vault layer has already rewritten files and moved them on disk.
//!
//! # Resolution order during planning
//!
//! Links that resolve to the renamed file via an **alias** are not rewritten —
//! the alias stays valid after the filename changes. Only links whose `target`
//! text matches the old filename stem or old path-qualified path are updated.
//!
//! # Ambiguity after rename
//!
//! If the new filename stem already exists in the index as another file, the
//! rewritten link uses a path-qualified target (e.g., `[[folder/new-name]]`)
//! to remain unambiguous.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{Index, IndexError};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A single link-text rewrite within one file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RenameEdit {
    /// Vault-relative path of the file that needs to be rewritten.
    pub file_path: String,
    /// The original wiki-link text (e.g., `"[[old-name]]"`).
    pub before: String,
    /// The replacement wiki-link text (e.g., `"[[new-name]]"`).
    pub after: String,
}

/// The full set of rewrites required for a rename operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamePlan {
    /// The original vault-relative path being renamed.
    pub old_path: String,
    /// The new vault-relative path after rename.
    pub new_path: String,
    /// All link text edits across the vault. May be empty.
    pub edits: Vec<RenameEdit>,
}

// ---------------------------------------------------------------------------
// Public helpers
// ---------------------------------------------------------------------------

/// Extracts the filename stem from a vault-relative path.
///
/// `"notes/my-note.md"` → `"my-note"`,  `"root.md"` → `"root"`
pub(crate) fn stem_from_path(path: &str) -> &str {
    let filename = path.rsplit('/').next().unwrap_or(path);
    filename.strip_suffix(".md").unwrap_or(filename)
}

/// Applies all `RenameEdit` replacements for `path` to `content`.
///
/// All occurrences of each `before` string are replaced with `after`.
/// Multiple edits for the same file are applied sequentially.
pub fn rewrite_content(content: &str, path: &str, edits: &[RenameEdit]) -> String {
    let mut result = content.to_string();
    for edit in edits.iter().filter(|e| e.file_path == path) {
        result = result.replace(&edit.before, &edit.after);
    }
    result
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

impl Index {
    /// Computes the set of link rewrites required to rename `old_path` to
    /// `new_path`, without performing any I/O or index mutations.
    ///
    /// Handles both file renames (`*.md`) and directory renames (no `.md`
    /// extension — all indexed files under the directory are processed).
    pub fn plan_rename(&self, old_path: &str, new_path: &str) -> Result<RenamePlan, IndexError> {
        if old_path.ends_with(".md") {
            self.plan_single_file_rename(old_path, new_path)
        } else {
            self.plan_dir_rename(old_path, new_path)
        }
    }

    // -----------------------------------------------------------------------
    // Single-file planning
    // -----------------------------------------------------------------------

    fn plan_single_file_rename(
        &self,
        old_path: &str,
        new_path: &str,
    ) -> Result<RenamePlan, IndexError> {
        let old_stem = stem_from_path(old_path);
        let new_stem = stem_from_path(new_path);

        // Find the ID of the file being renamed.
        let file_id = match self.id_for_path(old_path)? {
            Some(id) => id,
            None => {
                return Ok(RenamePlan {
                    old_path: old_path.to_string(),
                    new_path: new_path.to_string(),
                    edits: vec![],
                });
            }
        };

        // Get aliases — links reaching the file via alias don't need rewriting.
        let aliases = self.get_aliases(&file_id)?;

        // Find all links that currently resolve to this file.
        let link_rows: Vec<(String, String, Option<String>, Option<String>)> = {
            let mut stmt = self.conn.prepare(
                "SELECT f.path, l.target, l.fragment, l.alias
                 FROM links l
                 JOIN files f ON f.id = l.source_id
                 WHERE l.resolved_target_id = ?1",
            )?;
            let rows = stmt.query_map(params![file_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut edits: Vec<RenameEdit> = Vec::new();

        for (source_path, target, fragment, link_alias) in &link_rows {
            // Skip alias-based links — they remain valid after rename.
            if aliases.iter().any(|a| a.eq_ignore_ascii_case(target)) {
                continue;
            }

            let new_target = determine_new_target(
                target,
                old_stem,
                new_stem,
                old_path,
                new_path,
                source_path,
                self,
            )?;

            // Skip if the rewrite produces no change.
            if new_target.eq_ignore_ascii_case(target) {
                continue;
            }

            let before = format_wiki_link(target, fragment.as_deref(), link_alias.as_deref());
            let after = format_wiki_link(&new_target, fragment.as_deref(), link_alias.as_deref());

            if before != after {
                edits.push(RenameEdit {
                    file_path: source_path.clone(),
                    before,
                    after,
                });
            }
        }

        // Deduplicate: same (file_path, before) should only appear once.
        // When before==after after dedup we still keep the unique before pattern.
        edits.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then_with(|| a.before.cmp(&b.before))
        });
        edits.dedup_by(|a, b| a.file_path == b.file_path && a.before == b.before);

        Ok(RenamePlan {
            old_path: old_path.to_string(),
            new_path: new_path.to_string(),
            edits,
        })
    }

    // -----------------------------------------------------------------------
    // Directory planning
    // -----------------------------------------------------------------------

    fn plan_dir_rename(&self, old_dir: &str, new_dir: &str) -> Result<RenamePlan, IndexError> {
        let old_prefix = normalize_dir_prefix(old_dir);
        let new_prefix = normalize_dir_prefix(new_dir);

        // Find every indexed file inside old_dir.
        let files = self.files_by_path_prefix(&old_prefix)?;

        let mut all_edits: Vec<RenameEdit> = Vec::new();

        for file in &files {
            let suffix = &file.path[old_prefix.len()..];
            let new_file_path = format!("{}{}", new_prefix, suffix);
            let plan = self.plan_single_file_rename(&file.path, &new_file_path)?;
            all_edits.extend(plan.edits);
        }

        // Deduplicate across the combined edit set.
        all_edits.sort_by(|a, b| {
            a.file_path
                .cmp(&b.file_path)
                .then_with(|| a.before.cmp(&b.before))
        });
        all_edits.dedup_by(|a, b| a.file_path == b.file_path && a.before == b.before);

        Ok(RenamePlan {
            old_path: old_dir.to_string(),
            new_path: new_dir.to_string(),
            edits: all_edits,
        })
    }
}

// ---------------------------------------------------------------------------
// Application (index-only)
// ---------------------------------------------------------------------------

impl Index {
    /// Updates the SQLite index after a file rename has already been applied
    /// on disk by the Vault layer.
    ///
    /// - Updates `files.path` for the renamed file (preserves its `id`).
    /// - Re-upserts all modified files (rewritten links + renamed file).
    /// - Re-resolves links affected by the rename.
    pub fn apply_rename_index(
        &mut self,
        old_path: &str,
        new_path: &str,
        // Files to re-index: (vault-relative path, mtime_secs, raw markdown content).
        // Must include the renamed file at new_path.
        modified_files: &[(String, i64, String)],
    ) -> Result<(), IndexError> {
        // Move the path pointer — ID and all child FKs stay stable.
        self.conn.execute(
            "UPDATE files SET path = ?1 WHERE path = ?2",
            params![new_path, old_path],
        )?;
        self.conn.execute(
            "UPDATE fts SET path = ?1 WHERE path = ?2",
            params![new_path, old_path],
        )?;

        // Re-upsert every modified file so links and metadata are current.
        for (path, mtime, content) in modified_files {
            let note = tektite_parser::parse(content);
            self.upsert(path, *mtime, &note)?;
        }

        // Re-resolve links that may have been affected by the rename but were
        // not covered by the files re-upserted above.
        let old_stem = stem_from_path(old_path);
        let new_stem = stem_from_path(new_path);
        self.re_resolve_links_matching_stem(old_stem)?;
        if new_stem != old_stem {
            self.re_resolve_links_matching_stem(new_stem)?;
        }

        Ok(())
    }

    /// Updates the index after a directory rename on disk.
    ///
    /// - Updates `files.path` for every file inside the renamed directory.
    /// - Re-upserts all modified files.
    pub fn apply_dir_rename_index(
        &mut self,
        _old_dir: &str,
        _new_dir: &str,
        // (old_path, new_path) pairs for every file in the renamed directory.
        file_renames: &[(String, String)],
        // Files to re-index: (path, mtime_secs, raw content).
        modified_files: &[(String, i64, String)],
    ) -> Result<(), IndexError> {
        for (old_path, new_path) in file_renames {
            self.conn.execute(
                "UPDATE files SET path = ?1 WHERE path = ?2",
                params![new_path, old_path],
            )?;
            self.conn.execute(
                "UPDATE fts SET path = ?1 WHERE path = ?2",
                params![new_path, old_path],
            )?;
        }

        for (path, mtime, content) in modified_files {
            let note = tektite_parser::parse(content);
            self.upsert(path, *mtime, &note)?;
        }

        // Re-resolve any remaining unresolved links related to the moved stems.
        for (old_path, new_path) in file_renames {
            let old_stem = stem_from_path(old_path);
            let new_stem = stem_from_path(new_path);
            self.re_resolve_links_matching_stem(old_stem)?;
            if new_stem != old_stem {
                self.re_resolve_links_matching_stem(new_stem)?;
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Reconstructs a `[[wiki-link]]` string from its components.
pub(crate) fn format_wiki_link(
    target: &str,
    fragment: Option<&str>,
    alias: Option<&str>,
) -> String {
    let mut s = format!("[[{target}");
    if let Some(frag) = fragment {
        s.push('#');
        s.push_str(frag);
    }
    if let Some(a) = alias {
        s.push('|');
        s.push_str(a);
    }
    s.push_str("]]");
    s
}

/// Determines what the link `target` text should become after `old_path` is
/// renamed to `new_path`.
///
/// Rules:
/// - If `target` matches `old_stem` → use `new_stem` (or path-qualified if
///   `new_stem` would be ambiguous from `source_path`).
/// - If `target` matches `old_path` without `.md` → use `new_path` without `.md`.
/// - Otherwise → return `target` unchanged.
fn determine_new_target(
    target: &str,
    old_stem: &str,
    new_stem: &str,
    old_path: &str,
    new_path: &str,
    source_path: &str,
    index: &Index,
) -> Result<String, IndexError> {
    let old_path_no_ext = old_path.strip_suffix(".md").unwrap_or(old_path);
    let new_path_no_ext = new_path.strip_suffix(".md").unwrap_or(new_path);

    // Case 1: plain stem link  [[old-stem]]
    if target.eq_ignore_ascii_case(old_stem) {
        // Would [[new_stem]] be ambiguous after rename? Count other indexed
        // files with the same stem (excluding the file being renamed itself,
        // which currently still has old_path).
        let new_stem_candidates = index.files_by_stem(new_stem)?;
        let collisions = new_stem_candidates
            .iter()
            .filter(|f| !f.path.eq_ignore_ascii_case(old_path))
            .count();

        if collisions > 0 {
            // Use path-qualified target to avoid post-rename ambiguity.
            return Ok(new_path_no_ext.to_string());
        }
        return Ok(new_stem.to_string());
    }

    // Case 2: path-qualified link  [[folder/old-stem]]
    // Always keep path-qualified — preserves the user's intent.
    if target.eq_ignore_ascii_case(old_path_no_ext) {
        return Ok(new_path_no_ext.to_string());
    }

    // No change needed.
    let _ = source_path; // currently unused beyond the above cases
    Ok(target.to_string())
}

/// Returns a directory prefix string with a guaranteed trailing slash.
fn normalize_dir_prefix(dir: &str) -> String {
    let trimmed = dir.trim_end_matches('/');
    format!("{}/", trimmed)
}
