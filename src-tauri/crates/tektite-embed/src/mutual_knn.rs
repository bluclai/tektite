//! Mutual top-K nearest-neighbour computation over file-level centroids.
//!
//! For each indexed file we compute one centroid vector from its chunks
//! (handled by [`Cache::all_file_centroids`]), then score every pair with
//! cosine similarity. A pair `(a, b)` becomes a semantic edge iff `b` is in
//! `a`'s top-K *and* `a` is in `b`'s top-K. The mutual requirement is
//! deliberate — asymmetric top-K produces hubs that dominate the force
//! layout and bury the signal.
//!
//! The implementation is brute-force O(n²) and runs on a background thread
//! through a caller-provided progress callback + cancellation probe. At v1
//! personal-vault scales (≤ 5000 notes, 256-dim centroids) this completes
//! in well under a second on commodity hardware.

use std::cmp::Ordering as CmpOrdering;
use std::collections::HashSet;

use crate::{Cache, Vector};

/// Options controlling the mutual-kNN computation.
#[derive(Debug, Clone)]
pub struct MutualKnnOptions {
    /// Target neighbours per file.
    pub k: usize,
    /// Inclusive lower bound on cosine similarity. Pairs below this are
    /// discarded before the mutual check runs.
    pub min_similarity: f32,
}

/// A mutual-kNN edge between two files with its cosine similarity.
///
/// `source` and `target` are sorted (`source < target`) so that the caller
/// never has to dedupe pairs returned as both `(a, b)` and `(b, a)`.
#[derive(Debug, Clone)]
pub struct MutualKnnEdge {
    pub source: String,
    pub target: String,
    pub score: f32,
}

/// Progress callback surface passed into [`compute_mutual_knn`].
///
/// `report` fires roughly every 50 processed files (plus once on completion)
/// so the frontend can drive a progress indicator. `is_cancelled` is polled
/// at the same cadence so superseded requests bail out quickly.
pub trait KnnProgress {
    fn report(&mut self, done: u32, total: u32);
    fn is_cancelled(&self) -> bool;
}

/// Default implementation: no progress, never cancelled. Used by tests.
pub struct NoopProgress;
impl KnnProgress for NoopProgress {
    fn report(&mut self, _done: u32, _total: u32) {}
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Returns the mutual top-K semantic edges over the given file set.
///
/// `allowed_file_ids` restricts the input corpus — files outside this set
/// are ignored (their centroids aren't even computed). Pass `None` to use
/// every file currently in the cache.
///
/// Returns an empty vector when the request is cancelled mid-compute; the
/// caller is responsible for emitting any `graph:knn-cancelled` event.
pub fn compute_mutual_knn<P: KnnProgress>(
    cache: &Cache,
    allowed_file_ids: Option<&HashSet<String>>,
    opts: &MutualKnnOptions,
    progress: &mut P,
) -> Vec<MutualKnnEdge> {
    if opts.k == 0 {
        return Vec::new();
    }

    let mut centroids: Vec<(String, Vector)> = cache
        .all_file_centroids()
        .into_iter()
        .filter(|(id, _)| match allowed_file_ids {
            Some(set) => set.contains(id),
            None => true,
        })
        .collect();
    // Deterministic ordering so equal-score ties resolve consistently.
    centroids.sort_by(|a, b| a.0.cmp(&b.0));

    let n = centroids.len();
    let total = n as u32;
    progress.report(0, total);
    if n < 2 {
        progress.report(total, total);
        return Vec::new();
    }

    // For each file, a small top-K heap keyed by score ascending (so we can
    // pop the smallest when the heap is full).
    let mut top_k: Vec<Vec<(f32, usize)>> = vec![Vec::with_capacity(opts.k + 1); n];

    for i in 0..n {
        if progress.is_cancelled() {
            return Vec::new();
        }
        for j in (i + 1)..n {
            let score = dot(&centroids[i].1, &centroids[j].1);
            if score < opts.min_similarity {
                continue;
            }
            push_top_k(&mut top_k[i], opts.k, (score, j));
            push_top_k(&mut top_k[j], opts.k, (score, i));
        }
        if i % 50 == 49 {
            progress.report((i + 1) as u32, total);
        }
    }

    let mut neighbours: Vec<HashSet<usize>> = top_k
        .iter()
        .map(|v| v.iter().map(|&(_, idx)| idx).collect())
        .collect();
    // Build edge list from the top-K sets; deduplicate by enforcing `a < b`.
    let mut edges: Vec<MutualKnnEdge> = Vec::new();
    for i in 0..n {
        for &(score, j) in &top_k[i] {
            if i >= j {
                continue;
            }
            if !neighbours[j].contains(&i) {
                continue;
            }
            edges.push(MutualKnnEdge {
                source: centroids[i].0.clone(),
                target: centroids[j].0.clone(),
                score,
            });
        }
    }
    // Silence unused-mut clippy — neighbours is only read above, but the
    // compiler-visible type needs to stay `Vec<HashSet>` for lookups.
    neighbours.clear();

    edges.sort_by(|a, b| {
        (a.source.as_str(), a.target.as_str()).cmp(&(b.source.as_str(), b.target.as_str()))
    });

    progress.report(total, total);
    edges
}

fn push_top_k(slot: &mut Vec<(f32, usize)>, k: usize, entry: (f32, usize)) {
    slot.push(entry);
    // Sort descending by score; tie-break by index for determinism.
    slot.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(CmpOrdering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });
    if slot.len() > k {
        slot.truncate(k);
    }
}

/// Dot product over L2-normalised inputs = cosine similarity.
fn dot(a: &Vector, b: &Vector) -> f32 {
    let mut acc = 0.0f32;
    for i in 0..a.len() {
        acc += a[i] * b[i];
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CacheEntry, EMBED_DIM};
    use std::sync::Arc;

    fn unit(prefix: [f32; 4]) -> Vector {
        let mut v = [0f32; EMBED_DIM];
        v[..4].copy_from_slice(&prefix);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in v.iter_mut() {
                *x /= norm;
            }
        }
        v
    }

    fn entry(chunk_id: &str, file_id: &str, vec: Vector) -> CacheEntry {
        CacheEntry {
            chunk_id: chunk_id.to_string(),
            file_id: file_id.to_string(),
            vector: Arc::new(vec),
        }
    }

    #[test]
    fn empty_input_returns_empty() {
        let cache = Cache::new();
        let edges = compute_mutual_knn(
            &cache,
            None,
            &MutualKnnOptions {
                k: 4,
                min_similarity: 0.0,
            },
            &mut NoopProgress,
        );
        assert!(edges.is_empty());
    }

    #[test]
    fn mutual_pair_becomes_edge() {
        let cache = Cache::new();
        let v = unit([1.0, 0.0, 0.0, 0.0]);
        cache.replace_for_file("a", vec![entry("ca", "a", v)]);
        cache.replace_for_file("b", vec![entry("cb", "b", v)]);
        let edges = compute_mutual_knn(
            &cache,
            None,
            &MutualKnnOptions {
                k: 4,
                min_similarity: 0.0,
            },
            &mut NoopProgress,
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, "a");
        assert_eq!(edges[0].target, "b");
    }

    #[test]
    fn min_similarity_filters_weak_pairs() {
        let cache = Cache::new();
        cache.replace_for_file("a", vec![entry("ca", "a", unit([1.0, 0.0, 0.0, 0.0]))]);
        cache.replace_for_file("b", vec![entry("cb", "b", unit([0.0, 1.0, 0.0, 0.0]))]);
        let edges = compute_mutual_knn(
            &cache,
            None,
            &MutualKnnOptions {
                k: 4,
                min_similarity: 0.5,
            },
            &mut NoopProgress,
        );
        assert!(edges.is_empty());
    }

    #[test]
    fn asymmetric_top_k_is_rejected() {
        // a=[1,0], b=[0.95,0.31], c=[0.9,0.4] → sim(b,c) ≈ 0.98 beats both
        // sim(a,b)=0.95 and sim(a,c)=0.9. With k=1, a's top is b but b's top
        // is c — that pair is *not* mutual, so no (a,b) edge. The mutual
        // winner is (b,c). Proves asymmetric top-K doesn't leak through.
        let cache = Cache::new();
        cache.replace_for_file("a", vec![entry("ca", "a", unit([1.0, 0.0, 0.0, 0.0]))]);
        cache.replace_for_file("b", vec![entry("cb", "b", unit([0.95, 0.31, 0.0, 0.0]))]);
        cache.replace_for_file("c", vec![entry("cc", "c", unit([0.9, 0.4, 0.0, 0.0]))]);
        let edges = compute_mutual_knn(
            &cache,
            None,
            &MutualKnnOptions {
                k: 1,
                min_similarity: 0.0,
            },
            &mut NoopProgress,
        );
        assert_eq!(edges.len(), 1);
        assert_eq!((edges[0].source.as_str(), edges[0].target.as_str()), ("b", "c"));
    }

    #[test]
    fn allowed_filter_is_respected() {
        let cache = Cache::new();
        let v = unit([1.0, 0.0, 0.0, 0.0]);
        cache.replace_for_file("a", vec![entry("ca", "a", v)]);
        cache.replace_for_file("b", vec![entry("cb", "b", v)]);
        cache.replace_for_file("c", vec![entry("cc", "c", v)]);
        let allowed: HashSet<String> = ["a".into(), "b".into()].into_iter().collect();
        let edges = compute_mutual_knn(
            &cache,
            Some(&allowed),
            &MutualKnnOptions {
                k: 4,
                min_similarity: 0.0,
            },
            &mut NoopProgress,
        );
        assert_eq!(edges.len(), 1);
        assert_eq!((edges[0].source.as_str(), edges[0].target.as_str()), ("a", "b"));
    }

    #[test]
    fn cancellation_returns_empty() {
        struct AlwaysCancel;
        impl KnnProgress for AlwaysCancel {
            fn report(&mut self, _: u32, _: u32) {}
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        let cache = Cache::new();
        let v = unit([1.0, 0.0, 0.0, 0.0]);
        cache.replace_for_file("a", vec![entry("ca", "a", v)]);
        cache.replace_for_file("b", vec![entry("cb", "b", v)]);
        let edges = compute_mutual_knn(
            &cache,
            None,
            &MutualKnnOptions {
                k: 4,
                min_similarity: 0.0,
            },
            &mut AlwaysCancel,
        );
        assert!(edges.is_empty());
    }
}
