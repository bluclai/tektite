//! SQLite-backed chunk + embedding persistence.
//!
//! The `chunks` table is created by `tektite-index`'s migration runner; this
//! module simply opens a second connection to the same database and reads
//! / writes chunk rows. The `FOREIGN KEYS = ON` pragma is enabled
//! per-connection so a file delete cascades into this connection's view
//! of `chunks` as well.
//!
//! All vector I/O is zero-copy via [`bytemuck`]: a `&[f32; 256]` is cast to
//! `&[u8; 1024]` on write, and a 1024-byte `Vec<u8>` is cast back to
//! `&[f32; 256]` on read. Endianness is host-native. The database is
//! app-local — we never share it across machines — so no network-byte-order
//! normalisation is needed.

use std::path::Path;

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use uuid::Uuid;

use crate::{chunker::Chunk, EmbedError, Vector, EMBED_DIM};

const VECTOR_BYTES: usize = EMBED_DIM * std::mem::size_of::<f32>();

/// Minimal view of a stored chunk as seen from the dedup path — lean
/// enough to materialise for every file in the vault on vault open.
#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub id: String,
    pub chunk_index: u32,
    pub content_hash: String,
    pub vector: Vector,
}

/// Metadata joined with the owning file's path, used to hydrate search
/// hits coming out of the cache.
#[derive(Debug, Clone)]
pub struct ChunkMetadata {
    pub id: String,
    pub file_id: String,
    pub file_path: String,
    pub heading_path: Option<String>,
    pub heading_text: Option<String>,
    pub heading_level: Option<u8>,
    pub content: String,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens a second connection to the shared `index.db`.
    pub fn open(db_path: &Path) -> Result<Self, EmbedError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        Ok(Self { conn })
    }

    /// Opens an in-memory index database. Only useful for tests that drive
    /// the `Store` directly — the schema is created here (not via
    /// `tektite-index`) so the tests don't need to pull that crate in.
    #[cfg(any(test, feature = "test-support"))]
    pub fn open_in_memory() -> Result<Self, EmbedError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS files (
                id         TEXT PRIMARY KEY,
                path       TEXT UNIQUE NOT NULL,
                mtime_secs INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS chunks (
                id            TEXT PRIMARY KEY,
                file_id       TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                chunk_index   INTEGER NOT NULL,
                heading_path  TEXT,
                heading_text  TEXT,
                heading_level INTEGER,
                content       TEXT NOT NULL,
                content_hash  TEXT NOT NULL,
                token_count   INTEGER NOT NULL,
                embedding     BLOB NOT NULL
             );
             CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_id);",
        )?;
        Ok(Self { conn })
    }

    /// Test-only: insert a file row so chunk FKs resolve.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_test_file(&self, id: &str, path: &str) -> Result<(), EmbedError> {
        self.conn.execute(
            "INSERT INTO files (id, path, mtime_secs) VALUES (?1, ?2, 0)",
            params![id, path],
        )?;
        Ok(())
    }

    /// Loads every `(chunk_id, file_id, vector)` triple for cache warm-up.
    pub fn all_vectors(&self) -> Result<Vec<(String, String, Vector)>, EmbedError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, file_id, embedding FROM chunks")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let file_id: String = row.get(1)?;
            let blob: Vec<u8> = row.get(2)?;
            Ok((id, file_id, blob))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, file_id, blob) = row?;
            let vec = vector_from_blob(&blob)?;
            out.push((id, file_id, vec));
        }
        Ok(out)
    }

    /// True when at least one chunk exists for the given file. Cheaper
    /// than [`chunks_for_file`] because it never materialises vectors —
    /// intended for the vault-open scan's "does this already-indexed
    /// file need embedding?" check.
    pub fn has_chunks_for_file(&self, file_id: &str) -> Result<bool, EmbedError> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT 1 FROM chunks WHERE file_id = ?1 LIMIT 1")?;
        Ok(stmt.exists(params![file_id])?)
    }

    /// Returns the existing chunks for one file, ordered by `chunk_index`.
    /// Used by [`EmbedService::reindex_file`] to decide which chunks can
    /// skip re-embedding.
    pub fn chunks_for_file(&self, file_id: &str) -> Result<Vec<ChunkRecord>, EmbedError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, chunk_index, content_hash, embedding
             FROM chunks
             WHERE file_id = ?1
             ORDER BY chunk_index",
        )?;
        let rows = stmt.query_map(params![file_id], |row| {
            let id: String = row.get(0)?;
            let idx: i64 = row.get(1)?;
            let hash: String = row.get(2)?;
            let blob: Vec<u8> = row.get(3)?;
            Ok((id, idx, hash, blob))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, idx, hash, blob) = row?;
            out.push(ChunkRecord {
                id,
                chunk_index: idx as u32,
                content_hash: hash,
                vector: vector_from_blob(&blob)?,
            });
        }
        Ok(out)
    }

    /// Fetches metadata for a set of chunk IDs in a single query, joined
    /// with the owning file's path. Ordering is not guaranteed — callers
    /// match by id.
    pub fn chunks_by_ids(&self, ids: &[&str]) -> Result<Vec<ChunkMetadata>, EmbedError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<&str> = (0..ids.len()).map(|_| "?").collect();
        let sql = format!(
            "SELECT c.id, c.file_id, f.path, c.heading_path, c.heading_text, c.heading_level, c.content
             FROM chunks c
             JOIN files f ON f.id = c.file_id
             WHERE c.id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(ids.iter()), |row| {
            let level: Option<i64> = row.get(5)?;
            Ok(ChunkMetadata {
                id: row.get(0)?,
                file_id: row.get(1)?,
                file_path: row.get(2)?,
                heading_path: row.get(3)?,
                heading_text: row.get(4)?,
                heading_level: level.map(|n| n as u8),
                content: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Atomically replaces every chunk row for one file. `chunks[i]` is
    /// paired with `finalised[i]` which holds the chunk's id (either reused
    /// from a previous row or freshly minted) and its embedding.
    pub fn replace_file_chunks(
        &self,
        file_id: &str,
        chunks: &[Chunk],
        finalised: &[(String, Vector)],
    ) -> Result<(), EmbedError> {
        if chunks.len() != finalised.len() {
            return Err(EmbedError::Embedder(format!(
                "replace_file_chunks: {} chunks vs {} vectors",
                chunks.len(),
                finalised.len()
            )));
        }

        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM chunks WHERE file_id = ?1", params![file_id])?;

        for (chunk, (id, vector)) in chunks.iter().zip(finalised.iter()) {
            let bytes = vector_to_bytes(vector);
            tx.execute(
                "INSERT INTO chunks
                 (id, file_id, chunk_index, heading_path, heading_text, heading_level,
                  content, content_hash, token_count, embedding)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    id,
                    file_id,
                    chunk.chunk_index as i64,
                    chunk.heading_path,
                    chunk.heading_text,
                    chunk.heading_level.map(|n| n as i64),
                    chunk.content,
                    chunk.content_hash,
                    chunk.token_count as i64,
                    bytes,
                ],
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Returns the chunk id for a chunk identified by `file_id` and
    /// `heading_path`. When `heading_path` is `None`, matches chunks with
    /// a NULL `heading_path` (i.e. the root chunk of a note that has no
    /// headings).
    pub fn chunk_id_by_heading(
        &self,
        file_id: &str,
        heading_path: Option<&str>,
    ) -> Result<Option<String>, EmbedError> {
        let id: Option<String> = match heading_path {
            Some(hp) => self.conn.query_row(
                "SELECT id FROM chunks WHERE file_id = ?1 AND heading_path = ?2 LIMIT 1",
                params![file_id, hp],
                |row| row.get(0),
            ),
            None => self.conn.query_row(
                "SELECT id FROM chunks WHERE file_id = ?1 AND heading_path IS NULL LIMIT 1",
                params![file_id],
                |row| row.get(0),
            ),
        }
        .optional()?;
        Ok(id)
    }

    /// Mint a fresh chunk id — kept here so all id generation lives next
    /// to the column that owns it.
    pub fn mint_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Test-only: raw access to the connection for manual manipulation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn conn_for_test(&self) -> &Connection {
        &self.conn
    }
}

fn vector_to_bytes(v: &Vector) -> Vec<u8> {
    bytemuck::cast_slice(v.as_slice()).to_vec()
}

fn vector_from_blob(blob: &[u8]) -> Result<Vector, EmbedError> {
    if blob.len() != VECTOR_BYTES {
        return Err(EmbedError::BlobSize {
            expected: VECTOR_BYTES,
            actual: blob.len(),
        });
    }
    let floats: &[f32] = bytemuck::cast_slice(blob);
    let mut out = [0f32; EMBED_DIM];
    out.copy_from_slice(floats);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_vector(seed: u8) -> Vector {
        let mut v = [0f32; EMBED_DIM];
        for (i, x) in v.iter_mut().enumerate() {
            *x = (i as f32) * 0.01 + seed as f32;
        }
        v
    }

    fn sample_chunk(index: usize, content: &str) -> Chunk {
        Chunk {
            chunk_index: index,
            heading_path: Some(format!("H{index}")),
            heading_text: Some(format!("H{index}")),
            heading_level: Some(2),
            content: content.to_string(),
            embed_input: content.to_string(),
            content_hash: format!("hash-{content}"),
            token_count: 1,
        }
    }

    #[test]
    fn insert_and_readback_round_trip() {
        let store = Store::open_in_memory().unwrap();
        store.insert_test_file("file-1", "notes/a.md").unwrap();

        let chunk = sample_chunk(0, "hello");
        let vec = sample_vector(1);
        store
            .replace_file_chunks("file-1", &[chunk.clone()], &[("chunk-a".into(), vec)])
            .unwrap();

        let rows = store.chunks_for_file("file-1").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "chunk-a");
        assert_eq!(rows[0].content_hash, chunk.content_hash);
        assert_eq!(rows[0].vector, vec);
    }

    #[test]
    fn blob_round_trip_is_lossless() {
        let store = Store::open_in_memory().unwrap();
        store.insert_test_file("f", "a.md").unwrap();

        let mut weird = [0f32; EMBED_DIM];
        weird[0] = f32::MIN_POSITIVE;
        weird[1] = f32::MAX;
        weird[2] = -1.234_567_8;
        weird[255] = std::f32::consts::PI;

        store
            .replace_file_chunks(
                "f",
                &[sample_chunk(0, "x")],
                &[("id0".into(), weird)],
            )
            .unwrap();

        let rows = store.chunks_for_file("f").unwrap();
        assert_eq!(rows[0].vector, weird);
    }

    #[test]
    fn replace_deletes_stale_rows() {
        let store = Store::open_in_memory().unwrap();
        store.insert_test_file("f", "a.md").unwrap();

        let v0 = sample_vector(0);
        let v1 = sample_vector(1);
        store
            .replace_file_chunks(
                "f",
                &[sample_chunk(0, "a"), sample_chunk(1, "b")],
                &[("c0".into(), v0), ("c1".into(), v1)],
            )
            .unwrap();
        assert_eq!(store.chunks_for_file("f").unwrap().len(), 2);

        // Second call with only one chunk must drop the stale one.
        store
            .replace_file_chunks("f", &[sample_chunk(0, "a")], &[("c0-new".into(), v0)])
            .unwrap();
        let rows = store.chunks_for_file("f").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "c0-new");
    }

    #[test]
    fn cascade_deletes_chunks_when_file_removed() {
        let store = Store::open_in_memory().unwrap();
        store.insert_test_file("f", "a.md").unwrap();
        store
            .replace_file_chunks(
                "f",
                &[sample_chunk(0, "x")],
                &[("c0".into(), sample_vector(7))],
            )
            .unwrap();

        store
            .conn
            .execute("DELETE FROM files WHERE id = 'f'", [])
            .unwrap();

        assert!(store.chunks_for_file("f").unwrap().is_empty());
    }

    #[test]
    fn chunks_by_ids_returns_metadata_with_file_path() {
        let store = Store::open_in_memory().unwrap();
        store.insert_test_file("f", "notes/a.md").unwrap();
        store
            .replace_file_chunks(
                "f",
                &[sample_chunk(0, "hello")],
                &[("c0".into(), sample_vector(0))],
            )
            .unwrap();

        let metas = store.chunks_by_ids(&["c0"]).unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].file_path, "notes/a.md");
        assert_eq!(metas[0].content, "hello");
        assert_eq!(metas[0].heading_path.as_deref(), Some("H0"));
        assert_eq!(metas[0].heading_text.as_deref(), Some("H0"));
        assert_eq!(metas[0].heading_level, Some(2));
    }

    #[test]
    fn all_vectors_streams_every_row() {
        let store = Store::open_in_memory().unwrap();
        store.insert_test_file("f1", "a.md").unwrap();
        store.insert_test_file("f2", "b.md").unwrap();
        store
            .replace_file_chunks(
                "f1",
                &[sample_chunk(0, "x")],
                &[("a".into(), sample_vector(0))],
            )
            .unwrap();
        store
            .replace_file_chunks(
                "f2",
                &[sample_chunk(0, "y")],
                &[("b".into(), sample_vector(1))],
            )
            .unwrap();

        let all = store.all_vectors().unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|(id, _, _)| id == "a"));
        assert!(all.iter().any(|(id, _, _)| id == "b"));
    }
}
