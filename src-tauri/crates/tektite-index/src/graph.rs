//! Local-neighborhood graph queries.
//!
//! Phase 0 of the graph-view plan: a BFS walk from a center note over the
//! `links` table (following both outgoing and incoming edges) with filters
//! and a hard node cap. No UI, no semantic edges — just the data layer that
//! the frontend graph panel will consume.

use std::collections::{HashMap, HashSet, VecDeque};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{Index, IndexError, NoteId};

/// Default BFS depth from the center note.
pub const DEFAULT_DEPTH: u8 = 1;
/// Upper bound on depth. Higher values fan out explosively in a densely
/// linked vault.
pub const MAX_DEPTH: u8 = 3;
/// Hard cap on returned nodes. When exceeded, prune lowest link_count first
/// (never dropping the center).
pub const NODE_CAP: usize = 50;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphFilters {
    /// OR-semantic tag filter — a node matches if any of its tags is in this list.
    /// `None` or empty list means no tag filtering.
    pub tags: Option<Vec<String>>,
    /// Folder path prefix (vault-relative). `None` means no folder filter.
    pub folder: Option<String>,
    /// Unix seconds; nodes with `mtime_secs < modified_after` are excluded.
    pub modified_after: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NoteId,
    pub path: String,
    pub title: String,
    pub tags: Vec<String>,
    /// Unix seconds since epoch.
    pub modified: i64,
    /// Total links touching this note (outgoing + resolved incoming).
    pub link_count: u32,
    /// True when the note has at least one stored chunk embedding —
    /// lets the frontend surface progressive embedding state without
    /// a second query.
    #[serde(default)]
    pub has_embedding: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: NoteId,
    pub target: NoteId,
    /// Discriminator for the frontend: `"link"` for wiki-link edges,
    /// `"semantic"` for embedding-similarity edges.
    pub kind: String,
    /// Cosine similarity for `"semantic"` edges; `None` for `"link"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphData {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl Index {
    /// Returns the link-neighborhood around `center_id` up to `depth` hops.
    ///
    /// BFS follows both outgoing links (via `get_links`) and incoming ones
    /// (via `get_backlinks`), deduplicating nodes and edges. Unresolved
    /// outgoing links (no `resolved_target_id`) are skipped — edges require
    /// both endpoints to exist in the index.
    ///
    /// Filters are applied to every node *except the center* during BFS so
    /// that filtered-out notes don't seed further expansion.
    ///
    /// If the collected node set exceeds [`NODE_CAP`], the lowest-`link_count`
    /// leaves are dropped (keeping the center) and edges that lose an
    /// endpoint are pruned.
    pub fn neighborhood(
        &self,
        center_id: &str,
        depth: u8,
        filters: &GraphFilters,
    ) -> Result<GraphData, IndexError> {
        let depth = depth.clamp(1, MAX_DEPTH);

        if self.path_for_id(center_id)?.is_none() {
            return Ok(GraphData {
                nodes: Vec::new(),
                edges: Vec::new(),
            });
        }

        let mut visited: HashSet<NoteId> = HashSet::new();
        let mut excluded: HashSet<NoteId> = HashSet::new();
        let mut edge_set: HashSet<(NoteId, NoteId)> = HashSet::new();
        let mut queue: VecDeque<(NoteId, u8)> = VecDeque::new();

        visited.insert(center_id.to_string());
        queue.push_back((center_id.to_string(), 0));

        while let Some((node_id, d)) = queue.pop_front() {
            if d >= depth {
                continue;
            }

            for link in self.get_links(&node_id)? {
                let Some(target_id) = link.resolved_target_id else {
                    continue;
                };
                if target_id == node_id {
                    continue;
                }
                if self.consider_neighbor(
                    &target_id,
                    filters,
                    &mut visited,
                    &mut excluded,
                    &mut queue,
                    d,
                )? {
                    edge_set.insert((node_id.clone(), target_id));
                }
            }

            for link in self.get_backlinks(&node_id)? {
                let source_id = link.source_id;
                if source_id == node_id {
                    continue;
                }
                if self.consider_neighbor(
                    &source_id,
                    filters,
                    &mut visited,
                    &mut excluded,
                    &mut queue,
                    d,
                )? {
                    edge_set.insert((source_id, node_id.clone()));
                }
            }
        }

        let mut node_map = self.graph_node_metadata(&visited)?;

        if node_map.len() > NODE_CAP {
            let mut by_link_count: Vec<(NoteId, u32)> = node_map
                .iter()
                .filter(|(id, _)| id.as_str() != center_id)
                .map(|(id, n)| (id.clone(), n.link_count))
                .collect();
            by_link_count.sort_by_key(|(_, c)| *c);
            let overflow = node_map.len() - NODE_CAP;
            for (id, _) in by_link_count.into_iter().take(overflow) {
                node_map.remove(&id);
            }
        }

        let mut edges: Vec<GraphEdge> = edge_set
            .into_iter()
            .filter(|(s, t)| node_map.contains_key(s) && node_map.contains_key(t))
            .map(|(source, target)| GraphEdge {
                source,
                target,
                kind: "link".to_string(),
                score: None,
            })
            .collect();
        edges.sort_by(|a, b| (a.source.as_str(), a.target.as_str()).cmp(&(b.source.as_str(), b.target.as_str())));

        let mut nodes: Vec<GraphNode> = node_map.into_values().collect();
        nodes.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(GraphData { nodes, edges })
    }

    /// Inspects a candidate neighbor: returns `true` if the edge to/from it
    /// should be recorded (i.e. the candidate is admissible), and schedules
    /// further expansion if we haven't visited it yet.
    fn consider_neighbor(
        &self,
        candidate: &str,
        filters: &GraphFilters,
        visited: &mut HashSet<NoteId>,
        excluded: &mut HashSet<NoteId>,
        queue: &mut VecDeque<(NoteId, u8)>,
        current_depth: u8,
    ) -> Result<bool, IndexError> {
        if visited.contains(candidate) {
            return Ok(true);
        }
        if excluded.contains(candidate) {
            return Ok(false);
        }
        if self.node_passes_filter(candidate, filters)? {
            visited.insert(candidate.to_string());
            queue.push_back((candidate.to_string(), current_depth + 1));
            Ok(true)
        } else {
            excluded.insert(candidate.to_string());
            Ok(false)
        }
    }

    fn node_passes_filter(
        &self,
        id: &str,
        filters: &GraphFilters,
    ) -> Result<bool, IndexError> {
        let row: Option<(String, i64)> = self
            .conn
            .query_row(
                "SELECT path, mtime_secs FROM files WHERE id = ?1",
                params![id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
            )
            .ok();
        let Some((path, mtime)) = row else {
            return Ok(false);
        };

        if let Some(folder) = filters.folder.as_deref() {
            if !folder.is_empty() && !path.starts_with(folder) {
                return Ok(false);
            }
        }
        if let Some(after) = filters.modified_after {
            if mtime < after {
                return Ok(false);
            }
        }
        if let Some(wanted) = filters.tags.as_ref() {
            if !wanted.is_empty() {
                let tags: Vec<String> = self
                    .get_tags(id)?
                    .into_iter()
                    .map(|t| t.name)
                    .collect();
                if !wanted.iter().any(|w| tags.iter().any(|t| t == w)) {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }

    /// Batch-fetches `GraphNode` metadata (path, title, tags, modified,
    /// link_count) for a set of note IDs. Missing IDs are simply absent
    /// from the returned map rather than an error.
    pub fn graph_node_metadata(
        &self,
        ids: &HashSet<NoteId>,
    ) -> Result<HashMap<NoteId, GraphNode>, IndexError> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        let placeholders = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql_params: Vec<&dyn rusqlite::ToSql> =
            ids.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let file_sql = format!(
            "SELECT f.id,
                    f.path,
                    f.mtime_secs,
                    COALESCE(NULLIF(fts.title, ''), f.path) AS title,
                    (SELECT COUNT(*) FROM links l
                       WHERE l.source_id = f.id
                          OR l.resolved_target_id = f.id) AS link_count,
                    EXISTS(SELECT 1 FROM chunks c WHERE c.file_id = f.id) AS has_embedding
             FROM files f
             LEFT JOIN fts ON fts.path = f.path
             WHERE f.id IN ({placeholders})"
        );
        let mut stmt = self.conn.prepare(&file_sql)?;
        let rows = stmt.query_map(sql_params.as_slice(), |row| {
            let link_count: i64 = row.get(4)?;
            let has_embedding: i64 = row.get(5)?;
            Ok(GraphNode {
                id: row.get(0)?,
                path: row.get(1)?,
                modified: row.get(2)?,
                title: row.get(3)?,
                tags: Vec::new(),
                link_count: link_count.max(0) as u32,
                has_embedding: has_embedding != 0,
            })
        })?;
        let mut map: HashMap<NoteId, GraphNode> = HashMap::new();
        for node in rows {
            let n = node?;
            map.insert(n.id.clone(), n);
        }

        let tag_sql = format!(
            "SELECT file_id, name FROM tags WHERE file_id IN ({placeholders})"
        );
        let mut tag_stmt = self.conn.prepare(&tag_sql)?;
        let tag_rows = tag_stmt.query_map(sql_params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in tag_rows {
            let (file_id, tag) = row?;
            if let Some(node) = map.get_mut(&file_id) {
                node.tags.push(tag);
            }
        }
        for node in map.values_mut() {
            node.tags.sort();
            node.tags.dedup();
        }

        Ok(map)
    }

    /// Returns every indexed markdown note plus the resolved wiki-link edges
    /// between them. Filters are applied to nodes; edges whose endpoints are
    /// filtered out are dropped.
    ///
    /// This is the whole-vault data source for the main-view graph tab. It
    /// intentionally returns no semantic edges — those arrive via
    /// [`EmbedService`]'s mutual-kNN command so progress can be surfaced
    /// independently.
    pub fn full_vault(&self, filters: &GraphFilters) -> Result<GraphData, IndexError> {
        let mut visited: HashSet<NoteId> = HashSet::new();
        {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM files WHERE LOWER(path) LIKE '%.md'")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            for row in rows {
                let id = row?;
                if self.node_passes_filter(&id, filters)? {
                    visited.insert(id);
                }
            }
        }

        if visited.is_empty() {
            return Ok(GraphData {
                nodes: Vec::new(),
                edges: Vec::new(),
            });
        }

        let node_map = self.graph_node_metadata(&visited)?;

        let mut edge_set: HashSet<(NoteId, NoteId)> = HashSet::new();
        {
            let mut stmt = self.conn.prepare(
                "SELECT source_id, resolved_target_id FROM links
                 WHERE resolved_target_id IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            for row in rows {
                let (source, target) = row?;
                if source == target {
                    continue;
                }
                if !node_map.contains_key(&source) || !node_map.contains_key(&target) {
                    continue;
                }
                edge_set.insert((source, target));
            }
        }

        let mut edges: Vec<GraphEdge> = edge_set
            .into_iter()
            .map(|(source, target)| GraphEdge {
                source,
                target,
                kind: "link".to_string(),
                score: None,
            })
            .collect();
        edges.sort_by(|a, b| {
            (a.source.as_str(), a.target.as_str()).cmp(&(b.source.as_str(), b.target.as_str()))
        });

        let mut nodes: Vec<GraphNode> = node_map.into_values().collect();
        nodes.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(GraphData { nodes, edges })
    }
}
