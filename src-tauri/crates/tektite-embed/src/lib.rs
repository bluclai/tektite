//! `tektite-embed` — Semantic vault index.
//!
//! Chunks markdown notes on heading boundaries, embeds each chunk with a
//! local model, stores raw f32 vectors alongside the existing SQLite index,
//! and serves cosine-similarity search out of an in-memory cache.
//!
//! Phase 1 is synchronous and uses a [`FakeEmbedder`]; Phase 2 swaps in
//! `OnnxEmbedder`, and Phase 3 moves embedding onto a background thread.
//!
//! Internal modules:
//! - [`chunker`]  — heading-based splitting
//! - [`embedder`] — [`Embedder`] trait + [`FakeEmbedder`]
//! - [`store`]    — SQLite persistence for `chunks` rows
//! - [`cache`]    — in-memory vector cache + cosine search

pub mod cache;
pub mod chunker;
pub mod embedder;
pub mod mutual_knn;
pub mod onnx;
pub mod queue;
pub mod store;

use std::path::Path;
use std::sync::Arc;

use thiserror::Error;

pub use cache::{Cache, CacheEntry};
pub use chunker::{chunk_note, Chunk};
pub use embedder::{Embedder, FakeEmbedder};
pub use mutual_knn::{
    compute_mutual_knn, KnnProgress, MutualKnnEdge, MutualKnnOptions,
};
pub use onnx::OnnxEmbedder;
pub use queue::{EmbedJob, EmbedProgress, EmbedQueue, Priority};
pub use store::{ChunkMetadata, ChunkRecord, Store};

/// Dimensionality of all stored vectors. Matryoshka-truncated from 768 in
/// Phase 2; the 256-dim commitment is frozen into the storage format
/// (raw f32 BLOB = 1024 bytes) so swapping later requires a re-embed, not
/// a schema migration.
pub const EMBED_DIM: usize = 256;

/// A raw embedding vector owned by the cache.
pub type Vector = [f32; EMBED_DIM];

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("embedder error: {0}")]
    Embedder(String),
    #[error("dimension mismatch: expected {expected}, got {actual}")]
    Dim { expected: usize, actual: usize },
    #[error("BLOB size mismatch: expected {expected} bytes, got {actual}")]
    BlobSize { expected: usize, actual: usize },
}

/// A single semantic search result.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SemanticHit {
    pub chunk_id: String,
    pub file_path: String,
    pub heading_path: Option<String>,
    /// Leaf heading text — what the chunk's section is titled
    /// (e.g. `"Setup"` from `"Intro / Setup"`). Used by the frontend to
    /// scroll directly to the heading on click without re-parsing
    /// `heading_path`. `None` when the chunk has no heading.
    pub heading_text: Option<String>,
    /// Markdown level of the leaf heading (1–6). `None` when
    /// `heading_text` is `None`.
    pub heading_level: Option<u8>,
    pub snippet: String,
    pub score: f32,
}

/// Top-level façade: holds the embedder, store, cache, and (in background
/// mode) the embed queue. Owned by the vault for its lifetime and shared
/// with the Tauri command layer.
pub struct EmbedService {
    /// Query-time embedder — used only by `search_semantic` to embed the
    /// user's query string. In background mode the *document* embedder
    /// lives on the queue's worker thread.
    query_embedder: Box<dyn Embedder>,
    store: Store,
    cache: Cache,
    /// Background queue. `None` in sync mode (tests / Phase 1 compat).
    queue: Option<EmbedQueue>,
}

impl EmbedService {
    /// Opens the embed service in **synchronous** mode — embedding happens
    /// inline in `reindex_file`. Used by tests and as a fallback.
    pub fn open(
        db_path: &Path,
        embedder: Box<dyn Embedder>,
    ) -> Result<Self, EmbedError> {
        let store = Store::open(db_path)?;
        let cache = Cache::new();
        cache.load_all_from_store(&store)?;
        Ok(Self {
            query_embedder: embedder,
            store,
            cache,
            queue: None,
        })
    }

    /// Opens the embed service in **background** mode.
    ///
    /// Document embeddings are processed on a dedicated worker thread via
    /// [`EmbedQueue`]. The `embedder_factory` is called lazily on that
    /// thread when the first job arrives (or on prewarm). The
    /// `query_embedder` is used synchronously for search queries on the
    /// calling thread.
    ///
    /// `on_progress` fires after every completed job.
    pub fn open_background<F, P>(
        db_path: &Path,
        query_embedder: Box<dyn Embedder>,
        embedder_factory: F,
        on_progress: P,
    ) -> Result<Self, EmbedError>
    where
        F: FnOnce() -> Box<dyn Embedder> + Send + 'static,
        P: Fn(EmbedProgress) + Send + 'static,
    {
        let store = Store::open(db_path)?;
        let cache = Cache::new();
        cache.load_all_from_store(&store)?;

        // The queue gets its own Store connection so it can write from the
        // worker thread without contending with the main-thread reads.
        let queue_store = Store::open(db_path)?;
        let queue = EmbedQueue::start(queue_store, cache.clone(), embedder_factory, on_progress);

        Ok(Self {
            query_embedder,
            store,
            cache,
            queue: Some(queue),
        })
    }

    /// Constructor intended for tests — uses the in-memory SQLite DB for
    /// `chunks`. The shared-file-id contract is the caller's responsibility.
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_in_memory(embedder: Box<dyn Embedder>) -> Result<Self, EmbedError> {
        let store = Store::open_in_memory()?;
        Ok(Self {
            query_embedder: embedder,
            store,
            cache: Cache::new(),
            queue: None,
        })
    }

    /// Re-chunk and re-embed one file.
    ///
    /// In **background mode** the work is queued on the worker thread and
    /// returns immediately. The cache will be updated asynchronously.
    ///
    /// In **sync mode** (tests, fallback) the embedding happens inline and
    /// the cache is updated before this returns.
    pub fn reindex_file(
        &self,
        file_id: &str,
        title: &str,
        note: &tektite_parser::ParsedNote,
    ) -> Result<(), EmbedError> {
        self.reindex_file_with_priority(file_id, title, note, Priority::Normal)
    }

    /// True when the file already has at least one stored chunk. Used by
    /// the vault-open scan to recognise already-indexed files that were
    /// never embedded (pre-semantic-index installs) so they can be
    /// backfilled without the usual mtime short-circuit.
    pub fn has_chunks_for_file(&self, file_id: &str) -> Result<bool, EmbedError> {
        self.store.has_chunks_for_file(file_id)
    }

    /// Like [`reindex_file`](Self::reindex_file) but with an explicit
    /// priority. Use [`Priority::High`] for live edits so they jump ahead
    /// of vault-open backlog items.
    pub fn reindex_file_with_priority(
        &self,
        file_id: &str,
        title: &str,
        note: &tektite_parser::ParsedNote,
        priority: Priority,
    ) -> Result<(), EmbedError> {
        if let Some(queue) = &self.queue {
            queue.submit(EmbedJob {
                file_id: file_id.to_string(),
                title: title.to_string(),
                note: note.clone(),
                priority,
            });
            return Ok(());
        }

        // Sync path — inline embedding.
        self.reindex_file_sync(file_id, title, note)
    }

    /// Synchronous inline reindex. Used in sync mode and as the shared
    /// implementation detail.
    fn reindex_file_sync(
        &self,
        file_id: &str,
        title: &str,
        note: &tektite_parser::ParsedNote,
    ) -> Result<(), EmbedError> {
        let chunks = chunk_note(title, note);
        let old = self.store.chunks_for_file(file_id)?;

        let mut reused: Vec<Option<(String, Vector)>> = Vec::with_capacity(chunks.len());
        let mut texts_to_embed: Vec<&str> = Vec::new();

        for (idx, chunk) in chunks.iter().enumerate() {
            let reuse = old.iter().find(|r| {
                r.chunk_index as usize == idx && r.content_hash == chunk.content_hash
            });
            match reuse {
                Some(existing) => {
                    reused.push(Some((existing.id.clone(), existing.vector)));
                }
                None => {
                    reused.push(None);
                    texts_to_embed.push(chunk.embed_input.as_str());
                }
            }
        }

        let fresh = if texts_to_embed.is_empty() {
            Vec::new()
        } else {
            self.query_embedder.embed_documents(&texts_to_embed)?
        };
        if fresh.len() != texts_to_embed.len() {
            return Err(EmbedError::Embedder(format!(
                "embedder returned {} vectors for {} inputs",
                fresh.len(),
                texts_to_embed.len()
            )));
        }

        let mut finalised: Vec<(String, Vector)> = Vec::with_capacity(chunks.len());
        let mut fresh_iter = fresh.into_iter();
        for slot in reused.into_iter() {
            match slot {
                Some(pair) => finalised.push(pair),
                None => {
                    let raw = fresh_iter.next().expect("embed target count matches");
                    let vec = to_fixed_vector(raw)?;
                    let id = uuid::Uuid::new_v4().to_string();
                    finalised.push((id, vec));
                }
            }
        }

        self.store.replace_file_chunks(file_id, &chunks, &finalised)?;

        let entries: Vec<CacheEntry> = finalised
            .into_iter()
            .map(|(chunk_id, vector)| CacheEntry {
                chunk_id,
                file_id: file_id.to_string(),
                vector: Arc::new(vector),
            })
            .collect();
        self.cache.replace_for_file(file_id, entries);

        Ok(())
    }

    /// Drops every chunk belonging to `file_id` from the in-memory cache.
    ///
    /// The underlying `chunks` rows are removed by SQLite via the `ON DELETE
    /// CASCADE` foreign key when the file row is deleted. Callers that
    /// remove the file through `tektite-index` therefore only need to clear
    /// the cache here.
    pub fn forget_file(&self, file_id: &str) {
        self.cache.remove_for_file(file_id);
    }

    /// Runs semantic search for a user query and returns the top `limit`
    /// hits, highest similarity first.
    pub fn search_semantic(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SemanticHit>, EmbedError> {
        if query.trim().is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let raw = self.query_embedder.embed_query(query)?;
        let query_vec = to_fixed_vector(raw)?;
        let ranked = self.cache.top_k(&query_vec, limit);
        if ranked.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<&str> = ranked.iter().map(|(id, _)| id.as_str()).collect();
        let metas = self.store.chunks_by_ids(&ids)?;

        let mut hits = Vec::with_capacity(ranked.len());
        for (id, score) in ranked {
            let Some(meta) = metas.iter().find(|m| m.id == id) else {
                continue;
            };
            hits.push(SemanticHit {
                chunk_id: meta.id.clone(),
                file_path: meta.file_path.clone(),
                heading_path: meta.heading_path.clone(),
                heading_text: meta.heading_text.clone(),
                heading_level: meta.heading_level,
                snippet: snippet_from(&meta.content),
                score,
            });
        }
        Ok(hits)
    }

    /// Returns notes semantically related to the given file.
    ///
    /// Computes a centroid vector from the source file's chunk embeddings,
    /// runs cosine search, and deduplicates results by file — returning
    /// at most one hit per related note. The source file is excluded.
    pub fn search_related_notes(
        &self,
        file_id: &str,
        limit: usize,
    ) -> Result<Vec<SemanticHit>, EmbedError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let centroid = match self.cache.centroid_for_file(file_id) {
            Some(c) => c,
            None => return Ok(Vec::new()),
        };

        // Over-fetch from the cache so we have enough after dedup.
        let over_limit = limit * 5;
        let ranked = self.cache.top_k_excluding(&centroid, over_limit, &[file_id]);
        if ranked.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<&str> = ranked.iter().map(|(id, _)| id.as_str()).collect();
        let metas = self.store.chunks_by_ids(&ids)?;

        // Deduplicate by file: keep the highest-scoring chunk per file.
        let mut seen_files = std::collections::HashSet::new();
        let mut hits = Vec::with_capacity(limit);
        for (id, score) in ranked {
            let Some(meta) = metas.iter().find(|m| m.id == id) else {
                continue; // cache/store out of sync — skip gracefully
            };
            if !seen_files.insert(meta.file_id.clone()) {
                continue; // already have a hit for this file
            }
            hits.push(SemanticHit {
                chunk_id: meta.id.clone(),
                file_path: meta.file_path.clone(),
                heading_path: meta.heading_path.clone(),
                heading_text: meta.heading_text.clone(),
                heading_level: meta.heading_level,
                snippet: snippet_from(&meta.content),
                score,
            });
            if hits.len() >= limit {
                break;
            }
        }
        Ok(hits)
    }

    /// Returns chunks similar to a specific section of a note.
    ///
    /// Looks up the chunk by `file_id` + `heading_path`, then runs cosine
    /// search for similar chunks across the vault. By default, chunks from
    /// the same file are excluded (set `exclude_same_file` to `false` to
    /// include them).
    pub fn search_similar_chunks(
        &self,
        file_id: &str,
        heading_path: Option<&str>,
        limit: usize,
        exclude_same_file: bool,
    ) -> Result<Vec<SemanticHit>, EmbedError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        // Find the source chunk's id via the store.
        let chunk_id = match self.store.chunk_id_by_heading(file_id, heading_path)? {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };

        // Get its vector from the cache.
        let query_vec = match self.cache.vector_for_chunk(&chunk_id) {
            Some(v) => v,
            None => return Ok(Vec::new()),
        };

        let ranked = if exclude_same_file {
            self.cache.top_k_excluding(&query_vec, limit, &[file_id])
        } else {
            self.cache.top_k(&query_vec, limit)
        };
        if ranked.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<&str> = ranked.iter().map(|(id, _)| id.as_str()).collect();
        let metas = self.store.chunks_by_ids(&ids)?;

        let mut hits = Vec::with_capacity(ranked.len());
        for (id, score) in ranked {
            let Some(meta) = metas.iter().find(|m| m.id == id) else {
                continue; // cache/store out of sync — skip gracefully
            };
            hits.push(SemanticHit {
                chunk_id: meta.id.clone(),
                file_path: meta.file_path.clone(),
                heading_path: meta.heading_path.clone(),
                heading_text: meta.heading_text.clone(),
                heading_level: meta.heading_level,
                snippet: snippet_from(&meta.content),
                score,
            });
        }
        Ok(hits)
    }

    /// Triggers the background worker to load the ONNX model ahead of
    /// the first real job. No-op in sync mode.
    pub fn prewarm(&self) {
        if let Some(queue) = &self.queue {
            queue.prewarm();
        }
    }

    /// Returns the current embed progress. In sync mode returns
    /// `done == total == 0`.
    pub fn progress(&self) -> EmbedProgress {
        match &self.queue {
            Some(queue) => queue.progress(),
            None => EmbedProgress { done: 0, total: 0 },
        }
    }

    /// Returns a reference to the in-memory vector cache.
    ///
    /// Used by callers that need to run whole-corpus scans over current
    /// embeddings (e.g. the mutual-kNN graph computation) without going
    /// through the search surfaces.
    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    /// Test-only accessor.
    #[cfg(any(test, feature = "test-support"))]
    pub fn store(&self) -> &Store {
        &self.store
    }
}

pub(crate) fn to_fixed_vector(raw: Vec<f32>) -> Result<Vector, EmbedError> {
    if raw.len() != EMBED_DIM {
        return Err(EmbedError::Dim {
            expected: EMBED_DIM,
            actual: raw.len(),
        });
    }
    let mut out = [0f32; EMBED_DIM];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn snippet_from(content: &str) -> String {
    const MAX: usize = 240;
    let trimmed = content.trim();
    if trimmed.chars().count() <= MAX {
        return trimmed.to_string();
    }
    let mut out = String::with_capacity(MAX + 1);
    for ch in trimmed.chars().take(MAX) {
        out.push(ch);
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tektite_parser::ParsedNote;

    fn make_note(body: &str) -> ParsedNote {
        tektite_parser::parse(body)
    }

    // Long-enough content to avoid the min-64-token merge in the chunker.
    const LONG_A: &str = "This is a fairly long paragraph about authentication and OAuth 2.0 \
        protocols. It covers authorization code flow, implicit grant, client credentials, \
        and resource owner password. Each grant type serves a different use case in modern \
        web applications and API security architectures. Understanding the flow helps \
        developers build secure systems that protect user data effectively.";

    const LONG_B: &str = "JWT refresh tokens are used to obtain new access tokens without \
        requiring the user to re-authenticate. They are typically longer-lived than access \
        tokens and must be stored securely. Rotation policies help mitigate token theft, \
        and proper revocation endpoints ensure compromised tokens can be invalidated \
        quickly across all client applications.";

    const LONG_C: &str = "User login with OAuth allows single sign-on across multiple \
        applications. The identity provider handles credential verification while the \
        relying party receives an authorization code that can be exchanged for tokens. \
        This pattern is fundamental to modern authentication architecture and reduces \
        the burden of password management on individual applications.";

    const LONG_D: &str = "How to make carbonara requires guanciale, eggs, pecorino romano, \
        black pepper, and good quality pasta like rigatoni or spaghetti. The technique \
        involves rendering the fat from the guanciale slowly, tempering the egg and \
        cheese mixture to avoid scrambling, and tossing everything together with starchy \
        pasta water for a creamy emulsion.";

    /// Sets up a service with 3 files, each with distinct content, so
    /// the FakeEmbedder produces different vectors for each.
    fn setup_service() -> EmbedService {
        let svc = EmbedService::open_in_memory(Box::new(FakeEmbedder::new())).unwrap();
        svc.store().insert_test_file("f1", "notes/auth.md").unwrap();
        svc.store().insert_test_file("f2", "notes/login.md").unwrap();
        svc.store().insert_test_file("f3", "notes/cooking.md").unwrap();

        // File 1: two sections with enough content to avoid merge.
        let note1 = make_note(&format!("## OAuth\n{LONG_A}\n\n## Tokens\n{LONG_B}"));
        svc.reindex_file("f1", "Auth", &note1).unwrap();

        // File 2: one section similar topic to f1.
        let note2 = make_note(&format!("## Login\n{LONG_C}"));
        svc.reindex_file("f2", "Login", &note2).unwrap();

        // File 3: completely different topic.
        let note3 = make_note(&format!("## Pasta\n{LONG_D}"));
        svc.reindex_file("f3", "Cooking", &note3).unwrap();

        svc
    }

    #[test]
    fn related_notes_excludes_source_file() {
        let svc = setup_service();
        let hits = svc.search_related_notes("f1", 10).unwrap();
        // f1 itself should never appear
        for hit in &hits {
            assert_ne!(hit.file_path, "notes/auth.md");
        }
    }

    #[test]
    fn related_notes_deduplicates_by_file() {
        let svc = setup_service();
        let hits = svc.search_related_notes("f1", 10).unwrap();
        let mut seen = std::collections::HashSet::new();
        for hit in &hits {
            assert!(
                seen.insert(hit.file_path.clone()),
                "duplicate file_path in results: {}",
                hit.file_path
            );
        }
    }

    #[test]
    fn related_notes_returns_empty_for_unknown_file() {
        let svc = setup_service();
        let hits = svc.search_related_notes("nonexistent", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn related_notes_returns_empty_with_zero_limit() {
        let svc = setup_service();
        let hits = svc.search_related_notes("f1", 0).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn similar_chunks_excludes_same_file_by_default() {
        let svc = setup_service();
        let hits = svc
            .search_similar_chunks("f1", Some("OAuth"), 10, true)
            .unwrap();
        for hit in &hits {
            assert_ne!(hit.file_path, "notes/auth.md");
        }
    }

    #[test]
    fn similar_chunks_returns_results() {
        let svc = setup_service();
        let hits = svc
            .search_similar_chunks("f1", Some("OAuth"), 10, true)
            .unwrap();
        // Should find chunks from f2 and f3
        assert!(!hits.is_empty());
    }

    #[test]
    fn similar_chunks_returns_empty_for_unknown_heading() {
        let svc = setup_service();
        let hits = svc
            .search_similar_chunks("f1", Some("Nonexistent"), 10, true)
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn similar_chunks_returns_empty_for_unknown_file() {
        let svc = setup_service();
        let hits = svc
            .search_similar_chunks("nonexistent", Some("OAuth"), 10, true)
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_semantic_returns_empty_for_empty_query() {
        let svc = setup_service();
        let hits = svc.search_semantic("", 10).unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn search_semantic_returns_results() {
        let svc = setup_service();
        let hits = svc.search_semantic("authentication", 10).unwrap();
        assert!(!hits.is_empty());
    }

    #[test]
    fn disabled_state_returns_empty() {
        // Service with no files = effectively disabled
        let svc = EmbedService::open_in_memory(Box::new(FakeEmbedder::new())).unwrap();
        assert!(svc.search_semantic("anything", 10).unwrap().is_empty());
        assert!(svc.search_related_notes("any", 10).unwrap().is_empty());
        assert!(svc.search_similar_chunks("any", None, 10, true).unwrap().is_empty());
    }

    #[test]
    fn prewarm_completes_without_error_in_sync_mode() {
        let svc = setup_service();
        svc.prewarm(); // no-op in sync mode, should not panic
    }

    #[test]
    fn progress_returns_zeros_in_sync_mode() {
        let svc = setup_service();
        let p = svc.progress();
        assert_eq!(p.done, 0);
        assert_eq!(p.total, 0);
    }

    #[test]
    fn cache_skip_missing_store_entry() {
        // Verify that search gracefully handles a cache entry whose
        // chunk_id has been deleted from the store.
        let svc = EmbedService::open_in_memory(Box::new(FakeEmbedder::new())).unwrap();
        svc.store().insert_test_file("f1", "a.md").unwrap();

        let note = make_note("some content here for testing");
        svc.reindex_file("f1", "Test", &note).unwrap();

        // Verify search works normally.
        assert!(!svc.search_semantic("testing", 5).unwrap().is_empty());

        // Now delete the file row (cascades to chunks) but leave the cache
        // with stale entries.
        svc.store()
            .conn_for_test()
            .execute("DELETE FROM files WHERE id = 'f1'", [])
            .unwrap();

        // Search should return empty (store entry gone) but not error.
        let hits = svc.search_semantic("testing", 5).unwrap();
        assert!(hits.is_empty());
    }
}
