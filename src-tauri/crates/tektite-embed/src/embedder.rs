//! The [`Embedder`] trait + the [`FakeEmbedder`] used by unit tests and
//! the Phase 1 tracer-bullet wiring.
//!
//! Phase 2 introduces `OnnxEmbedder`, which loads `nomic-embed-text-v1.5`
//! from a bundled Tauri resource and runs CPU inference through the `ort`
//! crate.

use sha2::{Digest, Sha256};

use crate::{EmbedError, EMBED_DIM};

/// Anything that can turn input strings into [L2-normalised] vectors.
///
/// Document and query embeddings are distinct calls so model-specific
/// prompt prefixes (e.g. nomic's `search_document:` / `search_query:`)
/// can be applied inside the embedder rather than leaked into the
/// chunker or the search command.
///
/// Implementations must be thread-safe (`Send + Sync`) because the
/// Phase 3 queue owns the embedder on a dedicated worker thread.
pub trait Embedder: Send + Sync {
    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError>;
}

/// Deterministic fake embedder used in tests and as the Phase 1 stand-in
/// for `OnnxEmbedder`.
///
/// Produces a stable, L2-normalised vector per input string by hashing the
/// text into a seed and filling the 256 dimensions from a cheap counter
/// PRF. Identical text → identical vector; different text → near-orthogonal
/// vectors (in expectation). Not semantically meaningful.
#[derive(Debug, Default, Clone)]
pub struct FakeEmbedder;

impl FakeEmbedder {
    pub fn new() -> Self {
        Self
    }
}

impl Embedder for FakeEmbedder {
    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts.iter().map(|t| fake_vector(t)).collect())
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        Ok(fake_vector(text))
    }
}

fn fake_vector(text: &str) -> Vec<f32> {
    // Seed from SHA-256 so identical text → identical vector across processes.
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let seed_bytes = hasher.finalize();

    let mut out = vec![0f32; EMBED_DIM];
    let seed_u64 = u64::from_le_bytes(seed_bytes[..8].try_into().expect("32-byte digest"));

    // xorshift64 produces 64 bits of entropy per iteration; we use the top
    // two bytes to drive each f32 so adjacent dims are not trivially
    // correlated.
    let mut state = seed_u64.max(1);
    for dim in out.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let high = (state >> 48) as u16;
        // Map u16 → (-1.0, 1.0)
        *dim = (high as f32 / 32_768.0) - 1.0;
    }

    // L2-normalise so cosine similarity = dot product, matching the
    // invariant the Phase 2 ONNX embedder will uphold.
    let norm: f32 = out.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for v in out.iter_mut() {
            *v /= norm;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_input_produces_same_vector() {
        let e = FakeEmbedder::new();
        let a = e.embed_documents(&["hello world"]).unwrap();
        let b = e.embed_documents(&["hello world"]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn different_inputs_produce_different_vectors() {
        let e = FakeEmbedder::new();
        let vecs = e.embed_documents(&["hello", "goodbye"]).unwrap();
        assert_ne!(vecs[0], vecs[1]);
    }

    #[test]
    fn output_dimension_is_256() {
        let e = FakeEmbedder::new();
        let v = e.embed_documents(&["any text"]).unwrap();
        assert_eq!(v[0].len(), EMBED_DIM);
    }

    #[test]
    fn output_is_unit_length() {
        let e = FakeEmbedder::new();
        let v = e.embed_documents(&["test"]).unwrap();
        let norm: f32 = v[0].iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "expected unit length, got {norm}");
    }

    #[test]
    fn batch_length_matches_input() {
        let e = FakeEmbedder::new();
        let v = e.embed_documents(&["a", "b", "c", "d"]).unwrap();
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn empty_batch_returns_empty() {
        let e = FakeEmbedder::new();
        let v = e.embed_documents(&[]).unwrap();
        assert!(v.is_empty());
    }

    #[test]
    fn query_and_document_for_same_text_match_for_fake_embedder() {
        // FakeEmbedder makes no semantic distinction between roles;
        // OnnxEmbedder will. This guards the FakeEmbedder contract only.
        let e = FakeEmbedder::new();
        let docs = e.embed_documents(&["topic"]).unwrap();
        let q = e.embed_query("topic").unwrap();
        assert_eq!(docs[0], q);
    }
}
