//! `tektite-search` — Search ranking and fuzzy matching over indexed vault data.
//!
//! Combines FTS content/title matches, fuzzy filename matches, and heading
//! matches into a single ranked result list suitable for the v0.1 sidebar.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use tektite_index::{Index, NoteId};

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

/// A single search result shown in the sidebar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The note's stable identifier. For v0.1 this is the vault-relative path.
    pub id: NoteId,
    /// Vault-relative path.
    pub path: String,
    /// Display title.
    pub title: String,
    /// Contextual snippet with match highlights or match reason.
    pub snippet: String,
    /// Raw relevance rank for debugging/UI tie-breakers.
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

#[derive(Debug, Clone)]
struct RankedSearchResult {
    result: SearchResult,
    score: i64,
}

pub fn search(index: &Index, query: &str, limit: usize) -> Result<Vec<SearchResult>, SearchError> {
    let trimmed = query.trim();
    if trimmed.is_empty() || limit == 0 {
        return Ok(vec![]);
    }

    let expanded_limit = limit.saturating_mul(3).max(limit);
    let fts_results = index.search_fts(trimmed, expanded_limit)?;
    let fuzzy_results = index.search_fuzzy_files(trimmed, expanded_limit)?;
    let heading_results = index.search_headings(trimmed, expanded_limit)?;
    let query_lower = trimmed.to_lowercase();

    let mut by_path: HashMap<String, RankedSearchResult> = HashMap::new();

    for row in fuzzy_results {
        let path = row.path;
        let title = title_from_name_or_path(&row.name, &path);
        let snippet = format!("Filename match · {}", path);
        let result = SearchResult {
            id: path.clone(),
            path: path.clone(),
            title,
            snippet,
            rank: -(row.score as f64),
        };
        insert_candidate(
            &mut by_path,
            RankedSearchResult {
                result,
                score: 5000 + row.score,
            },
        );
    }

    for row in fts_results {
        let path = row.path;
        let title = title_from_name_or_path(&row.title, &path);
        let snippet = normalize_snippet(&row.snippet, &title, &path);
        let title_bonus = if row.title.trim().eq_ignore_ascii_case(trimmed) {
            1200
        } else if row.title.to_lowercase().contains(&query_lower) {
            500
        } else {
            0
        };
        let result = SearchResult {
            id: path.clone(),
            path: path.clone(),
            title,
            snippet,
            rank: row.rank,
        };
        insert_candidate(
            &mut by_path,
            RankedSearchResult {
                result,
                score: 3000 + title_bonus + bm25_score(row.rank),
            },
        );
    }

    for row in heading_results {
        let path = row.file_path;
        let title = title_from_name_or_path("", &path);
        let snippet = format!(
            "Heading match · {} {}",
            "#".repeat(row.level as usize),
            row.text
        );
        let result = SearchResult {
            id: path.clone(),
            path: path.clone(),
            title,
            snippet,
            rank: -1500.0 + row.level as f64,
        };
        insert_candidate(
            &mut by_path,
            RankedSearchResult {
                result,
                score: 2000 - row.level as i64,
            },
        );
    }

    let mut ranked = by_path.into_values().collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.result.path.cmp(&b.result.path))
    });
    ranked.truncate(limit);

    Ok(ranked.into_iter().map(|entry| entry.result).collect())
}

fn insert_candidate(
    by_path: &mut HashMap<String, RankedSearchResult>,
    candidate: RankedSearchResult,
) {
    match by_path.get_mut(&candidate.result.path) {
        Some(existing) if candidate.score > existing.score => *existing = candidate,
        None => {
            by_path.insert(candidate.result.path.clone(), candidate);
        }
        _ => {}
    }
}

fn title_from_name_or_path(title: &str, path: &str) -> String {
    let trimmed = title.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    Path::new(path)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(path)
        .to_string()
}

fn normalize_snippet(snippet: &str, title: &str, path: &str) -> String {
    let trimmed = snippet.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    format!("{} · {}", title_from_name_or_path(title, path), path)
}

fn bm25_score(rank: f64) -> i64 {
    let normalized = (-rank * 100.0).round() as i64;
    normalized.clamp(-1000, 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_index() -> Index {
        let mut index = Index::open_in_memory().expect("index");
        let project = tektite_parser::parse(
            "---\ntitle: Project Atlas\n---\n# Project Atlas\nLaunch plans and roadmap\n",
        );
        let meeting = tektite_parser::parse(
            "# Weekly Meeting\nDiscussed Atlas roadmap and launch blockers\n",
        );
        let ideas = tektite_parser::parse("# Ideas\n[[Project Atlas]]\n# Atlas heading\n");

        index
            .upsert("notes/project-atlas.md", 1, &project)
            .expect("project");
        index
            .upsert("notes/weekly-meeting.md", 1, &meeting)
            .expect("meeting");
        index.upsert("ideas.md", 1, &ideas).expect("ideas");
        index
    }

    #[test]
    fn combined_search_prefers_filename_matches() {
        let index = seed_index();
        let results = search(&index, "project atlas", 5).expect("search");

        assert_eq!(
            results.first().map(|row| row.path.as_str()),
            Some("notes/project-atlas.md")
        );
    }

    #[test]
    fn combined_search_falls_back_to_heading_matches() {
        let index = seed_index();
        let results = search(&index, "atlas heading", 5).expect("search");

        assert!(results.iter().any(|row| row.path == "ideas.md"));
    }
}
