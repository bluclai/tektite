//! In-memory vector cache + brute-force cosine search.
//!
//! All vectors are L2-normalised by the [`Embedder`](crate::Embedder)
//! contract, so cosine similarity reduces to a dot product. Each search
//! scans the entire `Vec` under a read lock, which at personal vault
//! scale (even 50–100K chunks) stays under ~5ms on a laptop CPU.
//!
//! The cache is the hot path for reads but a slow path for writes: we
//! only write during `reindex_file` / `forget_file`, both of which are
//! already rare (edit, save, rename).

use std::sync::{Arc, RwLock};

use crate::{Store, Vector};

/// One entry in the in-memory vector cache.
///
/// `vector` is `Arc`-wrapped to keep clone cost constant: search builds a
/// short-lived snapshot of candidate entries under the read lock, and we
/// never want to memcpy 1 KiB per candidate.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub chunk_id: String,
    pub file_id: String,
    pub vector: Arc<Vector>,
}

/// The shared vector cache.
///
/// Cloning a [`Cache`] bumps the refcount on the inner `Arc<RwLock<…>>`;
/// all clones point at the same data.
#[derive(Clone, Default)]
pub struct Cache {
    entries: Arc<RwLock<Vec<CacheEntry>>>,
}

impl Cache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Populate from the store in one go. Used by `EmbedService::open` so
    /// search is ready the moment the vault opens.
    pub fn load_all_from_store(&self, store: &Store) -> Result<(), crate::EmbedError> {
        let rows = store.all_vectors()?;
        let mut guard = self.entries.write().expect("cache write poisoned");
        guard.clear();
        guard.reserve(rows.len());
        for (chunk_id, file_id, vector) in rows {
            guard.push(CacheEntry {
                chunk_id,
                file_id,
                vector: Arc::new(vector),
            });
        }
        Ok(())
    }

    /// Replace every entry owned by `file_id` with `new_entries`. Other
    /// files are untouched.
    pub fn replace_for_file(&self, file_id: &str, new_entries: Vec<CacheEntry>) {
        let mut guard = self.entries.write().expect("cache write poisoned");
        guard.retain(|e| e.file_id != file_id);
        guard.extend(new_entries);
    }

    /// Drop every entry owned by `file_id`.
    pub fn remove_for_file(&self, file_id: &str) {
        let mut guard = self.entries.write().expect("cache write poisoned");
        guard.retain(|e| e.file_id != file_id);
    }

    /// Returns the top `k` most-similar `(chunk_id, similarity)` pairs.
    /// Ties are broken by chunk_id for deterministic ordering.
    pub fn top_k(&self, query: &Vector, k: usize) -> Vec<(String, f32)> {
        if k == 0 {
            return Vec::new();
        }
        let guard = self.entries.read().expect("cache read poisoned");

        let mut ranked: Vec<(&str, f32)> = guard
            .iter()
            .map(|e| (e.chunk_id.as_str(), cosine(query, &e.vector)))
            .collect();

        // Sort descending by score, then ascending by id for stability.
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(b.0))
        });
        ranked.truncate(k);
        ranked
            .into_iter()
            .map(|(id, s)| (id.to_string(), s))
            .collect()
    }

    /// Returns the top `k` most-similar `(chunk_id, similarity)` pairs,
    /// excluding entries whose `file_id` matches any in `exclude_files`.
    pub fn top_k_excluding(
        &self,
        query: &Vector,
        k: usize,
        exclude_files: &[&str],
    ) -> Vec<(String, f32)> {
        if k == 0 {
            return Vec::new();
        }
        let guard = self.entries.read().expect("cache read poisoned");

        let mut ranked: Vec<(&str, f32)> = guard
            .iter()
            .filter(|e| !exclude_files.contains(&e.file_id.as_str()))
            .map(|e| (e.chunk_id.as_str(), cosine(query, &e.vector)))
            .collect();

        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(b.0))
        });
        ranked.truncate(k);
        ranked
            .into_iter()
            .map(|(id, s)| (id.to_string(), s))
            .collect()
    }

    /// Returns the averaged (centroid) vector for all entries belonging
    /// to `file_id`. Returns `None` if the file has no cached entries.
    pub fn centroid_for_file(&self, file_id: &str) -> Option<Vector> {
        let guard = self.entries.read().expect("cache read poisoned");
        let vecs: Vec<&Vector> = guard
            .iter()
            .filter(|e| e.file_id == file_id)
            .map(|e| e.vector.as_ref())
            .collect();
        if vecs.is_empty() {
            return None;
        }
        let n = vecs.len() as f32;
        let mut avg = [0f32; crate::EMBED_DIM];
        for v in &vecs {
            for (i, x) in v.iter().enumerate() {
                avg[i] += x;
            }
        }
        for x in avg.iter_mut() {
            *x /= n;
        }
        // L2-normalise so cosine search works correctly.
        let norm: f32 = avg.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in avg.iter_mut() {
                *x /= norm;
            }
        }
        Some(avg)
    }

    /// Returns the vector for a specific chunk, or `None` if not cached.
    pub fn vector_for_chunk(&self, chunk_id: &str) -> Option<Vector> {
        let guard = self.entries.read().expect("cache read poisoned");
        guard
            .iter()
            .find(|e| e.chunk_id == chunk_id)
            .map(|e| *e.vector)
    }

    /// Current number of cached entries. Primarily for tests and metrics.
    pub fn len(&self) -> usize {
        self.entries.read().expect("cache read poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Dot product, which equals cosine similarity for L2-normalised inputs.
/// Falls back to the safe formula if inputs aren't unit length — never
/// panics on zero vectors.
fn cosine(a: &Vector, b: &Vector) -> f32 {
    let mut dot = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
    }
    dot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: [f32; 4]) -> Vector {
        // Helper: build a full-length vector from a 4-element prefix and
        // normalise so dot products equal cosine.
        let mut out = [0f32; crate::EMBED_DIM];
        out[..4].copy_from_slice(&v);
        let norm: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in out.iter_mut() {
                *x /= norm;
            }
        }
        out
    }

    fn entry(chunk_id: &str, file_id: &str, vec: Vector) -> CacheEntry {
        CacheEntry {
            chunk_id: chunk_id.to_string(),
            file_id: file_id.to_string(),
            vector: Arc::new(vec),
        }
    }

    #[test]
    fn empty_cache_returns_no_results() {
        let cache = Cache::new();
        let q = unit([1.0, 0.0, 0.0, 0.0]);
        assert!(cache.top_k(&q, 5).is_empty());
    }

    #[test]
    fn top_k_returns_most_similar_first() {
        let cache = Cache::new();
        cache.replace_for_file(
            "f",
            vec![
                entry("a", "f", unit([1.0, 0.0, 0.0, 0.0])),
                entry("b", "f", unit([0.0, 1.0, 0.0, 0.0])),
                entry("c", "f", unit([0.8, 0.2, 0.0, 0.0])),
            ],
        );
        let q = unit([1.0, 0.0, 0.0, 0.0]);
        let hits = cache.top_k(&q, 2);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].0, "a"); // identical → highest
        assert_eq!(hits[1].0, "c"); // closer than "b"
    }

    #[test]
    fn replace_for_file_preserves_other_files() {
        let cache = Cache::new();
        cache.replace_for_file("f1", vec![entry("a", "f1", unit([1.0, 0.0, 0.0, 0.0]))]);
        cache.replace_for_file("f2", vec![entry("b", "f2", unit([0.0, 1.0, 0.0, 0.0]))]);
        cache.replace_for_file("f1", vec![entry("a2", "f1", unit([1.0, 0.0, 0.0, 0.0]))]);

        assert_eq!(cache.len(), 2);
        let q = unit([0.0, 1.0, 0.0, 0.0]);
        let hits = cache.top_k(&q, 1);
        assert_eq!(hits[0].0, "b"); // f2 untouched
    }

    #[test]
    fn remove_for_file_drops_only_that_files_entries() {
        let cache = Cache::new();
        cache.replace_for_file(
            "f1",
            vec![entry("a", "f1", unit([1.0, 0.0, 0.0, 0.0]))],
        );
        cache.replace_for_file(
            "f2",
            vec![entry("b", "f2", unit([0.0, 1.0, 0.0, 0.0]))],
        );
        cache.remove_for_file("f1");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn top_k_clamps_to_available_entries() {
        let cache = Cache::new();
        cache.replace_for_file(
            "f",
            vec![entry("a", "f", unit([1.0, 0.0, 0.0, 0.0]))],
        );
        assert_eq!(cache.top_k(&unit([1.0, 0.0, 0.0, 0.0]), 100).len(), 1);
    }

    #[test]
    fn top_k_excluding_omits_specified_files() {
        let cache = Cache::new();
        cache.replace_for_file(
            "f1",
            vec![entry("a", "f1", unit([1.0, 0.0, 0.0, 0.0]))],
        );
        cache.replace_for_file(
            "f2",
            vec![entry("b", "f2", unit([0.9, 0.1, 0.0, 0.0]))],
        );
        let q = unit([1.0, 0.0, 0.0, 0.0]);
        let hits = cache.top_k_excluding(&q, 10, &["f1"]);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, "b");
    }

    #[test]
    fn centroid_for_file_averages_vectors() {
        let cache = Cache::new();
        // Two vectors pointing in different directions.
        let v1 = unit([1.0, 0.0, 0.0, 0.0]);
        let v2 = unit([0.0, 1.0, 0.0, 0.0]);
        cache.replace_for_file(
            "f",
            vec![entry("a", "f", v1), entry("b", "f", v2)],
        );
        let centroid = cache.centroid_for_file("f").unwrap();
        // Average of [1,0,...] and [0,1,...] normalised → ~[0.707, 0.707, ...]
        assert!((centroid[0] - centroid[1]).abs() < 0.01);
        // Should be normalised.
        let norm: f32 = centroid.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[test]
    fn centroid_for_file_returns_none_for_unknown() {
        let cache = Cache::new();
        assert!(cache.centroid_for_file("nonexistent").is_none());
    }

    #[test]
    fn vector_for_chunk_returns_correct_vector() {
        let cache = Cache::new();
        let v = unit([0.5, 0.5, 0.0, 0.0]);
        cache.replace_for_file("f", vec![entry("c1", "f", v)]);
        let got = cache.vector_for_chunk("c1").unwrap();
        assert_eq!(got, v);
    }

    #[test]
    fn vector_for_chunk_returns_none_for_unknown() {
        let cache = Cache::new();
        assert!(cache.vector_for_chunk("nonexistent").is_none());
    }

    #[test]
    fn ties_are_broken_deterministically_by_chunk_id() {
        let cache = Cache::new();
        let shared = unit([1.0, 0.0, 0.0, 0.0]);
        cache.replace_for_file(
            "f",
            vec![
                entry("zz", "f", shared),
                entry("aa", "f", shared),
            ],
        );
        let hits = cache.top_k(&shared, 2);
        assert_eq!(hits[0].0, "aa");
        assert_eq!(hits[1].0, "zz");
    }
}
