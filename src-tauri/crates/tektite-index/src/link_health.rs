//! Unresolved-link health queries.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{Index, IndexError, LinkResolution};

const SAMPLE_SOURCE_LIMIT: usize = 3;

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
