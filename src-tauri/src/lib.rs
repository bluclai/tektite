use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use tektite_embed::{
    compute_mutual_knn, EmbedProgress, EmbedService, Embedder, FakeEmbedder, KnnProgress,
    MutualKnnOptions, OnnxEmbedder, Priority, SemanticHit,
};
use tektite_index::{
    BacklinkRow, FuzzyFileRow, GraphData, GraphEdge, GraphFilters, HeadingSearchRow, TagSearchRow,
    UnresolvedReport, UnresolvedSourceRef,
};
use tektite_search::SearchResult;
use tektite_vault::watcher::WatcherHandle;
use tektite_vault::{RenameOutcome, RenamePlan, Vault, VaultError, VaultTreeEntry};

// ---------------------------------------------------------------------------
// Managed state types
// ---------------------------------------------------------------------------

/// The currently open vault.
///
/// Wrapped in `Arc` so the filesystem watcher callback (which runs on a
/// separate thread) can clone a reference and update the index without
/// routing through the Tauri command layer.
struct VaultState(Arc<Mutex<Option<Vault>>>);

/// The filesystem watcher handle. Kept alive for the lifetime of the open
/// vault; replaced when a new vault is opened.
struct WatcherState(Mutex<Option<WatcherHandle>>);

/// In-memory cache of mutual-kNN graph edges, plus the set of currently
/// in-flight request cancellation flags.
///
/// Results are keyed by `(cache_version, filters_hash, k, min_similarity_bits)`
/// so a repeated call with identical inputs skips the O(n²) scan. Cache entries
/// are implicitly invalidated when the embedding cache mutates (the version
/// counter bumps, so a stale key can never collide).
#[derive(Default)]
struct KnnInner {
    cached: HashMap<KnnCacheKey, Vec<GraphEdge>>,
    cancel: HashMap<String, Arc<AtomicBool>>,
}

struct KnnState(Mutex<KnnInner>);

#[derive(PartialEq, Eq, Hash)]
struct KnnCacheKey {
    cache_version: u64,
    filters_hash: u64,
    k: u32,
    min_sim_bits: u32,
}

#[derive(Debug, Serialize, Clone)]
struct VaultFilesChangedPayload {
    paths: Vec<String>,
}

/// Payload emitted on `index:stats-changed` and returned by `index_get_vault_stats`.
#[derive(Debug, Serialize, Clone)]
struct IndexStatsPayload {
    note_count: u32,
    link_count: u32,
    unresolved_link_count: u32,
    /// Unix timestamp in milliseconds — when the index last settled.
    indexed_at: i64,
}

/// Builds an [`IndexStatsPayload`] from an open vault.
/// Returns `None` if the vault has no index yet.
fn build_stats_payload(vault: &Vault) -> Option<IndexStatsPayload> {
    let stats = vault.index.as_ref()?.vault_stats().ok()?;
    let indexed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    Some(IndexStatsPayload {
        note_count: stats.note_count,
        link_count: stats.link_count,
        unresolved_link_count: stats.unresolved_link_count,
        indexed_at,
    })
}

#[derive(Debug, Serialize, Clone)]
struct RenameResult {
    old_path: String,
    new_path: String,
    changed_paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// Helper: map VaultError to a String for the IPC boundary
// ---------------------------------------------------------------------------

fn ve(e: VaultError) -> String {
    e.to_string()
}

/// Resolves the ONNX model resource directory, if available.
fn resolve_embed_dir(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::path::BaseDirectory;
    app.path()
        .resolve("resources/embed", BaseDirectory::Resource)
        .ok()
}

/// Opens the embed service in background mode with lazy model loading.
///
/// Document embeddings are processed on a dedicated worker thread.
/// The ONNX model is loaded lazily on first embed job (or prewarm).
/// Progress events are emitted via `embed:progress`.
///
/// If the model files aren't present (no `resources/embed` dir) the
/// embed service is not created and `embed:unavailable` is emitted so
/// the frontend can hide semantic UI. The app still works — FTS, backlinks,
/// etc. are unaffected.
fn build_embed_service(
    app: &tauri::AppHandle,
    db_path: &Path,
) -> Option<EmbedService> {
    let embed_dir = match resolve_embed_dir(app) {
        Some(dir) if dir.join("model.onnx").exists() => dir,
        _ => {
            eprintln!("ONNX model not found — semantic search disabled");
            let _ = app.emit("embed:unavailable", ());
            return None;
        }
    };

    // Query embedder — loaded synchronously for search_semantic.
    let query_embedder: Box<dyn Embedder> = match OnnxEmbedder::from_resource_dir(&embed_dir) {
        Ok(e) => Box::new(e),
        Err(e) => {
            eprintln!("ONNX query embedder failed: {e} — semantic search disabled");
            let _ = app.emit("embed:unavailable", ());
            return None;
        }
    };

    // Factory for the worker thread's document embedder (lazy load).
    let factory_dir = embed_dir.clone();
    let embedder_factory = move || -> Box<dyn Embedder> {
        match OnnxEmbedder::from_resource_dir(&factory_dir) {
            Ok(e) => Box::new(e),
            Err(e) => {
                // Worker can't load the model — fall back to FakeEmbedder
                // so the worker thread doesn't panic. Jobs will produce
                // deterministic but non-semantic vectors; this is a rare
                // edge case (query embedder loaded fine, worker didn't).
                eprintln!("ONNX worker embedder failed: {e}");
                Box::new(FakeEmbedder::new())
            }
        }
    };

    let app_handle = app.clone();
    let on_progress = move |progress: EmbedProgress| {
        let _ = app_handle.emit("embed:progress", progress);
    };

    match EmbedService::open_background(db_path, query_embedder, embedder_factory, on_progress) {
        Ok(svc) => Some(svc),
        Err(e) => {
            eprintln!("embed service unavailable: {e}");
            let _ = app.emit("embed:unavailable", ());
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Vault management commands
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VaultEntry {
    path: String,
    name: String,
}

fn recent_vaults_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to get app data dir")
        .join("recent_vaults.json")
}

fn read_recent_vaults(app: &tauri::AppHandle) -> Vec<VaultEntry> {
    let path = recent_vaults_path(app);
    if !path.exists() {
        return vec![];
    }
    let content = fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&content).unwrap_or_default()
}

fn write_recent_vaults(app: &tauri::AppHandle, vaults: &[VaultEntry]) {
    let path = recent_vaults_path(app);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(vaults) {
        let _ = fs::write(&path, json);
    }
}

#[tauri::command]
fn vault_get_recent(app: tauri::AppHandle) -> Vec<VaultEntry> {
    read_recent_vaults(&app)
}

/// Opens a vault: creates a [`Vault`] in managed state and starts the watcher.
#[tauri::command]
fn vault_open(
    app: tauri::AppHandle,
    path: String,
    vault_state: State<VaultState>,
    watcher_state: State<WatcherState>,
) -> Result<VaultEntry, String> {
    // Open the vault (runs Index migrations that create the `chunks`
    // table) *before* constructing the embed service which needs that
    // table to exist.
    let mut vault = Vault::open_without_embed(&path).map_err(ve)?;

    // Now that the Index has run its migrations, build the embed service
    // in background mode.
    let db_path = vault.db_path();
    if let Some(svc) = build_embed_service(&app, &db_path) {
        vault.set_embed_service(svc);
    }
    let write_tokens = vault.write_tokens.clone();
    let vault_root = vault.root.clone();

    // Use the canonical root path for the entry returned to the frontend.
    // Vault::open() canonicalizes the path (resolves symlinks, `..`, etc.),
    // and we must use that same canonical form everywhere so vault-relative
    // path construction on the frontend matches what the backend stores.
    let canonical_path = vault_root.to_str().unwrap_or(&path).to_string();
    let name = PathBuf::from(&canonical_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Vault")
        .to_string();

    // Scan existing markdown files into the index. mtime-guarded so
    // subsequent opens of the same vault are fast.
    vault.scan_and_index().map_err(ve)?;

    // Schedule background model prewarm ~2s after vault open. The
    // delay avoids contending with the initial scan_and_index. On an
    // already-indexed vault (no backlog) this ensures the model is warm
    // by the time the user searches.
    //
    // We need to send the prewarm message *after* the vault is in managed
    // state, so we capture a clone of the vault Arc and fire from a
    // detached thread.
    {
        let vault_arc = vault_state.0.clone();
        std::thread::Builder::new()
            .name("embed-prewarm".into())
            .spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let guard = vault_arc.lock().unwrap();
                if let Some(vault) = guard.as_ref() {
                    if let Some(embed) = vault.embed.as_ref() {
                        embed.prewarm();
                    }
                }
            })
            .ok(); // fire-and-forget — failure is non-fatal
    }

    // Capture stats before the vault is moved into managed state.
    let maybe_stats = build_stats_payload(&vault);

    // Replace vault state.
    *vault_state.0.lock().unwrap() = Some(vault);

    // Start the watcher (replaces any existing one).
    // The callback receives the list of external change events so it can
    // update the index before notifying the frontend.
    let vault_arc = vault_state.0.clone();
    let app_clone = app.clone();
    let events_root = vault_root.clone();
    let handle = tektite_vault::watcher::start(vault_root, write_tokens, move |events| {
        let mut changed_paths: Vec<String> = Vec::new();

        let mut guard = vault_arc.lock().unwrap();
        if let Some(vault) = guard.as_mut() {
            for event in &events {
                use tektite_vault::watcher::WatchEventKind;
                let result = match event.kind {
                    WatchEventKind::Remove => vault.remove_from_index(&event.path),
                    WatchEventKind::Create | WatchEventKind::Modify => {
                        vault.reindex_file(&event.path)
                    }
                };
                if let Err(e) = result {
                    eprintln!("index update failed for {:?}: {e}", event.path);
                }

                if let Ok(rel) = event.path.strip_prefix(&events_root) {
                    let rel = rel.to_string_lossy().replace('\\', "/");
                    if !changed_paths.iter().any(|p| p == &rel) {
                        changed_paths.push(rel);
                    }
                }
            }
        }
        drop(guard);

        let _ = app_clone.emit("file-tree-updated", ());
        if !changed_paths.is_empty() {
            let _ = app_clone.emit(
                "vault-files-changed",
                VaultFilesChangedPayload {
                    paths: changed_paths,
                },
            );
        }
    })
    .map_err(ve)?;
    *watcher_state.0.lock().unwrap() = Some(handle);

    // Update recent vaults list.
    let entry = VaultEntry {
        path: canonical_path.clone(),
        name: name.clone(),
    };
    let mut vaults = read_recent_vaults(&app);
    vaults.retain(|v| v.path != canonical_path);
    vaults.insert(0, entry.clone());
    vaults.truncate(10);
    write_recent_vaults(&app, &vaults);

    // Push initial stats to the frontend now that the index is ready.
    if let Some(payload) = maybe_stats {
        let _ = app.emit("index:stats-changed", payload);
    }

    Ok(entry)
}

// ---------------------------------------------------------------------------
// Editor file I/O commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn editor_read_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| format!("Failed to open {path}: {e}"))
}

/// Reads at most `max_bytes` from the start of a file.
///
/// Designed for tooltip previews — avoids loading a 100 KB note to show
/// the first 200 chars. The slice is truncated to a valid UTF-8 boundary
/// so the returned string is always safe to use.
#[tauri::command]
fn preview_get_content(path: String, max_bytes: usize) -> Result<String, String> {
    use std::io::{BufReader, Read};
    let file = fs::File::open(&path).map_err(|e| format!("preview_get_content: {e}"))?;
    let mut reader = BufReader::new(file);
    let mut buf = vec![0u8; max_bytes];
    let n = reader
        .read(&mut buf)
        .map_err(|e| format!("preview_get_content read: {e}"))?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Writes file content and immediately updates the index.
///
/// Routes through the vault so write-tokens are registered (preventing the
/// watcher from treating the save as an external change) and the index stays
/// current without waiting for the watcher round-trip.
#[tauri::command]
fn editor_write_file(
    app: tauri::AppHandle,
    path: String,
    content: String,
    vault_state: State<VaultState>,
) -> Result<(), String> {
    let maybe_stats = {
        let mut guard = vault_state.0.lock().unwrap();
        let vault = guard.as_mut().ok_or("No vault open")?;

        // Derive the vault-relative path.
        let abs = PathBuf::from(&path);
        let rel = abs
            .strip_prefix(&vault.root)
            .map_err(|_| format!("Cannot save outside the open vault: {path}"))?
            .to_string_lossy()
            .replace('\\', "/");

        // Write through the vault (registers write tokens for watcher suppression).
        vault
            .write_file(&rel, &content)
            .map_err(|e| format!("Failed to save {rel}: {}", ve(e)))?;

        // Immediately re-index so backlinks and link resolution are current.
        let mut indexed_file_id: Option<String> = None;
        let mut parsed_for_embed: Option<tektite_parser::ParsedNote> = None;
        if let Some(index) = vault.index.as_mut() {
            let mtime = fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let parsed = tektite_parser::parse(&content);
            match index.upsert(&rel, mtime, &parsed) {
                Ok(id) => {
                    indexed_file_id = Some(id);
                    parsed_for_embed = Some(parsed);
                }
                Err(e) => eprintln!("editor_write_file: inline reindex failed: {e}"),
            }
        }

        // Queue re-embedding with High priority so it jumps ahead of any
        // vault-open backlog. In background mode this returns immediately.
        if let (Some(embed), Some(file_id), Some(parsed)) = (
            vault.embed.as_ref(),
            indexed_file_id.as_ref(),
            parsed_for_embed.as_ref(),
        ) {
            let title = embed_title(&rel, parsed);
            if let Err(e) = embed.reindex_file_with_priority(
                file_id,
                &title,
                parsed,
                Priority::High,
            ) {
                eprintln!("editor_write_file: embed reindex failed: {e}");
            }
        }

        build_stats_payload(vault)
    };

    if let Some(payload) = maybe_stats {
        let _ = app.emit("index:stats-changed", payload);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// File-explorer commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn files_get_tree(vault_state: State<VaultState>) -> Result<Vec<VaultTreeEntry>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    vault.get_tree().map_err(ve)
}

#[tauri::command]
fn files_create_file(
    app: tauri::AppHandle,
    rel_path: String,
    initial_content: Option<String>,
    vault_state: State<VaultState>,
) -> Result<Vec<VaultTreeEntry>, String> {
    let (tree, maybe_stats) = {
        let mut guard = vault_state.0.lock().unwrap();
        let vault = guard.as_mut().ok_or("No vault open")?;
        vault
            .create_file(&rel_path, initial_content.as_deref())
            .map_err(ve)?;

        let abs = vault.absolute_path(&rel_path).map_err(ve)?;
        if let Err(error) = vault.reindex_file(&abs) {
            eprintln!("files_create_file: failed to index new file {rel_path}: {error}");
        }

        let tree = vault.get_tree().map_err(ve)?;
        let stats = build_stats_payload(vault);
        (tree, stats)
    };

    if let Some(payload) = maybe_stats {
        let _ = app.emit("index:stats-changed", payload);
    }

    Ok(tree)
}

#[tauri::command]
fn files_create_folder(
    rel_path: String,
    vault_state: State<VaultState>,
) -> Result<Vec<VaultTreeEntry>, String> {
    let mut guard = vault_state.0.lock().unwrap();
    let vault = guard.as_mut().ok_or("No vault open")?;
    vault.create_folder(&rel_path).map_err(ve)?;
    vault.get_tree().map_err(ve)
}

#[tauri::command]
fn files_delete(
    app: tauri::AppHandle,
    rel_path: String,
    vault_state: State<VaultState>,
) -> Result<Vec<VaultTreeEntry>, String> {
    let (tree, maybe_stats) = {
        let mut guard = vault_state.0.lock().unwrap();
        let vault = guard.as_mut().ok_or("No vault open")?;

        // Remove from index *before* deleting from disk so the index cannot
        // end up with a dangling entry (the watcher will also fire later,
        // making this idempotent).
        let abs = vault.absolute_path(&rel_path).map_err(ve)?;
        let _ = vault.remove_from_index(&abs);

        // Register a write token so the watcher suppresses the delete event
        // and doesn't redundantly try to remove from index.
        vault.write_tokens.insert(abs.clone());

        vault.delete(&rel_path).map_err(ve)?;

        let tree = vault.get_tree().map_err(ve)?;
        let stats = build_stats_payload(vault);
        (tree, stats)
    };

    if let Some(payload) = maybe_stats {
        let _ = app.emit("index:stats-changed", payload);
    }

    Ok(tree)
}

// ---------------------------------------------------------------------------
// Rename commands
// ---------------------------------------------------------------------------

/// Returns a preview of all wiki-link rewrites required to rename `old_path`
/// to `new_path`. No side effects — safe to call before asking the user to
/// confirm.
#[tauri::command]
fn vault_plan_rename(
    old_path: String,
    new_path: String,
    vault_state: State<VaultState>,
) -> Result<RenamePlan, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    vault.plan_rename(&old_path, &new_path).map_err(ve)
}

/// Executes a previously computed [`RenamePlan`]: rewrites affected files,
/// renames the target on disk, and updates the index.
#[tauri::command]
fn vault_apply_rename(
    app: tauri::AppHandle,
    plan: RenamePlan,
    vault_state: State<VaultState>,
) -> Result<RenameResult, String> {
    let outcome = {
        let mut guard = vault_state.0.lock().unwrap();
        let vault = guard.as_mut().ok_or("No vault open")?;
        vault.apply_rename(&plan).map_err(ve)?
    };

    let RenameOutcome {
        old_path,
        new_path,
        changed_paths,
    } = outcome;

    let _ = app.emit("file-tree-updated", ());
    if !changed_paths.is_empty() {
        let _ = app.emit(
            "vault-files-changed",
            VaultFilesChangedPayload {
                paths: changed_paths.clone(),
            },
        );
    }

    // Emit updated stats — rename rewrites links, so counts may change.
    {
        let guard = vault_state.0.lock().unwrap();
        if let Some(vault) = guard.as_ref() {
            if let Some(payload) = build_stats_payload(vault) {
                let _ = app.emit("index:stats-changed", payload);
            }
        }
    }

    Ok(RenameResult {
        old_path,
        new_path,
        changed_paths,
    })
}

// ---------------------------------------------------------------------------
// Index stats command
// ---------------------------------------------------------------------------

/// Returns current vault-wide aggregate stats (note count, link count, unresolved count).
///
/// The frontend also receives these via the `index:stats-changed` push event
/// after every index mutation. This command is used to fetch the initial state
/// on vault open before the first event arrives.
#[tauri::command]
fn index_get_vault_stats(vault_state: State<VaultState>) -> Result<IndexStatsPayload, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    build_stats_payload(vault).ok_or_else(|| "Index not available".into())
}

// ---------------------------------------------------------------------------
// Index query commands (Phase 7: wiki-link foundation)
// ---------------------------------------------------------------------------

/// The result of resolving a wiki-link, serialised for the frontend.
///
/// Maps `LinkResolution` to a JSON-friendly tagged union that TypeScript
/// can discriminate on the `kind` field.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LinkResolutionResult {
    Resolved { path: String },
    Unresolved,
    Ambiguous { paths: Vec<String> },
}

/// Resolves a wiki-link target string against the vault index.
///
/// `target`      — the raw target text (e.g. `"Note"`, `"folder/Note"`)
/// `source_path` — vault-relative path of the file containing the link,
///                 used for proximity tiebreaking (optional).
#[tauri::command]
fn index_resolve_link(
    target: String,
    source_path: Option<String>,
    vault_state: State<VaultState>,
) -> Result<LinkResolutionResult, String> {
    use tektite_index::LinkResolution;

    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let resolution = index
        .resolve_link(&target, source_path.as_deref())
        .map_err(|e| e.to_string())?;

    let id_to_path = |id: &str| -> Result<String, String> {
        index
            .path_for_id(id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Note ID {} not found", id))
    };

    let result = match resolution {
        LinkResolution::Resolved(id) => {
            let path = id_to_path(&id)?;
            LinkResolutionResult::Resolved { path }
        }
        LinkResolution::Unresolved => LinkResolutionResult::Unresolved,
        LinkResolution::Ambiguous(ids) => {
            let mut paths: Vec<_> = ids
                .iter()
                .map(|id| id_to_path(id))
                .collect::<Result<Vec<_>, _>>()?;
            paths.sort();
            LinkResolutionResult::Ambiguous { paths }
        }
    };

    Ok(result)
}

/// Returns all indexed files as `{ path, name }` records for autocomplete.
#[tauri::command]
fn index_get_files(vault_state: State<VaultState>) -> Result<Vec<FileCompletionEntry>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let files = index.all_files().map_err(|e| e.to_string())?;
    let entries = files
        .into_iter()
        .filter(|f| f.path.ends_with(".md"))
        .map(|f| {
            let name = PathBuf::from(&f.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            FileCompletionEntry { path: f.path, name }
        })
        .collect();

    Ok(entries)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileCompletionEntry {
    path: String,
    name: String,
}

/// Returns all headings in a given file (by vault-relative path).
/// Used for `[[note#heading]]` fragment autocomplete.
#[tauri::command]
fn index_get_headings_for_file(
    file_path: String,
    vault_state: State<VaultState>,
) -> Result<Vec<HeadingCompletionEntry>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let file_id = index
        .id_for_path(&file_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("File not in index: {}", file_path))?;

    let headings = index.get_headings(&file_id).map_err(|e| e.to_string())?;

    let entries = headings
        .into_iter()
        .map(|h| HeadingCompletionEntry {
            level: h.level,
            text: h.text,
        })
        .collect();

    Ok(entries)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HeadingCompletionEntry {
    level: u8,
    text: String,
}

// ---------------------------------------------------------------------------
// Backlinks commands (Phase 9)
// ---------------------------------------------------------------------------

/// A backlink record returned to the frontend.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BacklinkEntry {
    /// Vault-relative path of the note that contains the link.
    pub source_path: String,
    /// Display title of the source note.
    pub source_title: String,
    /// The raw link target text as written in the source file.
    pub target: String,
    /// Optional heading fragment, e.g. `"heading-text"`.
    pub fragment: Option<String>,
    /// Optional display alias used in the link.
    pub alias: Option<String>,
}

/// Returns all notes that link to the given file (by vault-relative path).
#[tauri::command]
fn index_get_backlinks(
    file_path: String,
    vault_state: State<VaultState>,
) -> Result<Vec<BacklinkEntry>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    // Resolve the file path to its NoteId.
    let target_id = index
        .id_for_path(&file_path)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("File not in index: {}", file_path))?;

    let entries = index
        .get_backlink_rows(&target_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|row: BacklinkRow| BacklinkEntry {
            source_path: row.source_path,
            source_title: row.source_title,
            target: row.target,
            fragment: row.fragment,
            alias: row.alias,
        })
        .collect();

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Graph view commands (Phase 0 — link edges only)
// ---------------------------------------------------------------------------

/// Returns every indexed `.md` note with its resolved wiki-link edges.
///
/// Data source for the main-view graph tab. Applies optional filters to
/// nodes, drops edges whose endpoints are filtered out. Returns an empty
/// graph if the vault isn't open or the index isn't available.
#[tauri::command]
fn graph_get_full_vault(
    filters: Option<GraphFilters>,
    vault_state: State<VaultState>,
) -> Result<GraphData, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let filters = filters.unwrap_or_default();
    index.full_vault(&filters).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
struct KnnProgressPayload {
    done: u32,
    total: u32,
    request_id: String,
}

#[derive(Debug, Clone, Serialize)]
struct KnnSignalPayload {
    request_id: String,
}

#[derive(Debug, Serialize)]
struct GraphKnnResponse {
    edges: Vec<GraphEdge>,
}

struct KnnProgressEmitter {
    app: tauri::AppHandle,
    request_id: String,
    cancel: Arc<AtomicBool>,
}

impl KnnProgress for KnnProgressEmitter {
    fn report(&mut self, done: u32, total: u32) {
        let _ = self.app.emit(
            "graph:knn-progress",
            KnnProgressPayload {
                done,
                total,
                request_id: self.request_id.clone(),
            },
        );
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }
}

/// Stable 64-bit hash of a `GraphFilters` value. Used as part of the
/// mutual-kNN cache key so two calls with equivalent filter sets share a
/// cached result even when the serialised forms arrive in different orders.
fn hash_filters(filters: &GraphFilters) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    let mut tags = filters.tags.clone().unwrap_or_default();
    tags.sort();
    tags.hash(&mut hasher);
    filters.folder.hash(&mut hasher);
    filters.modified_after.hash(&mut hasher);
    hasher.finish()
}

/// Computes mutual top-K semantic edges across the embedded corpus.
///
/// Runs brute-force cosine on a blocking worker thread, emitting
/// `graph:knn-progress` events roughly every 50 files. Results are cached
/// by `(cache_version, filters_hash, k, min_similarity)` so repeated calls
/// with identical inputs skip the scan entirely.
///
/// `request_id` is a caller-provided string that `graph_cancel_knn` can
/// target to abort superseded requests. Cancelled requests emit
/// `graph:knn-cancelled` and return an empty edge list.
#[tauri::command]
fn graph_get_mutual_knn(
    app: tauri::AppHandle,
    k: Option<u32>,
    min_similarity: Option<f32>,
    filters: Option<GraphFilters>,
    request_id: String,
    vault_state: State<VaultState>,
    knn_state: State<KnnState>,
) -> Result<GraphKnnResponse, String> {
    let k = k.unwrap_or(4).clamp(1, 16);
    let min_similarity = min_similarity.unwrap_or(0.55).clamp(0.0, 1.0);
    let filters = filters.unwrap_or_default();

    let guard = vault_state.0.lock().unwrap();
    let vault = match guard.as_ref() {
        Some(v) => v,
        None => {
            let _ = app.emit(
                "graph:knn-complete",
                KnnSignalPayload {
                    request_id: request_id.clone(),
                },
            );
            return Ok(GraphKnnResponse { edges: Vec::new() });
        }
    };
    let embed = match vault.embed.as_ref() {
        Some(e) => e,
        None => {
            let _ = app.emit(
                "graph:knn-complete",
                KnnSignalPayload {
                    request_id: request_id.clone(),
                },
            );
            return Ok(GraphKnnResponse { edges: Vec::new() });
        }
    };
    let index = match vault.index.as_ref() {
        Some(i) => i,
        None => {
            let _ = app.emit(
                "graph:knn-complete",
                KnnSignalPayload {
                    request_id: request_id.clone(),
                },
            );
            return Ok(GraphKnnResponse { edges: Vec::new() });
        }
    };

    // Derive the allowed file set from the index so GraphFilters applies
    // before the O(n²) scan runs. An empty filter list means "no filter".
    let allowed = if filters.tags.as_ref().map_or(true, |t| t.is_empty())
        && filters.folder.as_deref().map_or(true, str::is_empty)
        && filters.modified_after.is_none()
    {
        None
    } else {
        let graph_data = index.full_vault(&filters).map_err(|e| e.to_string())?;
        let ids: HashSet<String> = graph_data.nodes.into_iter().map(|n| n.id).collect();
        Some(ids)
    };

    let cache_version = embed.cache().version();
    let cache_key = KnnCacheKey {
        cache_version,
        filters_hash: hash_filters(&filters),
        k,
        min_sim_bits: min_similarity.to_bits(),
    };

    // Fast path: cached result. Still emit a complete signal so listeners
    // that attached after the call started can observe the termination.
    {
        let inner = knn_state.0.lock().unwrap();
        if let Some(cached) = inner.cached.get(&cache_key) {
            let edges = cached.clone();
            drop(inner);
            let _ = app.emit(
                "graph:knn-complete",
                KnnSignalPayload {
                    request_id: request_id.clone(),
                },
            );
            return Ok(GraphKnnResponse { edges });
        }
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut inner = knn_state.0.lock().unwrap();
        inner.cancel.insert(request_id.clone(), cancel.clone());
    }

    let opts = MutualKnnOptions {
        k: k as usize,
        min_similarity,
    };
    let mut emitter = KnnProgressEmitter {
        app: app.clone(),
        request_id: request_id.clone(),
        cancel: cancel.clone(),
    };
    let raw_edges = compute_mutual_knn(embed.cache(), allowed.as_ref(), &opts, &mut emitter);
    let was_cancelled = cancel.load(Ordering::Acquire);

    // Release cached file/index references before the long mutex acquire
    // below — the guard is already bound to `vault`, which we no longer use.
    drop(guard);

    // Map internal edge pairs to GraphEdge DTOs.
    let edges: Vec<GraphEdge> = raw_edges
        .into_iter()
        .map(|e| GraphEdge {
            source: e.source,
            target: e.target,
            kind: "semantic".to_string(),
            score: Some(e.score),
        })
        .collect();

    {
        let mut inner = knn_state.0.lock().unwrap();
        inner.cancel.remove(&request_id);
        if !was_cancelled {
            inner.cached.insert(cache_key, edges.clone());
        }
    }

    if was_cancelled {
        let _ = app.emit(
            "graph:knn-cancelled",
            KnnSignalPayload {
                request_id: request_id.clone(),
            },
        );
        return Ok(GraphKnnResponse { edges: Vec::new() });
    }

    let _ = app.emit(
        "graph:knn-complete",
        KnnSignalPayload {
            request_id: request_id.clone(),
        },
    );
    Ok(GraphKnnResponse { edges })
}

/// Marks an in-flight mutual-kNN request as cancelled. The worker polls the
/// cancel flag inside the compute loop and bails out at the next checkpoint.
/// No-op if the request has already completed or was never started.
#[tauri::command]
fn graph_cancel_knn(request_id: String, knn_state: State<KnnState>) {
    let inner = knn_state.0.lock().unwrap();
    if let Some(flag) = inner.cancel.get(&request_id) {
        flag.store(true, Ordering::Release);
    }
}

/// Returns grouped unresolved wiki-link targets across the vault.
#[tauri::command]
fn index_unresolved_link_report(
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<UnresolvedReport, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let limit = limit.unwrap_or(500).min(5_000);
    index.report_unresolved(limit).map_err(|e| e.to_string())
}

/// Returns source references for a grouped unresolved target.
#[tauri::command]
fn index_unresolved_target_sources(
    target: String,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<UnresolvedSourceRef>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let limit = limit.unwrap_or(500).min(5_000);
    index
        .unresolved_target_sources(&target, limit)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Search commands (Phase 8)
// ---------------------------------------------------------------------------

/// Full-text search over the vault using FTS5.
#[tauri::command]
fn search_full_text(
    query: String,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<SearchResult>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let limit = limit.unwrap_or(20).min(100); // cap at 100
    tektite_search::search(index, &query, limit).map_err(|e| e.to_string())
}

/// Fuzzy-match files by name.
#[tauri::command]
fn search_fuzzy_files(
    query: String,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<FuzzyFileRow>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let limit = limit.unwrap_or(20).min(100);
    index
        .search_fuzzy_files(&query, limit)
        .map_err(|e| e.to_string())
}

/// Search headings across the vault.
#[tauri::command]
fn search_headings(
    query: String,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<HeadingSearchRow>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let limit = limit.unwrap_or(20).min(100);
    index
        .search_headings(&query, limit)
        .map_err(|e| e.to_string())
}

/// Semantic search over the vault's chunk embeddings.
///
/// Embeds the query with the active embedder, runs brute-force cosine
/// similarity against the in-memory cache, and joins back to the `chunks`
/// table for metadata.
///
/// Returns an empty list if no vault is open or the embed service failed
/// to initialise — never an error, so the frontend can treat semantic
/// search as a best-effort enhancement over lexical search.
#[tauri::command]
fn search_semantic(
    query: String,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<SemanticHit>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = match guard.as_ref() {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };
    let embed = match vault.embed.as_ref() {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    let limit = limit.unwrap_or(20).min(100);
    embed.search_semantic(&query, limit).map_err(|e| e.to_string())
}

/// Returns notes semantically related to the given file.
///
/// Computes a centroid vector from the file's chunk embeddings, runs
/// cosine search, and deduplicates by file — one entry per related note.
/// Returns empty if the embed service is unavailable or the file has no
/// embeddings yet.
#[tauri::command]
fn search_related_notes(
    file_path: String,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<SemanticHit>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = match guard.as_ref() {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };
    let embed = match vault.embed.as_ref() {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    let index = match vault.index.as_ref() {
        Some(i) => i,
        None => return Ok(Vec::new()),
    };
    let file_id = match index.id_for_path(&file_path).map_err(|e| e.to_string())? {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };
    let limit = limit.unwrap_or(10).min(50);
    embed
        .search_related_notes(&file_id, limit)
        .map_err(|e| e.to_string())
}

/// Returns chunks similar to a specific section of a note.
///
/// Given a file path and optional heading path, finds the matching chunk
/// and returns similar chunks across the vault. Chunks from the same
/// source file are excluded by default.
#[tauri::command]
fn search_similar_chunks(
    file_path: String,
    heading_path: Option<String>,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<SemanticHit>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = match guard.as_ref() {
        Some(v) => v,
        None => return Ok(Vec::new()),
    };
    let embed = match vault.embed.as_ref() {
        Some(e) => e,
        None => return Ok(Vec::new()),
    };
    let index = match vault.index.as_ref() {
        Some(i) => i,
        None => return Ok(Vec::new()),
    };
    let file_id = match index.id_for_path(&file_path).map_err(|e| e.to_string())? {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };
    let limit = limit.unwrap_or(10).min(50);
    embed
        .search_similar_chunks(
            &file_id,
            heading_path.as_deref(),
            limit,
            true, // exclude same file by default
        )
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Aura — generative continuation (Phase 6)
// ---------------------------------------------------------------------------

/// Returns a stub continuation for the given cursor position in a file.
///
/// Phase-6 scope: returns a canned suggestion so the UI surface, keybindings,
/// and accept/dismiss flow can be exercised. Real model-backed continuation
/// will replace this body in a later phase.
#[tauri::command]
fn aura_continue(file_path: String, cursor_offset: usize) -> Result<String, String> {
    let content = fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to open {file_path}: {e}"))?;

    // Take up to ~120 chars of preceding context for a tiny bit of flavour.
    // The real impl will feed this plus more into the local model.
    let upto = cursor_offset.min(content.len());
    let window_start = upto.saturating_sub(120);
    let prefix = &content[window_start..upto];
    let trimmed = prefix.trim_end();

    let ends_with_sentence = trimmed
        .chars()
        .last()
        .map(|c| matches!(c, '.' | '!' | '?'))
        .unwrap_or(true);

    let suggestion = if ends_with_sentence {
        "The thought unfurls further here — a quiet elaboration the writer might pick up, or sweep aside with a keystroke."
    } else {
        "…and the sentence finds its footing, settling into a cadence the writer can either carry forward or dismiss."
    };

    Ok(suggestion.to_string())
}

/// Derives the display title used when embedding chunks for this note.
/// Mirrors `tektite_vault::note_title` but that helper is crate-private.
fn embed_title(rel_path: &str, note: &tektite_parser::ParsedNote) -> String {
    if let Some(title) = note.frontmatter.get("title").and_then(|v| v.as_str()) {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    PathBuf::from(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel_path)
        .to_string()
}

/// Returns every distinct tag name in the vault (sorted) for the filter UI.
#[tauri::command]
fn index_list_all_tags(vault_state: State<VaultState>) -> Result<Vec<String>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;
    index.all_tag_names().map_err(|e| e.to_string())
}

/// Appends a `[[target]]` wiki-link to the end of `source_path` and reindexes.
///
/// Both paths are vault-relative. The target link text is derived from the
/// target's filename stem (matches the frontend's existing wiki-link style).
/// A blank line is inserted before the link if the file doesn't already end
/// with one, so the appended link doesn't fuse into the previous paragraph.
#[tauri::command]
fn graph_append_wiki_link(
    app: tauri::AppHandle,
    source_path: String,
    target_path: String,
    vault_state: State<VaultState>,
) -> Result<(), String> {
    let maybe_stats = {
        let mut guard = vault_state.0.lock().unwrap();
        let vault = guard.as_mut().ok_or("No vault open")?;

        let existing = vault
            .read_file(&source_path)
            .map_err(|e| format!("Failed to read {source_path}: {}", ve(e)))?;

        let stem = std::path::Path::new(&target_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("Invalid target path: {target_path}"))?;
        let link = format!("[[{stem}]]");

        let mut next = existing;
        if next.ends_with("\n\n") {
            next.push_str(&link);
            next.push('\n');
        } else if next.ends_with('\n') {
            next.push('\n');
            next.push_str(&link);
            next.push('\n');
        } else if next.is_empty() {
            next.push_str(&link);
            next.push('\n');
        } else {
            next.push_str("\n\n");
            next.push_str(&link);
            next.push('\n');
        }

        vault
            .write_file(&source_path, &next)
            .map_err(|e| format!("Failed to save {source_path}: {}", ve(e)))?;

        let mut indexed_file_id: Option<String> = None;
        let mut parsed_for_embed: Option<tektite_parser::ParsedNote> = None;
        if let Some(index) = vault.index.as_mut() {
            let abs = vault.root.join(&source_path);
            let mtime = fs::metadata(&abs)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let parsed = tektite_parser::parse(&next);
            match index.upsert(&source_path, mtime, &parsed) {
                Ok(id) => {
                    indexed_file_id = Some(id);
                    parsed_for_embed = Some(parsed);
                }
                Err(e) => eprintln!("graph_append_wiki_link: reindex failed: {e}"),
            }
        }

        if let (Some(embed), Some(file_id), Some(parsed)) = (
            vault.embed.as_ref(),
            indexed_file_id.as_ref(),
            parsed_for_embed.as_ref(),
        ) {
            let title = embed_title(&source_path, parsed);
            if let Err(e) = embed.reindex_file_with_priority(
                file_id,
                &title,
                parsed,
                Priority::High,
            ) {
                eprintln!("graph_append_wiki_link: embed reindex failed: {e}");
            }
        }

        build_stats_payload(vault)
    };

    if let Some(payload) = maybe_stats {
        let _ = app.emit("index:stats-changed", payload);
    }

    Ok(())
}

/// Search tags across the vault.
#[tauri::command]
fn search_tags(
    query: String,
    limit: Option<usize>,
    vault_state: State<VaultState>,
) -> Result<Vec<TagSearchRow>, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    let index = vault.index.as_ref().ok_or("Index not available")?;

    let limit = limit.unwrap_or(20).min(100);
    index.search_tags(&query, limit).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Workspace persistence
// ---------------------------------------------------------------------------
//
// The Rust layer is a dumb JSON store — the frontend owns the schema and
// handles version checking. We accept and return serde_json::Value so the
// shape can evolve without touching Rust.

fn workspace_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to get app data dir")
        .join("workspace.json")
}

#[tauri::command]
fn workspace_load(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let path = workspace_path(&app);
    if !path.exists() {
        return Err("not found".into());
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
fn workspace_save(app: tauri::AppHandle, state: serde_json::Value) -> Result<(), String> {
    let path = workspace_path(&app);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Pinned notes persistence
// ---------------------------------------------------------------------------
//
// Persisted per-vault at `<vault_root>/.tektite/pinned.json`. Contents are
// opaque JSON owned by the frontend — we just read/write the file.

fn pinned_path(vault_state: &State<VaultState>) -> Result<PathBuf, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or_else(|| "No vault open".to_string())?;
    Ok(vault.root.join(".tektite").join("pinned.json"))
}

#[tauri::command]
fn pinned_load(vault_state: State<VaultState>) -> Result<serde_json::Value, String> {
    let path = pinned_path(&vault_state)?;
    if !path.exists() {
        return Ok(serde_json::json!({ "version": 1, "paths": [] }));
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
fn pinned_save(
    vault_state: State<VaultState>,
    state: serde_json::Value,
) -> Result<(), String> {
    let path = pinned_path(&vault_state)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Graph view persistence
// ---------------------------------------------------------------------------
//
// Persisted per-vault at `<vault_root>/.tektite/graph.json`. Schema is
// owned by the frontend (positions map, viewport, settings, open sections).
// Returns `null` when the file doesn't exist so the caller can treat that
// as first-open and fall back to BFS-ring seeding.

fn graph_state_path(vault_state: &State<VaultState>) -> Result<PathBuf, String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or_else(|| "No vault open".to_string())?;
    Ok(vault.root.join(".tektite").join("graph.json"))
}

#[tauri::command]
fn graph_state_load(vault_state: State<VaultState>) -> Result<serde_json::Value, String> {
    let path = graph_state_path(&vault_state)?;
    if !path.exists() {
        return Ok(serde_json::Value::Null);
    }
    let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
fn graph_state_save(
    vault_state: State<VaultState>,
    state: serde_json::Value,
) -> Result<(), String> {
    let path = graph_state_path(&vault_state)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&state).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(VaultState(Arc::new(Mutex::new(None))))
        .manage(WatcherState(Mutex::new(None)))
        .manage(KnnState(Mutex::new(KnnInner::default())))
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            vault_get_recent,
            vault_open,
            editor_read_file,
            preview_get_content,
            editor_write_file,
            files_get_tree,
            files_create_file,
            files_create_folder,
            files_delete,
            vault_plan_rename,
            vault_apply_rename,
            index_get_vault_stats,
            index_resolve_link,
            index_get_files,
            index_get_headings_for_file,
            index_get_backlinks,
            index_unresolved_link_report,
            index_unresolved_target_sources,
            graph_get_full_vault,
            graph_get_mutual_knn,
            graph_cancel_knn,
            graph_append_wiki_link,
            index_list_all_tags,
            search_full_text,
            search_fuzzy_files,
            search_headings,
            search_tags,
            search_semantic,
            search_related_notes,
            search_similar_chunks,
            workspace_load,
            workspace_save,
            pinned_load,
            pinned_save,
            graph_state_load,
            graph_state_save,
            aura_continue,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
