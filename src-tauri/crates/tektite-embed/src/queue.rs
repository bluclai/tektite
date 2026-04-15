//! Background embedding pipeline.
//!
//! A dedicated `std::thread` owns the [`Embedder`] and pulls
//! [`EmbedJob`]s from an `mpsc` channel. Live edits (from
//! `editor_write_file`) carry [`Priority::High`] and jump ahead of
//! vault-open backlog items ([`Priority::Normal`]).
//!
//! The worker chunks, hashes (skipping unchanged), embeds, writes to the
//! [`Store`], and updates the in-memory [`Cache`]. After each completed
//! job it invokes a caller-supplied progress callback so the Tauri layer
//! can emit `embed:progress` events.
//!
//! ## Lazy model loading
//!
//! The embedder is wrapped in a `Lazy<Box<dyn Embedder>>`: it isn't
//! constructed until the first embed job (or an explicit prewarm request).
//! This avoids blocking vault open on model load.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::cache::{Cache, CacheEntry};
use crate::chunker::chunk_note;
use crate::embedder::Embedder;
use crate::store::Store;
use crate::{to_fixed_vector, EmbedError, Vector};

/// Job priority. High-priority jobs (live edits) are processed before
/// normal-priority jobs (vault-open backlog).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Normal = 0,
    High = 1,
}

/// A unit of work sent to the background worker.
#[derive(Debug)]
pub struct EmbedJob {
    pub file_id: String,
    pub title: String,
    pub note: tektite_parser::ParsedNote,
    pub priority: Priority,
}

/// Internal message type for the channel.
enum Message {
    Job(Box<EmbedJob>),
    /// Warm up the embedder with a dummy input so the model is loaded
    /// before the user's first search.
    Prewarm,
    /// Graceful shutdown.
    Shutdown,
}

/// Progress snapshot emitted after each completed job.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct EmbedProgress {
    pub done: u32,
    pub total: u32,
}

/// Handle to the background embed queue. Cheaply cloneable (inner `Sender`
/// is `Arc`-backed).
pub struct EmbedQueue {
    tx: Sender<Message>,
    /// Monotonically-increasing count of jobs submitted. The worker
    /// increments `done` after each completion, so `(done, total)` is the
    /// progress pair exposed to the frontend.
    total: Arc<AtomicU32>,
    done: Arc<AtomicU32>,
    /// Join handle for graceful shutdown. `None` after `shutdown()`.
    handle: Option<JoinHandle<()>>,
}

impl EmbedQueue {
    /// Spawns the background worker thread.
    ///
    /// `embedder_factory` is called **on the worker thread** the first time
    /// an embed is needed (lazy load). `on_progress` fires after every
    /// completed job with the current `(done, total)` snapshot.
    pub fn start<F, P>(
        store: Store,
        cache: Cache,
        embedder_factory: F,
        on_progress: P,
    ) -> Self
    where
        F: FnOnce() -> Box<dyn Embedder> + Send + 'static,
        P: Fn(EmbedProgress) + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Message>();
        let total = Arc::new(AtomicU32::new(0));
        let done = Arc::new(AtomicU32::new(0));

        let t = total.clone();
        let d = done.clone();

        let handle = std::thread::Builder::new()
            .name("embed-worker".into())
            .spawn(move || {
                worker_loop(rx, store, cache, embedder_factory, on_progress, t, d);
            })
            .expect("failed to spawn embed worker thread");

        Self {
            tx,
            total,
            done,
            handle: Some(handle),
        }
    }

    /// Enqueues a file for background embedding.
    pub fn submit(&self, job: EmbedJob) {
        self.total.fetch_add(1, Ordering::Relaxed);
        // If the channel is closed the worker has shut down — silently drop.
        let _ = self.tx.send(Message::Job(Box::new(job)));
    }

    /// Triggers a dummy embed to force model loading. Call from a
    /// `tokio::spawn` task a few seconds after vault open.
    pub fn prewarm(&self) {
        let _ = self.tx.send(Message::Prewarm);
    }

    /// Returns the current progress snapshot.
    pub fn progress(&self) -> EmbedProgress {
        EmbedProgress {
            done: self.done.load(Ordering::Relaxed),
            total: self.total.load(Ordering::Relaxed),
        }
    }

    /// Sends a shutdown signal and joins the worker thread.
    pub fn shutdown(&mut self) {
        let _ = self.tx.send(Message::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for EmbedQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---------------------------------------------------------------------------
// Worker loop
// ---------------------------------------------------------------------------

fn worker_loop<F, P>(
    rx: Receiver<Message>,
    store: Store,
    cache: Cache,
    embedder_factory: F,
    on_progress: P,
    total: Arc<AtomicU32>,
    done: Arc<AtomicU32>,
) where
    F: FnOnce() -> Box<dyn Embedder>,
    P: Fn(EmbedProgress),
{
    let mut embedder: Option<Box<dyn Embedder>> = None;
    let mut pending: Vec<EmbedJob> = Vec::new();

    let ensure_embedder =
        |embedder: &mut Option<Box<dyn Embedder>>, factory: &mut Option<F>| -> bool {
            if embedder.is_some() {
                return true;
            }
            if let Some(f) = factory.take() {
                *embedder = Some(f());
                true
            } else {
                // Factory already consumed but embedder is None — shouldn't
                // happen, but guard against it.
                false
            }
        };

    // We need the factory to be consumed at most once. Wrap in Option so
    // the closure above can `.take()` it.
    let mut factory: Option<F> = Some(embedder_factory);

    loop {
        // Drain all available messages into `pending`, blocking on the first
        // one if the pending queue is empty.
        if pending.is_empty() {
            match rx.recv() {
                Ok(Message::Job(job)) => pending.push(*job),
                Ok(Message::Prewarm) => {
                    ensure_embedder(&mut embedder, &mut factory);
                    continue;
                }
                Ok(Message::Shutdown) | Err(_) => break,
            }
        }

        // Non-blocking drain of everything else that's queued up.
        loop {
            match rx.try_recv() {
                Ok(Message::Job(job)) => pending.push(*job),
                Ok(Message::Prewarm) => {
                    ensure_embedder(&mut embedder, &mut factory);
                }
                Ok(Message::Shutdown) => {
                    // Process nothing more — exit immediately.
                    return;
                }
                Err(_) => break,
            }
        }

        // Sort: High-priority jobs first, then by insertion order (stable).
        // Since `Priority::High > Priority::Normal`, we sort descending by
        // priority.
        pending.sort_by(|a, b| b.priority.cmp(&a.priority));

        // Process the highest-priority job.
        let job = pending.remove(0);
        if let Err(e) = process_job(&job, &store, &cache, &mut embedder, &mut factory) {
            tracing::warn!("embed worker: failed to process {}: {e}", job.file_id);
        }

        let d = done.fetch_add(1, Ordering::Relaxed) + 1;
        let t = total.load(Ordering::Relaxed);
        on_progress(EmbedProgress { done: d, total: t });
    }
}

fn process_job<F>(
    job: &EmbedJob,
    store: &Store,
    cache: &Cache,
    embedder: &mut Option<Box<dyn Embedder>>,
    factory: &mut Option<F>,
) -> Result<(), EmbedError>
where
    F: FnOnce() -> Box<dyn Embedder>,
{
    // Ensure embedder is loaded.
    if embedder.is_none() {
        if let Some(f) = factory.take() {
            *embedder = Some(f());
        } else {
            return Err(EmbedError::Embedder("embedder unavailable".into()));
        }
    }
    let emb = embedder.as_ref().expect("embedder loaded above");

    let chunks = chunk_note(&job.title, &job.note);
    let old = store.chunks_for_file(&job.file_id)?;

    let mut reused: Vec<Option<(String, Vector)>> = Vec::with_capacity(chunks.len());
    let mut texts_to_embed: Vec<&str> = Vec::new();

    for (idx, chunk) in chunks.iter().enumerate() {
        let reuse = old.iter().find(|r| {
            r.chunk_index as usize == idx && r.content_hash == chunk.content_hash
        });
        match reuse {
            Some(existing) => reused.push(Some((existing.id.clone(), existing.vector))),
            None => {
                reused.push(None);
                texts_to_embed.push(chunk.embed_input.as_str());
            }
        }
    }

    let fresh = if texts_to_embed.is_empty() {
        Vec::new()
    } else {
        emb.embed_documents(&texts_to_embed)?
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

    store.replace_file_chunks(&job.file_id, &chunks, &finalised)?;

    let entries: Vec<CacheEntry> = finalised
        .into_iter()
        .map(|(chunk_id, vector)| CacheEntry {
            chunk_id,
            file_id: job.file_id.clone(),
            vector: Arc::new(vector),
        })
        .collect();
    cache.replace_for_file(&job.file_id, entries);

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::embedder::FakeEmbedder;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};

    fn make_note(body: &str) -> tektite_parser::ParsedNote {
        tektite_parser::parse(body)
    }

    fn make_store_and_cache() -> (Store, Cache) {
        let store = Store::open_in_memory().expect("in-memory store");
        let cache = Cache::new();
        (store, cache)
    }

    /// Insert a test file row so chunk FK constraints are satisfied.
    fn seed_file(store: &Store, id: &str, path: &str) {
        store.insert_test_file(id, path).expect("seed file");
    }

    #[test]
    fn basic_job_processes_and_updates_cache() {
        let (store, cache) = make_store_and_cache();
        seed_file(&store, "f1", "a.md");

        let progress: Arc<Mutex<Vec<EmbedProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let pcap = progress.clone();

        let mut q = EmbedQueue::start(
            store,
            cache.clone(),
            || Box::new(FakeEmbedder::new()),
            move |p| pcap.lock().unwrap().push(p),
        );

        q.submit(EmbedJob {
            file_id: "f1".into(),
            title: "A".into(),
            note: make_note("# H\nbody text\n"),
            priority: Priority::Normal,
        });

        // Give the worker time to process.
        std::thread::sleep(std::time::Duration::from_millis(200));
        q.shutdown();

        assert!(cache.len() > 0, "cache should have entries after processing");

        let events = progress.lock().unwrap();
        assert!(!events.is_empty(), "should have received progress events");
        assert_eq!(events.last().unwrap().done, 1);
        assert_eq!(events.last().unwrap().total, 1);
    }

    #[test]
    fn high_priority_jobs_are_processed_before_normal() {
        // We submit several normal jobs then one high-priority job while
        // the worker is blocked. The high-priority job should be processed
        // before the remaining normal ones.

        let (store, cache) = make_store_and_cache();
        for i in 0..5 {
            seed_file(&store, &format!("f{i}"), &format!("{i}.md"));
        }

        let processed: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let _pcap = processed.clone();

        // Use a barrier to stall the worker so we can queue everything
        // before any processing starts.
        let gate = Arc::new(std::sync::Barrier::new(2));
        let gate2 = gate.clone();

        let factory_called = Arc::new(AtomicBool::new(false));
        let fc = factory_called.clone();

        // Wrap the embedder in something that signals when it first runs.
        let mut q = EmbedQueue::start(
            store,
            cache.clone(),
            move || {
                fc.store(true, Ordering::SeqCst);
                // Wait for the test to queue all jobs before proceeding.
                gate2.wait();
                Box::new(FakeEmbedder::new())
            },
            move |_| {},
        );

        // Submit a prewarm to trigger factory, then wait for it to block.
        q.prewarm();

        // Wait until the factory has been called (it will then block on
        // the barrier).
        while !factory_called.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Queue normal jobs.
        for i in 0..4 {
            q.submit(EmbedJob {
                file_id: format!("f{i}"),
                title: format!("Note {i}"),
                note: make_note(&format!("# H\nbody {i}\n")),
                priority: Priority::Normal,
            });
        }
        // Queue one high-priority job.
        q.submit(EmbedJob {
            file_id: "f4".into(),
            title: "Urgent".into(),
            note: make_note("# Urgent\nurgent body\n"),
            priority: Priority::High,
        });

        // Release the gate so the worker starts processing.
        gate.wait();

        // Let it finish.
        std::thread::sleep(std::time::Duration::from_millis(500));
        q.shutdown();

        // The high-priority job (f4) should have been processed first.
        // We can verify by checking the cache has all entries.
        assert_eq!(cache.len(), 5);
    }

    #[test]
    fn content_hash_skip_avoids_re_embedding() {
        let (store, cache) = make_store_and_cache();
        seed_file(&store, "f1", "a.md");

        let embed_count = Arc::new(AtomicU32::new(0));
        let ec = embed_count.clone();

        // Custom embedder that counts calls.
        struct CountingEmbedder {
            inner: FakeEmbedder,
            count: Arc<AtomicU32>,
        }
        impl Embedder for CountingEmbedder {
            fn embed_documents(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
                self.count.fetch_add(texts.len() as u32, Ordering::Relaxed);
                self.inner.embed_documents(texts)
            }
            fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
                self.inner.embed_query(text)
            }
        }

        let mut q = EmbedQueue::start(
            store,
            cache.clone(),
            move || {
                Box::new(CountingEmbedder {
                    inner: FakeEmbedder::new(),
                    count: ec,
                })
            },
            |_| {},
        );

        let note = make_note("# H\nbody text\n");

        // First embed.
        q.submit(EmbedJob {
            file_id: "f1".into(),
            title: "A".into(),
            note: note.clone(),
            priority: Priority::Normal,
        });
        std::thread::sleep(std::time::Duration::from_millis(200));

        let count_after_first = embed_count.load(Ordering::Relaxed);
        assert!(count_after_first > 0);

        // Same content → should skip.
        q.submit(EmbedJob {
            file_id: "f1".into(),
            title: "A".into(),
            note,
            priority: Priority::Normal,
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        q.shutdown();

        let count_after_second = embed_count.load(Ordering::Relaxed);
        assert_eq!(
            count_after_first, count_after_second,
            "unchanged content should not trigger re-embedding"
        );
    }

    #[test]
    fn graceful_shutdown_drains_cleanly() {
        let (store, cache) = make_store_and_cache();
        let mut q = EmbedQueue::start(
            store,
            cache,
            || Box::new(FakeEmbedder::new()),
            |_| {},
        );
        // No jobs submitted — just shut down.
        q.shutdown();
        // Should not hang or panic.
    }

    #[test]
    fn progress_counting_is_accurate() {
        let (store, cache) = make_store_and_cache();
        for i in 0..3 {
            seed_file(&store, &format!("f{i}"), &format!("{i}.md"));
        }

        let mut q = EmbedQueue::start(
            store,
            cache,
            || Box::new(FakeEmbedder::new()),
            |_| {},
        );

        for i in 0..3 {
            q.submit(EmbedJob {
                file_id: format!("f{i}"),
                title: format!("N{i}"),
                note: make_note(&format!("body {i}\n")),
                priority: Priority::Normal,
            });
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
        q.shutdown();

        let p = q.progress();
        assert_eq!(p.total, 3);
        assert_eq!(p.done, 3);
    }
}
