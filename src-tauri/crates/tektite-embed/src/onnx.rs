//! Real ONNX-backed embedder using `nomic-embed-text-v1.5`.
//!
//! Loads a quantised ONNX model + a HuggingFace tokenizer from disk,
//! tokenises a batch with BERT-style padding, runs CPU inference through
//! `ort`, mean-pools the last hidden state under the attention mask, L2
//! normalises, and truncates to the first [`EMBED_DIM`] (256) dimensions
//! per nomic's Matryoshka recipe.
//!
//! Document vs query inputs receive the model's required prompt prefixes
//! (`search_document:` / `search_query:`) inside this module so callers
//! never see them.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ndarray::Array2;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Value;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer, TruncationParams, TruncationStrategy};

use crate::embedder::Embedder;
use crate::{EmbedError, EMBED_DIM};

/// nomic v1.5 produces 768-dim outputs that we Matryoshka-truncate.
const MODEL_DIM: usize = 768;
/// Hard cap on tokens per input — matches the model's positional
/// embedding length.
const MAX_SEQ_LEN: usize = 8192;
/// The chunker's MAX_TOKENS is 512, so this is a defensive ceiling for
/// over-long queries; standard chunk inputs never exceed it.
const TOKENISE_LIMIT: usize = 2048;

const DOC_PREFIX: &str = "search_document: ";
const QUERY_PREFIX: &str = "search_query: ";

pub struct OnnxEmbedder {
    /// `ort::Session` is `Send` but uses interior mutability across runs;
    /// we serialise calls through a `Mutex` so the `Embedder` impl can
    /// stay `&self`. CPU inference is dominated by matmul, not lock
    /// contention.
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl OnnxEmbedder {
    /// Loads the model + tokenizer from disk. Errors if either file is
    /// missing or cannot be parsed.
    pub fn new(model_path: &Path, tokenizer_path: &Path) -> Result<Self, EmbedError> {
        let session = Session::builder()
            .map_err(onnx_err)?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(onnx_err)?
            .commit_from_file(model_path)
            .map_err(onnx_err)?;

        let mut tokenizer = Tokenizer::from_file(tokenizer_path)
            .map_err(|e| EmbedError::Embedder(format!("tokenizer load: {e}")))?;

        tokenizer.with_padding(Some(PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        }));
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: TOKENISE_LIMIT.min(MAX_SEQ_LEN),
                strategy: TruncationStrategy::LongestFirst,
                ..Default::default()
            }))
            .map_err(|e| EmbedError::Embedder(format!("tokenizer truncation: {e}")))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
        })
    }

    /// Convenience constructor: looks for `model.onnx` and `tokenizer.json`
    /// under `dir`. Tauri passes the resolved resource directory.
    pub fn from_resource_dir(dir: &Path) -> Result<Self, EmbedError> {
        let model: PathBuf = dir.join("model.onnx");
        let tok: PathBuf = dir.join("tokenizer.json");
        Self::new(&model, &tok)
    }

    fn run_batch(&self, inputs: Vec<String>) -> Result<Vec<Vec<f32>>, EmbedError> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }

        let encodings = self
            .tokenizer
            .encode_batch(inputs, true)
            .map_err(|e| EmbedError::Embedder(format!("tokenize: {e}")))?;

        let batch = encodings.len();
        let seq_len = encodings.iter().map(|e| e.get_ids().len()).max().unwrap_or(0);
        if seq_len == 0 {
            return Ok(vec![vec![0f32; EMBED_DIM]; batch]);
        }

        let mut ids = Array2::<i64>::zeros((batch, seq_len));
        let mut mask = Array2::<i64>::zeros((batch, seq_len));
        let mut type_ids = Array2::<i64>::zeros((batch, seq_len));
        for (b, enc) in encodings.iter().enumerate() {
            for (t, &v) in enc.get_ids().iter().enumerate() {
                ids[[b, t]] = v as i64;
            }
            for (t, &v) in enc.get_attention_mask().iter().enumerate() {
                mask[[b, t]] = v as i64;
            }
            for (t, &v) in enc.get_type_ids().iter().enumerate() {
                type_ids[[b, t]] = v as i64;
            }
        }

        let id_value = Value::from_array(ids).map_err(onnx_err)?;
        let mask_value = Value::from_array(mask.clone()).map_err(onnx_err)?;
        let type_value = Value::from_array(type_ids).map_err(onnx_err)?;

        let mut session = self.session.lock().expect("embed session mutex poisoned");
        let outputs = session
            .run(ort::inputs![
                "input_ids" => id_value,
                "attention_mask" => mask_value,
                "token_type_ids" => type_value,
            ])
            .map_err(onnx_err)?;

        // nomic exposes the pooled output under several names depending on
        // the export; pick the first tensor available.
        let (_, hidden) = outputs
            .iter()
            .next()
            .ok_or_else(|| EmbedError::Embedder("model produced no outputs".into()))?;
        let (shape_ref, raw) = hidden.try_extract_tensor::<f32>().map_err(onnx_err)?;
        let dims: Vec<usize> = shape_ref
            .as_ref()
            .iter()
            .map(|&d| d as usize)
            .collect();

        if dims.len() != 3 || dims[0] != batch || dims[1] != seq_len || dims[2] != MODEL_DIM {
            return Err(EmbedError::Embedder(format!(
                "unexpected hidden shape {:?}, expected [{batch}, {seq_len}, {MODEL_DIM}]",
                dims
            )));
        }

        let mut out = Vec::with_capacity(batch);
        for b in 0..batch {
            let mut pooled = vec![0f32; MODEL_DIM];
            let mut count: f32 = 0.0;
            for t in 0..seq_len {
                let m = mask[[b, t]];
                if m == 0 {
                    continue;
                }
                count += 1.0;
                let base = (b * seq_len + t) * MODEL_DIM;
                for d in 0..MODEL_DIM {
                    pooled[d] += raw[base + d];
                }
            }
            if count > 0.0 {
                for v in pooled.iter_mut() {
                    *v /= count;
                }
            }
            pooled.truncate(EMBED_DIM);
            l2_normalise(&mut pooled);
            out.push(pooled);
        }
        Ok(out)
    }
}

impl Embedder for OnnxEmbedder {
    fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let prefixed: Vec<String> = texts.iter().map(|t| format!("{DOC_PREFIX}{t}")).collect();
        self.run_batch(prefixed)
    }

    fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut out = self.run_batch(vec![format!("{QUERY_PREFIX}{text}")])?;
        out.pop()
            .ok_or_else(|| EmbedError::Embedder("query embedding empty".into()))
    }
}

fn onnx_err<E: std::fmt::Display>(e: E) -> EmbedError {
    EmbedError::Embedder(format!("onnx: {e}"))
}

fn l2_normalise(v: &mut [f32]) {
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

#[cfg(all(test, feature = "integration"))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn resource_dir() -> PathBuf {
        // Layout: src-tauri/resources/embed/{model.onnx,tokenizer.json}
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("resources")
            .join("embed")
    }

    #[test]
    fn loads_model_and_produces_correct_dim() {
        let e = OnnxEmbedder::from_resource_dir(&resource_dir()).expect("load model");
        let v = e.embed_query("hello").unwrap();
        assert_eq!(v.len(), EMBED_DIM);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-3, "expected unit length, got {norm}");
    }

    #[test]
    fn handles_unicode_and_emoji() {
        let e = OnnxEmbedder::from_resource_dir(&resource_dir()).expect("load model");
        let v = e
            .embed_documents(&["café résumé naïve 🚀 中文 한국어"])
            .unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].len(), EMBED_DIM);
    }

    #[test]
    fn semantically_related_texts_score_higher_than_unrelated() {
        let e = OnnxEmbedder::from_resource_dir(&resource_dir()).expect("load model");
        let docs = e
            .embed_documents(&[
                "User authentication and login flows",
                "How to bake sourdough bread at home",
            ])
            .unwrap();
        let q = e.embed_query("OAuth sign-in").unwrap();
        let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
        let auth = dot(&q, &docs[0]);
        let bread = dot(&q, &docs[1]);
        assert!(auth > bread, "expected auth ({auth}) > bread ({bread})");
    }

    /// End-to-end: embed → persist as BLOB → read back → compare. The
    /// store's unit tests already cover BLOB round-trips with synthetic
    /// vectors; this guards the specific production path where the vector
    /// comes straight out of `OnnxEmbedder`.
    #[test]
    fn embedding_round_trips_through_store_losslessly() {
        use crate::store::Store;
        use crate::{Chunk, Vector, EMBED_DIM};

        let embedder = OnnxEmbedder::from_resource_dir(&resource_dir()).expect("load model");
        let vecs = embedder
            .embed_documents(&["round trip check"])
            .expect("embed");
        assert_eq!(vecs[0].len(), EMBED_DIM);

        let mut fixed = [0f32; EMBED_DIM];
        fixed.copy_from_slice(&vecs[0]);

        let store = Store::open_in_memory().expect("store");
        store
            .insert_test_file("file-1", "notes/a.md")
            .expect("seed file");
        let chunk = Chunk {
            chunk_index: 0,
            heading_path: None,
            content: "round trip check".into(),
            embed_input: "search_document: round trip check".into(),
            content_hash: "abc".into(),
            token_count: 5,
        };
        let chunk_id = "chunk-0".to_string();
        let fin: Vec<(String, Vector)> = vec![(chunk_id.clone(), fixed)];
        store
            .replace_file_chunks("file-1", &[chunk], &fin)
            .expect("persist");

        let roundtrip = store.chunks_for_file("file-1").expect("read back");
        assert_eq!(roundtrip.len(), 1);
        assert_eq!(roundtrip[0].vector, fixed, "vector altered by round-trip");
    }
}
