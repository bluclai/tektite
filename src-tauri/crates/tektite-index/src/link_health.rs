//! Unresolved-link health queries and vault-wide aggregate stats.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{Index, IndexError, LinkResolution};

const SAMPLE_SOURCE_LIMIT: usize = 3;

// ---------------------------------------------------------------------------
// Vault stats
// ---------------------------------------------------------------------------

/// Aggregate counts for the vault — cheap to compute, used by the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultStats {
    pub note_count: u32,
    pub link_count: u32,
    pub unresolved_link_count: u32,
}

impl Index {
    /// Returns aggregate vault statistics in a single read pass.
    ///
    /// Three `COUNT(*)` queries — expected to complete in <5 ms on large vaults
    /// because every column referenced is a primary key or indexed foreign key.
    pub fn vault_stats(&self) -> Result<VaultStats, IndexError> {
        let note_count = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get::<_, i64>(0))?
            as u32;

        let link_count = self
            .conn
            .query_row("SELECT COUNT(*) FROM links", [], |r| r.get::<_, i64>(0))?
            as u32;

        let unresolved_link_count = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM links WHERE resolved_target_id IS NULL",
                [],
                |r| r.get::<_, i64>(0),
            )?
            as u32;

        Ok(VaultStats {
            note_count,
            link_count,
            unresolved_link_count,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnresolvedTargetKind {
    Unresolved,
    Ambiguous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedTargetRow {
    pub target: String,
    pub kind: UnresolvedTargetKind,
    pub reference_count: usize,
    pub sample_sources: Vec<String>,
    pub has_more_sources: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedReport {
    pub rows: Vec<UnresolvedTargetRow>,
    pub total_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnresolvedSourceRef {
    pub source_path: String,
    pub source_title: String,
    pub target: String,
    pub fragment: Option<String>,
    pub alias: Option<String>,
}

#[derive(Debug)]
struct RawUnresolvedLink {
    target: String,
    source_path: String,
}

impl Index {
    /// Returns unresolved wiki-link targets grouped case-insensitively.
    ///
    /// Rows are sorted by reference count descending, then target alphabetically.
    /// `total_count` reflects the full grouped row count before the limit is applied.
    pub fn report_unresolved(&self, limit: usize) -> Result<UnresolvedReport, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT l.target, src.path
             FROM links l
             JOIN files src ON src.id = l.source_id
             WHERE l.resolved_target_id IS NULL
             ORDER BY LOWER(l.target), LOWER(src.path), l.id",
        )?;

        let raw_rows = stmt.query_map(params![], |row| {
            Ok(RawUnresolvedLink {
                target: row.get(0)?,
                source_path: row.get(1)?,
            })
        })?;

        let mut grouped: Vec<(String, UnresolvedTargetRow)> = Vec::new();

        for row in raw_rows {
            let row = row?;
            let normalized = row.target.to_lowercase();

            if let Some((_, existing)) = grouped.iter_mut().find(|(key, _)| *key == normalized) {
                existing.reference_count += 1;
                if existing.sample_sources.len() < SAMPLE_SOURCE_LIMIT {
                    existing.sample_sources.push(row.source_path);
                } else {
                    existing.has_more_sources = true;
                }
                continue;
            }

            grouped.push((
                normalized,
                UnresolvedTargetRow {
                    target: row.target,
                    kind: UnresolvedTargetKind::Unresolved,
                    reference_count: 1,
                    sample_sources: vec![row.source_path],
                    has_more_sources: false,
                },
            ));
        }

        let total_count = grouped.len();
        let mut rows: Vec<UnresolvedTargetRow> = grouped.into_iter().map(|(_, row)| row).collect();

        for row in &mut rows {
            row.kind = match self.resolve_link(&row.target, None)? {
                LinkResolution::Ambiguous(_) => UnresolvedTargetKind::Ambiguous,
                LinkResolution::Resolved(_) | LinkResolution::Unresolved => {
                    UnresolvedTargetKind::Unresolved
                }
            };
        }

        rows.sort_by(|a, b| {
            b.reference_count
                .cmp(&a.reference_count)
                .then_with(|| a.target.to_lowercase().cmp(&b.target.to_lowercase()))
        });
        rows.truncate(limit);

        Ok(UnresolvedReport { rows, total_count })
    }

    /// Returns source references for a grouped unresolved target.
    ///
    /// Matching is case-insensitive on the raw target text and results are
    /// sorted deterministically by source path, then link id.
    pub fn unresolved_target_sources(
        &self,
        target: &str,
        limit: usize,
    ) -> Result<Vec<UnresolvedSourceRef>, IndexError> {
        let mut stmt = self.conn.prepare(
            "SELECT src.path,
                    COALESCE(NULLIF(fts.title, ''), src.path),
                    l.target,
                    l.fragment,
                    l.alias
             FROM links l
             JOIN files src ON src.id = l.source_id
             LEFT JOIN fts ON fts.path = src.path
             WHERE l.resolved_target_id IS NULL
               AND LOWER(l.target) = LOWER(?1)
             ORDER BY LOWER(src.path), l.id
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(params![target, limit as i64], |row| {
            Ok(UnresolvedSourceRef {
                source_path: row.get(0)?,
                source_title: row.get(1)?,
                target: row.get(2)?,
                fragment: row.get(3)?,
                alias: row.get(4)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_index() -> Index {
        let mut index = Index::open_in_memory().expect("index");

        // note-a links to note-b (resolved) and to ghost (unresolved)
        let parsed_a = tektite_parser::parse("# Note A\n[[note-b]] [[ghost]]\n");
        // note-b links to note-a (resolved)
        let parsed_b = tektite_parser::parse("# Note B\n[[note-a]]\n");
        // note-c has no links
        let parsed_c = tektite_parser::parse("# Note C\nJust text.\n");

        index.upsert("note-a.md", 1, &parsed_a).expect("a");
        index.upsert("note-b.md", 1, &parsed_b).expect("b");
        index.upsert("note-c.md", 1, &parsed_c).expect("c");

        index
    }

    #[test]
    fn vault_stats_counts_are_correct() {
        let index = seed_index();
        let stats = index.vault_stats().expect("stats");

        assert_eq!(stats.note_count, 3);
        // note-a: 2 links; note-b: 1 link; note-c: 0 links
        assert_eq!(stats.link_count, 3);
        // only [[ghost]] from note-a is unresolved
        assert_eq!(stats.unresolved_link_count, 1);
    }

    #[test]
    fn vault_stats_empty_index() {
        let index = Index::open_in_memory().expect("index");
        let stats = index.vault_stats().expect("stats");

        assert_eq!(stats.note_count, 0);
        assert_eq!(stats.link_count, 0);
        assert_eq!(stats.unresolved_link_count, 0);
    }

    #[test]
    fn vault_stats_all_resolved() {
        let mut index = Index::open_in_memory().expect("index");

        let parsed_a = tektite_parser::parse("# A\n[[b]]\n");
        let parsed_b = tektite_parser::parse("# B\n[[a]]\n");
        index.upsert("a.md", 1, &parsed_a).expect("a");
        index.upsert("b.md", 1, &parsed_b).expect("b");

        let stats = index.vault_stats().expect("stats");
        assert_eq!(stats.note_count, 2);
        assert_eq!(stats.link_count, 2);
        assert_eq!(stats.unresolved_link_count, 0);
    }
}
