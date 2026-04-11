use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};

use tektite_index::{BacklinkRow, FuzzyFileRow, HeadingSearchRow};
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

#[derive(Debug, Serialize, Clone)]
struct VaultFilesChangedPayload {
    paths: Vec<String>,
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
    // Open the vault and populate the index from disk.
    let mut vault = Vault::open(&path).map_err(ve)?;
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

    Ok(entry)
}

// ---------------------------------------------------------------------------
// Editor file I/O commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn editor_read_file(path: String) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|e| format!("Failed to open {path}: {e}"))
}

/// Writes file content and immediately updates the index.
///
/// Routes through the vault so write-tokens are registered (preventing the
/// watcher from treating the save as an external change) and the index stays
/// current without waiting for the watcher round-trip.
#[tauri::command]
fn editor_write_file(
    path: String,
    content: String,
    vault_state: State<VaultState>,
) -> Result<(), String> {
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
    if let Some(index) = vault.index.as_mut() {
        let mtime = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let parsed = tektite_parser::parse(&content);
        if let Err(e) = index.upsert(&rel, mtime, &parsed) {
            eprintln!("editor_write_file: inline reindex failed: {e}");
        }
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
    rel_path: String,
    vault_state: State<VaultState>,
) -> Result<Vec<VaultTreeEntry>, String> {
    let mut guard = vault_state.0.lock().unwrap();
    let vault = guard.as_mut().ok_or("No vault open")?;
    vault.create_file(&rel_path).map_err(ve)?;

    let abs = vault.absolute_path(&rel_path).map_err(ve)?;
    if let Err(error) = vault.reindex_file(&abs) {
        eprintln!("files_create_file: failed to index new file {rel_path}: {error}");
    }

    vault.get_tree().map_err(ve)
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
fn files_delete(rel_path: String, vault_state: State<VaultState>) -> Result<(), String> {
    let guard = vault_state.0.lock().unwrap();
    let vault = guard.as_ref().ok_or("No vault open")?;
    vault.delete(&rel_path).map_err(ve)
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

    Ok(RenameResult {
        old_path,
        new_path,
        changed_paths,
    })
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
// App entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(VaultState(Arc::new(Mutex::new(None))))
        .manage(WatcherState(Mutex::new(None)))
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            vault_get_recent,
            vault_open,
            editor_read_file,
            editor_write_file,
            files_get_tree,
            files_create_file,
            files_create_folder,
            files_delete,
            vault_plan_rename,
            vault_apply_rename,
            index_resolve_link,
            index_get_files,
            index_get_headings_for_file,
            index_get_backlinks,
            search_full_text,
            search_fuzzy_files,
            search_headings,
            workspace_load,
            workspace_save,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
