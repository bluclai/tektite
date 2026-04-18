//! `tektite-vault` — Vault orchestration.
//!
//! Owns filesystem operations, `walkdir`-backed tree scanning, file-system
//! watching (via `notify` + `notify-debouncer-full`), self-write suppression,
//! and the file CRUD operations exposed to the Tauri command layer.
//!
//! The [`Vault`] struct holds a live [`tektite_index::Index`] so rename
//! planning and application can coordinate file I/O with index updates in a
//! single call.

pub mod scan;
pub mod watcher;

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

use tektite_embed::{EmbedService, Embedder, Priority};
use tektite_index::{rewrite_content, Index};

pub use tektite_index::{IndexError, RenameEdit, RenamePlan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameOutcome {
    pub old_path: String,
    pub new_path: String,
    pub changed_paths: Vec<String>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Index error: {0}")]
    Index(#[from] IndexError),
    #[error("Vault not open")]
    NotOpen,
    #[error("Path is outside vault root")]
    OutsideRoot,
    #[error("Invalid path: {0}")]
    InvalidPath(String),
    #[error("Watcher error: {0}")]
    Watcher(String),
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A file or directory entry in the vault tree (serialised to frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultTreeEntry {
    /// Vault-relative path (forward slashes on all platforms).
    pub path: String,
    /// Display name (filename component only).
    pub name: String,
    /// `true` if this entry is a directory.
    pub is_dir: bool,
    /// `true` if this entry is a markdown file.
    pub is_markdown: bool,
    /// Child entries — populated for directories, empty for files.
    pub children: Vec<VaultTreeEntry>,
}

// ---------------------------------------------------------------------------
// Vault
// ---------------------------------------------------------------------------

/// An open vault rooted at a single directory.
///
/// The watcher is initialised separately via [`watcher::start`] once
/// [`Vault`] is placed in Tauri managed state.
pub struct Vault {
    /// Absolute path to the vault root directory.
    pub root: PathBuf,
    /// Self-write suppression set — paths the app itself is about to write.
    pub write_tokens: watcher::WriteTokenSet,
    /// Live SQLite index for the vault. Stored in `.tektite/index.db`.
    pub index: Option<Index>,
    /// Semantic index (chunks + vectors) over the same database. `None` if
    /// the embedder failed to initialise — the rest of the vault keeps
    /// working, semantic search simply returns empty results.
    pub embed: Option<EmbedService>,
}

impl Vault {
    /// Opens a vault at `root` with the caller-provided embedder.
    ///
    /// Creates the `.tektite/` metadata directory if it doesn't exist and
    /// opens (or creates) the index database. The embedder is injected so
    /// production callers can wire in `OnnxEmbedder` while tests use
    /// `FakeEmbedder` deterministically.
    ///
    /// For background-mode embedding (Phase 3), prefer
    /// [`open_with_embed_service`] and construct the [`EmbedService`] with
    /// [`EmbedService::open_background`] in the Tauri layer.
    pub fn open(
        root: impl AsRef<Path>,
        embedder: Box<dyn Embedder>,
    ) -> Result<Self, VaultError> {
        let root = root.as_ref().canonicalize().map_err(VaultError::Io)?;
        if !root.is_dir() {
            return Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault root not found: {}", root.display()),
            )));
        }

        let meta_dir = root.join(".tektite");
        std::fs::create_dir_all(&meta_dir)?;

        let db_path = meta_dir.join("index.db");
        let index = Index::open(&db_path).ok();

        let embed = if index.is_some() {
            match EmbedService::open(&db_path, embedder) {
                Ok(svc) => Some(svc),
                Err(e) => {
                    tracing::warn!("embed service unavailable: {e}");
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            root,
            write_tokens: watcher::WriteTokenSet::new(),
            index,
            embed,
        })
    }

    /// Opens a vault without an embed service, then lets the caller
    /// attach one via [`set_embed_service`]. Use when the caller needs
    /// control over the embed service configuration (e.g. background
    /// mode with progress callbacks) but the Index must be opened first
    /// to run migrations that create the `chunks` table.
    pub fn open_without_embed(
        root: impl AsRef<Path>,
    ) -> Result<Self, VaultError> {
        let root = root.as_ref().canonicalize().map_err(VaultError::Io)?;
        if !root.is_dir() {
            return Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault root not found: {}", root.display()),
            )));
        }

        let meta_dir = root.join(".tektite");
        std::fs::create_dir_all(&meta_dir)?;

        let db_path = meta_dir.join("index.db");
        let index = Index::open(&db_path).ok();

        Ok(Self {
            root,
            write_tokens: watcher::WriteTokenSet::new(),
            index,
            embed: None,
        })
    }

    /// Attaches an [`EmbedService`] after the vault (and its Index) have
    /// been opened. Call this before [`scan_and_index`](Self::scan_and_index).
    pub fn set_embed_service(&mut self, svc: EmbedService) {
        self.embed = Some(svc);
    }

    /// Returns the path to the index database.
    pub fn db_path(&self) -> PathBuf {
        self.root.join(".tektite").join("index.db")
    }

    // -----------------------------------------------------------------------
    // File I/O
    // -----------------------------------------------------------------------

    /// Resolves a vault-relative path to an absolute path inside the vault.
    pub fn absolute_path(&self, rel_path: &str) -> Result<PathBuf, VaultError> {
        self.abs(rel_path)
    }

    /// Reads a file. `rel_path` is vault-relative.
    pub fn read_file(&self, rel_path: &str) -> Result<String, VaultError> {
        let abs = self.abs(rel_path)?;
        std::fs::read_to_string(abs).map_err(VaultError::Io)
    }

    /// Writes UTF-8 content to a file. Records the path in the write-token
    /// set so the watcher ignores the resulting inotify event.
    pub fn write_file(&self, rel_path: &str, content: &str) -> Result<(), VaultError> {
        let abs = self.abs(rel_path)?;
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.write_tokens.insert(abs.clone());
        std::fs::write(&abs, content).map_err(VaultError::Io)
    }

    // -----------------------------------------------------------------------
    // Tree operations
    // -----------------------------------------------------------------------

    /// Returns the full vault directory tree as a nested [`VaultTreeEntry`].
    pub fn get_tree(&self) -> Result<Vec<VaultTreeEntry>, VaultError> {
        scan::build_tree(&self.root)
    }

    /// Creates a new markdown file, optionally seeding it with `initial_content`.
    /// Parent directories are created as needed. The write is registered in the
    /// write-token set.
    pub fn create_file(
        &self,
        rel_path: &str,
        initial_content: Option<&str>,
    ) -> Result<(), VaultError> {
        let abs = self.abs(rel_path)?;
        if abs.exists() {
            return Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("file already exists: {}", abs.display()),
            )));
        }
        if let Some(parent) = abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.write_tokens.insert(abs.clone());
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&abs)
            .map_err(VaultError::Io)?;
        if let Some(content) = initial_content {
            use std::io::Write;
            file.write_all(content.as_bytes()).map_err(VaultError::Io)?;
        }
        Ok(())
    }

    /// Creates a directory (and all intermediate directories).
    pub fn create_folder(&self, rel_path: &str) -> Result<(), VaultError> {
        let abs = self.abs(rel_path)?;
        if abs.exists() {
            return Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("folder already exists: {}", abs.display()),
            )));
        }
        std::fs::create_dir_all(abs).map_err(VaultError::Io)
    }

    /// Removes a file or directory (recursively). The path must be inside the
    /// vault root.
    pub fn delete(&self, rel_path: &str) -> Result<(), VaultError> {
        let abs = self.abs(rel_path)?;
        if abs.is_dir() {
            std::fs::remove_dir_all(abs).map_err(VaultError::Io)
        } else {
            std::fs::remove_file(abs).map_err(VaultError::Io)
        }
    }

    // -----------------------------------------------------------------------
    // Initial vault scan
    // -----------------------------------------------------------------------

    /// Walks the vault and upserts every markdown file into the index.
    ///
    /// Called once on vault open to populate the index from disk. Hidden
    /// directories (names starting with `.`) are skipped — this excludes
    /// `.tektite/`, `.git/`, and similar metadata directories.
    ///
    /// Files whose `mtime` matches the stored value are skipped so re-opening
    /// a large vault is fast after the first scan.
    pub fn scan_and_index(&mut self) -> Result<(), VaultError> {
        let index = match self.index.as_mut() {
            Some(i) => i,
            None => return Ok(()),
        };

        for entry in WalkDir::new(&self.root)
            .into_iter()
            .filter_entry(|e| {
                // Skip hidden directories (e.g. .tektite, .git).
                // Always descend into the root itself.
                if e.depth() == 0 {
                    return true;
                }
                !e.file_name().to_str().is_some_and(|n| n.starts_with('.'))
            })
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && is_markdown(e.path()))
        {
            let abs = entry.path();
            let rel = abs
                .strip_prefix(&self.root)
                .map_err(|_| VaultError::OutsideRoot)?
                .to_string_lossy()
                .replace('\\', "/");

            let mtime = file_mtime(abs);

            // Skip if already indexed with the same mtime — unless the
            // embed service is attached and the file has no chunks yet.
            // That case covers vaults opened before the semantic index
            // existed: the file is in the index but was never chunked,
            // so a naive mtime skip would permanently starve the embed
            // backlog.
            let already_current = index
                .get_mtime(&rel)
                .is_ok_and(|stored| stored == Some(mtime));
            if already_current {
                let needs_embed_backfill = match self.embed.as_ref() {
                    Some(embed) => match index.id_for_path(&rel) {
                        Ok(Some(file_id)) => embed
                            .has_chunks_for_file(&file_id)
                            .map(|has| !has)
                            .unwrap_or(false),
                        _ => false,
                    },
                    None => false,
                };
                if !needs_embed_backfill {
                    continue;
                }
            }

            let content = match std::fs::read_to_string(abs) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("scan_and_index: skipping {:?}: {e}", abs);
                    continue;
                }
            };
            let parsed = tektite_parser::parse(&content);
            let file_id = index.upsert(&rel, mtime, &parsed)?;

            if let Some(embed) = self.embed.as_ref() {
                let title = note_title(&rel, &parsed);
                // Vault-open backlog uses Normal priority. In background
                // mode this is non-blocking — the queue worker will process
                // it later.
                if let Err(e) = embed.reindex_file(&file_id, &title, &parsed) {
                    tracing::warn!("embed reindex failed for {rel}: {e}");
                }
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Incremental index updates
    // -----------------------------------------------------------------------

    /// Parses the file at `abs_path` and upserts it into the index.
    ///
    /// Silently no-ops if:
    /// - the path is not a `.md` file,
    /// - the path is outside the vault root, or
    /// - no index is open.
    pub fn reindex_file(&mut self, abs_path: &Path) -> Result<(), VaultError> {
        if !is_markdown(abs_path) {
            return Ok(());
        }
        let index = match self.index.as_mut() {
            Some(i) => i,
            None => return Ok(()),
        };
        let rel = abs_path
            .strip_prefix(&self.root)
            .map_err(|_| VaultError::OutsideRoot)?
            .to_string_lossy()
            .replace('\\', "/");
        let content = std::fs::read_to_string(abs_path).map_err(VaultError::Io)?;
        let mtime = file_mtime(abs_path);
        let parsed = tektite_parser::parse(&content);
        let file_id = index.upsert(&rel, mtime, &parsed)?;

        if let Some(embed) = self.embed.as_ref() {
            let title = note_title(&rel, &parsed);
            // Live edits from the watcher get High priority so they jump
            // ahead of any vault-open backlog.
            if let Err(e) = embed.reindex_file_with_priority(
                &file_id,
                &title,
                &parsed,
                Priority::High,
            ) {
                tracing::warn!("embed reindex failed for {rel}: {e}");
            }
        }
        Ok(())
    }

    /// Removes a file from the index by absolute path.
    ///
    /// Silently no-ops if the path is not a `.md` file, is outside the vault
    /// root, or no index is open.
    pub fn remove_from_index(&mut self, abs_path: &Path) -> Result<(), VaultError> {
        if !is_markdown(abs_path) {
            return Ok(());
        }
        let rel = abs_path
            .strip_prefix(&self.root)
            .map_err(|_| VaultError::OutsideRoot)?
            .to_string_lossy()
            .replace('\\', "/");

        // Resolve the file id *before* removing from the index so we can
        // purge the embed cache by id. The DB rows in `chunks` cascade
        // automatically when the `files` row is dropped.
        let file_id = self
            .index
            .as_ref()
            .and_then(|idx| idx.id_for_path(&rel).ok().flatten());

        let index = match self.index.as_mut() {
            Some(i) => i,
            None => return Ok(()),
        };
        index.remove_file(&rel)?;

        if let (Some(id), Some(embed)) = (file_id, self.embed.as_ref()) {
            embed.forget_file(&id);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Rename: preview / apply
    // -----------------------------------------------------------------------

    /// Computes all wiki-link rewrites required to rename `old_rel` to
    /// `new_rel` without performing any I/O. Returns a [`RenamePlan`] that
    /// can be presented to the user before they confirm.
    ///
    /// Supports both file renames (`.md` paths) and directory renames.
    pub fn plan_rename(&self, old_rel: &str, new_rel: &str) -> Result<RenamePlan, VaultError> {
        self.validate_rename_paths(old_rel, new_rel)?;
        let index = self.index.as_ref().ok_or(VaultError::NotOpen)?;
        index
            .plan_rename(old_rel, new_rel)
            .map_err(VaultError::Index)
    }

    /// Executes a [`RenamePlan`]: rewrites affected files, renames on disk,
    /// and updates the SQLite index.
    pub fn apply_rename(&mut self, plan: &RenamePlan) -> Result<RenameOutcome, VaultError> {
        // Require an index.
        if self.index.is_none() {
            return Err(VaultError::NotOpen);
        }

        self.validate_rename_paths(&plan.old_path, &plan.new_path)?;

        let fresh_plan = self.plan_rename(&plan.old_path, &plan.new_path)?;
        if fresh_plan.edits != plan.edits {
            return Err(VaultError::InvalidPath(
                "Rename preview is stale. Preview the rename again before applying it.".to_string(),
            ));
        }

        let is_dir_rename = !plan.old_path.ends_with(".md");

        if is_dir_rename {
            self.apply_dir_rename(plan)
        } else {
            self.apply_file_rename(plan)
        }
    }

    // -----------------------------------------------------------------------
    // Single-file rename
    // -----------------------------------------------------------------------

    fn apply_file_rename(&mut self, plan: &RenamePlan) -> Result<RenameOutcome, VaultError> {
        // 1. Apply link text rewrites and write modified files.
        let mut rewrites: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for path in plan.edits.iter().map(|edit| edit.file_path.as_str()) {
            if rewrites.contains_key(path) {
                continue;
            }

            let current = self.read_file(path)?;
            rewrites.insert(
                path.to_string(),
                rewrite_content(&current, path, &plan.edits),
            );
        }
        for (path, content) in &rewrites {
            self.write_file(path, content)?;
        }

        // 2. Move the file on disk.
        let old_abs = self.abs(&plan.old_path)?;
        let new_abs = self.abs(&plan.new_path)?;
        if let Some(parent) = new_abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.write_tokens.insert(old_abs.clone());
        self.write_tokens.insert(new_abs.clone());
        std::fs::rename(&old_abs, &new_abs)?;

        // 3. Build the re-index list.
        let renamed_content = self.read_file(&plan.new_path)?;
        let renamed_mtime = file_mtime(&new_abs);
        let mut modified: Vec<(String, i64, String)> =
            vec![(plan.new_path.clone(), renamed_mtime, renamed_content)];
        for (path, content) in &rewrites {
            if *path != plan.new_path {
                let abs = self.abs(path)?;
                modified.push((path.clone(), file_mtime(&abs), content.clone()));
            }
        }

        // 4. Update the index.
        self.index.as_mut().unwrap().apply_rename_index(
            &plan.old_path,
            &plan.new_path,
            &modified,
        )?;

        let mut changed_paths: Vec<String> = rewrites.keys().cloned().collect();
        if !changed_paths.iter().any(|path| path == &plan.new_path) {
            changed_paths.push(plan.new_path.clone());
        }
        changed_paths.sort();

        Ok(RenameOutcome {
            old_path: plan.old_path.clone(),
            new_path: plan.new_path.clone(),
            changed_paths,
        })
    }

    // -----------------------------------------------------------------------
    // Directory rename
    // -----------------------------------------------------------------------

    fn apply_dir_rename(&mut self, plan: &RenamePlan) -> Result<RenameOutcome, VaultError> {
        let old_abs = self.abs(&plan.old_path)?;
        let new_abs = self.abs(&plan.new_path)?;

        // Enumerate every markdown file inside the old directory.
        let old_prefix = format!("{}/", plan.old_path.trim_end_matches('/'));
        let new_prefix = format!("{}/", plan.new_path.trim_end_matches('/'));

        let file_entries: Vec<(String, String)> = WalkDir::new(&old_abs)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_type().is_file()
                    && e.path()
                        .extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("md"))
            })
            .filter_map(|e| {
                let rel = e
                    .path()
                    .strip_prefix(&self.root)
                    .ok()?
                    .to_string_lossy()
                    .replace('\\', "/");
                let suffix = rel.strip_prefix(&old_prefix)?.to_string();
                Some((rel, format!("{}{}", new_prefix, suffix)))
            })
            .collect();

        // Apply link rewrites.
        let mut rewrites: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for path in plan.edits.iter().map(|edit| edit.file_path.as_str()) {
            if rewrites.contains_key(path) {
                continue;
            }

            let current = self.read_file(path)?;
            rewrites.insert(
                path.to_string(),
                rewrite_content(&current, path, &plan.edits),
            );
        }
        for (path, content) in &rewrites {
            self.write_file(path, content)?;
        }

        // Move the directory on disk.
        if let Some(parent) = new_abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.write_tokens.insert(old_abs.clone());
        self.write_tokens.insert(new_abs.clone());
        std::fs::rename(&old_abs, &new_abs)?;

        // Build re-index list: all files in the new location + rewritten files.
        let mut modified: Vec<(String, i64, String)> = Vec::new();
        for (_, new_path) in &file_entries {
            let content = self.read_file(new_path)?;
            let abs = self.abs(new_path)?;
            modified.push((new_path.clone(), file_mtime(&abs), content));
        }
        for (path, content) in &rewrites {
            // Skip files already included above (files in the renamed dir).
            let already_included = file_entries.iter().any(|(_, np)| np == path);
            if !already_included {
                let abs = self.abs(path)?;
                modified.push((path.clone(), file_mtime(&abs), content.clone()));
            }
        }

        // Update the index.
        self.index.as_mut().unwrap().apply_dir_rename_index(
            &plan.old_path,
            &plan.new_path,
            &file_entries,
            &modified,
        )?;

        let mut changed_paths: Vec<String> = rewrites.keys().cloned().collect();
        for (_, new_path) in &file_entries {
            if !changed_paths.iter().any(|path| path == new_path) {
                changed_paths.push(new_path.clone());
            }
        }
        changed_paths.sort();

        Ok(RenameOutcome {
            old_path: plan.old_path.clone(),
            new_path: plan.new_path.clone(),
            changed_paths,
        })
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Resolves a vault-relative path to an absolute path, rejecting attempts
    /// to escape the vault root (path traversal).
    fn abs(&self, rel_path: &str) -> Result<PathBuf, VaultError> {
        let rel = normalize_rel_path(rel_path)?;
        let candidate = self.root.join(&rel);
        // Canonicalise the parent directory (file may not exist yet) to
        // resolve `..` components safely.
        let parent = candidate.parent().unwrap_or(&candidate);
        let resolved_parent = if parent.exists() {
            parent.canonicalize().map_err(VaultError::Io)?
        } else {
            // Parent doesn't exist yet (creating new file/dir) — check the
            // closest existing ancestor instead.
            let existing = existing_ancestor(parent);
            let canon = existing.canonicalize().map_err(VaultError::Io)?;
            // Reconstruct: canonical_ancestor + remaining relative suffix
            let suffix = parent.strip_prefix(existing).unwrap_or(parent);
            canon.join(suffix)
        };
        if !resolved_parent.starts_with(&self.root) {
            return Err(VaultError::OutsideRoot);
        }
        Ok(self.root.join(rel))
    }

    fn validate_rename_paths(&self, old_rel: &str, new_rel: &str) -> Result<(), VaultError> {
        let old_abs = self.abs(old_rel)?;
        let new_abs = self.abs(new_rel)?;

        if old_rel == new_rel {
            return Err(VaultError::InvalidPath(
                "New name must be different from the current name".into(),
            ));
        }

        if !old_abs.exists() {
            return Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("path does not exist: {old_rel}"),
            )));
        }

        if new_abs.exists() {
            return Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("destination already exists: {new_rel}"),
            )));
        }

        if old_abs.is_file() != new_rel.ends_with(".md") {
            return Err(VaultError::InvalidPath(
                "Markdown note renames must keep the .md extension".into(),
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Module-level helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `path` has a `.md` extension (case-insensitive).
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

/// Best-effort display title for a note. Prefers `frontmatter.title`, else
/// the filename stem. Used by the embedder (Phase 2 will prefix chunks
/// with this).
fn note_title(rel_path: &str, note: &tektite_parser::ParsedNote) -> String {
    if let Some(title) = note.frontmatter.get("title").and_then(|v| v.as_str()) {
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    Path::new(rel_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(rel_path)
        .to_string()
}

fn normalize_rel_path(rel_path: &str) -> Result<String, VaultError> {
    let rel = rel_path.trim().replace('\\', "/");
    if rel.is_empty() {
        return Err(VaultError::InvalidPath("path cannot be empty".into()));
    }

    let mut parts = Vec::new();
    for part in rel.split('/') {
        if part.is_empty() {
            return Err(VaultError::InvalidPath(
                "path cannot contain empty segments".into(),
            ));
        }
        if part == "." || part == ".." {
            return Err(VaultError::InvalidPath(format!(
                "path cannot contain navigation segments: {rel_path}"
            )));
        }
        parts.push(part);
    }

    Ok(parts.join("/"))
}

/// Returns the mtime of a file in seconds since UNIX_EPOCH, or 0 on failure.
fn file_mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Walks up the path until it finds an existing directory.
fn existing_ancestor(path: &Path) -> &Path {
    let mut p = path;
    loop {
        if p.exists() {
            return p;
        }
        match p.parent() {
            Some(parent) => p = parent,
            None => return p,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RenamePlan, Vault, VaultError};

    #[test]
    fn read_and_write_file_round_trip_inside_vault() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        vault
            .write_file("notes/today.md", "hello from tektite")
            .expect("write inside vault");

        let content = vault
            .read_file("notes/today.md")
            .expect("read inside vault");
        assert_eq!(content, "hello from tektite");
    }

    #[test]
    fn write_file_rejects_paths_outside_vault_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        let err = vault
            .write_file("../escape.md", "nope")
            .expect_err("outside-root write should fail");

        assert!(matches!(err, VaultError::InvalidPath(_)));
    }

    #[test]
    fn create_file_rejects_invalid_relative_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        let err = vault
            .create_file("notes//today.md", None)
            .expect_err("invalid path should fail");

        assert!(matches!(err, VaultError::InvalidPath(_)));
    }

    #[test]
    fn create_file_rejects_duplicate_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        vault
            .create_file("notes/today.md", None)
            .expect("first create succeeds");

        let err = vault
            .create_file("notes/today.md", None)
            .expect_err("duplicate create should fail");

        assert!(
            matches!(err, VaultError::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists)
        );
    }

    #[test]
    fn plan_rename_rejects_existing_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        std::fs::write(dir.path().join("old.md"), "# Old\n").unwrap();
        std::fs::write(dir.path().join("new.md"), "# New\n").unwrap();
        vault.scan_and_index().expect("scan vault");

        let err = vault
            .plan_rename("old.md", "new.md")
            .expect_err("existing destination should fail");

        assert!(
            matches!(err, VaultError::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists)
        );
    }

    #[test]
    fn apply_rename_rejects_stale_preview() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        std::fs::write(dir.path().join("target.md"), "# Target\n").unwrap();
        std::fs::write(dir.path().join("source.md"), "[[target]]\n").unwrap();
        vault.scan_and_index().expect("scan vault");

        let stale_plan = RenamePlan {
            old_path: "target.md".to_string(),
            new_path: "renamed.md".to_string(),
            edits: vec![],
        };

        let err = vault
            .apply_rename(&stale_plan)
            .expect_err("stale preview should fail");

        assert!(matches!(err, VaultError::InvalidPath(message) if message.contains("stale")));
        assert!(dir.path().join("target.md").exists());
        assert!(!dir.path().join("renamed.md").exists());
    }

    #[test]
    fn delete_removes_file_and_index_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        std::fs::write(dir.path().join("g.md"), "# Gone\n").unwrap();
        vault.scan_and_index().expect("scan vault");

        // Confirm it's indexed.
        let id = vault
            .index
            .as_ref()
            .unwrap()
            .id_for_path("g.md")
            .expect("index reachable");
        assert!(id.is_some(), "file must be indexed before delete");

        let abs = vault.absolute_path("g.md").expect("resolve path");
        vault.remove_from_index(&abs).expect("remove from index");
        vault.delete("g.md").expect("delete file");

        // File gone from disk.
        assert!(!dir.path().join("g.md").exists(), "file must be removed");

        // Entry gone from index.
        let still_there = vault
            .index
            .as_ref()
            .unwrap()
            .id_for_path("g.md")
            .expect("index reachable");
        assert!(still_there.is_none(), "index entry must be cleared after delete");
    }

    // -----------------------------------------------------------------------
    // Phase 1 — semantic index integration
    // -----------------------------------------------------------------------

    #[test]
    fn scan_populates_embed_cache_and_search_returns_hits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        std::fs::write(dir.path().join("a.md"), "# Alpha\nfirst body\n").unwrap();
        std::fs::write(dir.path().join("b.md"), "# Bravo\nsecond body\n").unwrap();
        vault.scan_and_index().expect("scan vault");

        let embed = vault.embed.as_ref().expect("embed service available");
        // Two notes × one chunk each → two cached vectors.
        assert_eq!(embed.cache().len(), 2);

        // Searching for the exact text of a chunk must surface that chunk
        // first (FakeEmbedder is deterministic on identical input).
        let hits = embed
            .search_semantic("# Alpha\nfirst body", 5)
            .expect("search ok");
        assert!(!hits.is_empty(), "expected at least one hit");
        assert_eq!(hits[0].file_path, "a.md");
        assert_eq!(hits[0].heading_path.as_deref(), Some("Alpha"));
    }

    #[test]
    fn unchanged_save_reuses_chunk_ids_and_skips_re_embedding() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        let path = dir.path().join("note.md");
        std::fs::write(&path, "# Same\nbody\n").unwrap();
        vault.scan_and_index().expect("scan vault");

        let file_id = vault
            .index
            .as_ref()
            .unwrap()
            .id_for_path("note.md")
            .unwrap()
            .expect("file indexed");
        let before = vault
            .embed
            .as_ref()
            .unwrap()
            .store()
            .chunks_for_file(&file_id)
            .unwrap();
        assert_eq!(before.len(), 1);
        let original_id = before[0].id.clone();

        // Re-trigger reindex with identical content. Same content_hash →
        // existing chunk row's id is preserved.
        vault.reindex_file(&path).expect("reindex");

        let after = vault
            .embed
            .as_ref()
            .unwrap()
            .store()
            .chunks_for_file(&file_id)
            .unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, original_id, "chunk id must be reused");
    }

    #[test]
    fn delete_cascades_chunks_and_clears_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut vault = Vault::open(dir.path(), Box::new(tektite_embed::FakeEmbedder::new())).expect("open vault");

        std::fs::write(dir.path().join("a.md"), "# A\nbody\n").unwrap();
        vault.scan_and_index().expect("scan vault");

        let file_id = vault
            .index
            .as_ref()
            .unwrap()
            .id_for_path("a.md")
            .unwrap()
            .expect("indexed");
        assert_eq!(vault.embed.as_ref().unwrap().cache().len(), 1);
        assert_eq!(
            vault
                .embed
                .as_ref()
                .unwrap()
                .store()
                .chunks_for_file(&file_id)
                .unwrap()
                .len(),
            1
        );

        let abs = vault.absolute_path("a.md").unwrap();
        vault.remove_from_index(&abs).expect("remove");

        // Cache purged + cascade dropped the row.
        assert_eq!(vault.embed.as_ref().unwrap().cache().len(), 0);
        assert!(vault
            .embed
            .as_ref()
            .unwrap()
            .store()
            .chunks_for_file(&file_id)
            .unwrap()
            .is_empty());
    }
}
