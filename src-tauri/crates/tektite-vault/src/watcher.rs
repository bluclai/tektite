//! Filesystem watcher with debouncing and self-write suppression.
//!
//! ## Self-write suppression
//!
//! When the app writes a file it inserts the absolute path into
//! [`WriteTokenSet`] before performing the write. The watcher callback skips
//! paths found in this set and removes them. Entries that are never consumed
//! (because the watcher event was lost) expire after [`TOKEN_TTL`].
//!
//! ## Debouncing
//!
//! `notify-debouncer-full` coalesces rapid filesystem events with a 200 ms
//! window before delivering them to the handler. External events are collected
//! into a [`Vec<WatchEvent>`] and forwarded to the caller-supplied callback so
//! the Tauri command layer can update the index and emit frontend events.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use notify::{EventKind, RecursiveMode, Watcher};
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, FileIdMap};

/// How long a write token survives without being consumed before it is
/// garbage-collected. 2 s is generous — watcher events typically arrive in
/// tens of milliseconds.
const TOKEN_TTL: Duration = Duration::from_secs(2);

/// Debounce window — coalesces rapid filesystem events.
const DEBOUNCE_MS: u64 = 200;

// ---------------------------------------------------------------------------
// Write-token set
// ---------------------------------------------------------------------------

/// Thread-safe set of paths the app has written and is expecting a watcher
/// event for. Paths are stored with an expiry timestamp.
#[derive(Clone)]
pub struct WriteTokenSet(Arc<Mutex<HashMap<PathBuf, Instant>>>);

impl WriteTokenSet {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }

    /// Register that the app is about to write `path`.
    pub fn insert(&self, path: PathBuf) {
        let mut map = self.0.lock().unwrap();
        map.insert(path, Instant::now() + TOKEN_TTL);
    }

    /// Returns `true` and removes the token if `path` was registered and has
    /// not yet expired. Also prunes all expired tokens on each call.
    pub fn consume(&self, path: &PathBuf) -> bool {
        let mut map = self.0.lock().unwrap();
        let now = Instant::now();
        // Prune stale tokens.
        map.retain(|_, expires| *expires > now);
        map.remove(path).is_some()
    }
}

impl Default for WriteTokenSet {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Watch event types
// ---------------------------------------------------------------------------

/// The kind of filesystem change relevant to the vault index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEventKind {
    /// A new file was created.
    Create,
    /// An existing file's content was modified.
    Modify,
    /// A file was deleted.
    Remove,
}

/// A single filesystem event delivered to the change callback.
///
/// Only markdown-relevant external events are forwarded — access events and
/// app-originated writes are suppressed before this type is constructed.
#[derive(Debug, Clone)]
pub struct WatchEvent {
    /// Absolute path to the changed file or directory.
    pub path: PathBuf,
    pub kind: WatchEventKind,
}

// ---------------------------------------------------------------------------
// Watcher handle
// ---------------------------------------------------------------------------

/// Owned handle to the active `notify` debouncer. Dropping this stops the
/// watcher. Store it in Tauri managed state alongside [`Vault`].
pub struct WatcherHandle {
    // The debouncer must stay alive for the watcher to keep running.
    _debouncer: Debouncer<notify::INotifyWatcher, FileIdMap>,
}

/// Starts watching `vault_root`. When non-suppressed filesystem changes are
/// detected after the debounce window, `on_change` is called with the list of
/// external [`WatchEvent`]s so the caller can update the index and notify the
/// frontend.
///
/// The caller (the Tauri command layer) owns the `AppHandle` and provides
/// `on_change` as a closure — this keeps `tektite-vault` free of a `tauri`
/// dependency.
///
/// Returns a [`WatcherHandle`] that must be kept alive for the watcher to
/// remain active.
pub fn start(
    vault_root: PathBuf,
    write_tokens: WriteTokenSet,
    on_change: impl Fn(Vec<WatchEvent>) + Send + 'static,
) -> Result<WatcherHandle, crate::VaultError> {
    let debounce = Duration::from_millis(DEBOUNCE_MS);

    let debouncer = new_debouncer(
        debounce,
        None,
        move |result: DebounceEventResult| match result {
            Ok(events) => {
                let mut external: Vec<WatchEvent> = Vec::new();
                for event in &events {
                    let kind = match event.kind {
                        EventKind::Create(_) => WatchEventKind::Create,
                        EventKind::Modify(_) => WatchEventKind::Modify,
                        EventKind::Remove(_) => WatchEventKind::Remove,
                        // Access and Other events are not meaningful for the index.
                        _ => continue,
                    };
                    for path in &event.paths {
                        if !write_tokens.consume(path) {
                            external.push(WatchEvent {
                                path: path.clone(),
                                kind: kind.clone(),
                            });
                        }
                    }
                }
                if !external.is_empty() {
                    on_change(external);
                }
            }
            Err(errors) => {
                for e in errors {
                    tracing::warn!("watcher error: {e}");
                }
            }
        },
    )
    .map_err(|e| crate::VaultError::Watcher(e.to_string()))?;

    // Borrow the inner watcher to start watching the vault root.
    // notify-debouncer-full 0.3 exposes the inner watcher via .watcher().
    let mut deb = debouncer;
    deb.watcher()
        .watch(&vault_root, RecursiveMode::Recursive)
        .map_err(|e: notify::Error| crate::VaultError::Watcher(e.to_string()))?;

    Ok(WatcherHandle { _debouncer: deb })
}
