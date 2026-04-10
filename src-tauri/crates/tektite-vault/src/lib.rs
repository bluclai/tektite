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

use tektite_index::Index;

pub use tektite_index::{IndexError, RenameEdit, RenamePlan};

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
}

impl Vault {
    /// Opens a vault at `root`. Returns an error if the path is not a directory.
    ///
    /// Creates the `.tektite/` metadata directory if it doesn't exist and
    /// opens (or creates) the index database.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, VaultError> {
        let root = root.as_ref().canonicalize().map_err(VaultError::Io)?;
        if !root.is_dir() {
            return Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("vault root not found: {}", root.display()),
            )));
        }

        // Ensure the .tektite metadata directory exists.
        let meta_dir = root.join(".tektite");
        std::fs::create_dir_all(&meta_dir)?;

        // Open the index database.
        let db_path = meta_dir.join("index.db");
        let index = Index::open(&db_path).ok(); // non-fatal — vault still opens without index

        Ok(Self {
            root,
            write_tokens: watcher::WriteTokenSet::new(),
            index,
        })
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

    /// Creates a new empty markdown file. Parent directories are created as needed.
    /// The write is registered in the write-token set.
    pub fn create_file(&self, rel_path: &str) -> Result<(), VaultError> {
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
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&abs)
            .map(|_| ())
            .map_err(VaultError::Io)
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

            // Skip if already indexed with the same mtime.
            let already_current = index
                .get_mtime(&rel)
                .is_ok_and(|stored| stored == Some(mtime));
            if already_current {
                continue;
            }

            let content = match std::fs::read_to_string(abs) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("scan_and_index: skipping {:?}: {e}", abs);
                    continue;
                }
            };
            let parsed = tektite_parser::parse(&content);
            index.upsert(&rel, mtime, &parsed)?;
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
        index.upsert(&rel, mtime, &parsed)?;
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
        let index = match self.index.as_mut() {
            Some(i) => i,
            None => return Ok(()),
        };
        let rel = abs_path
            .strip_prefix(&self.root)
            .map_err(|_| VaultError::OutsideRoot)?
            .to_string_lossy()
            .replace('\\', "/");
        index.remove_file(&rel)?;
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
        let index = self.index.as_ref().ok_or(VaultError::NotOpen)?;
        index
            .plan_rename(old_rel, new_rel)
            .map_err(VaultError::Index)
    }

    /// Executes a [`RenamePlan`]: rewrites affected files, renames on disk,
    /// and updates the SQLite index.
    pub fn apply_rename(&mut self, plan: &RenamePlan) -> Result<(), VaultError> {
        // Require an index.
        if self.index.is_none() {
            return Err(VaultError::NotOpen);
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

    fn apply_file_rename(&mut self, plan: &RenamePlan) -> Result<(), VaultError> {
        // 1. Apply link text rewrites and write modified files.
        let mut rewrites: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for edit in &plan.edits {
            let current = rewrites
                .get(&edit.file_path)
                .cloned()
                .unwrap_or_else(|| self.read_file(&edit.file_path).unwrap_or_default());
            rewrites.insert(
                edit.file_path.clone(),
                current.replace(&edit.before, &edit.after),
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
        let renamed_content = self.read_file(&plan.new_path).unwrap_or_default();
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

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Directory rename
    // -----------------------------------------------------------------------

    fn apply_dir_rename(&mut self, plan: &RenamePlan) -> Result<(), VaultError> {
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
        for edit in &plan.edits {
            let current = rewrites
                .get(&edit.file_path)
                .cloned()
                .unwrap_or_else(|| self.read_file(&edit.file_path).unwrap_or_default());
            rewrites.insert(
                edit.file_path.clone(),
                current.replace(&edit.before, &edit.after),
            );
        }
        for (path, content) in &rewrites {
            self.write_file(path, content)?;
        }

        // Move the directory on disk.
        if let Some(parent) = new_abs.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&old_abs, &new_abs)?;

        // Build re-index list: all files in the new location + rewritten files.
        let mut modified: Vec<(String, i64, String)> = Vec::new();
        for (_, new_path) in &file_entries {
            let content = self.read_file(new_path).unwrap_or_default();
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

        Ok(())
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
}

// ---------------------------------------------------------------------------
// Module-level helpers
// ---------------------------------------------------------------------------

/// Returns `true` if `path` has a `.md` extension (case-insensitive).
fn is_markdown(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
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
    use super::{Vault, VaultError};

    #[test]
    fn read_and_write_file_round_trip_inside_vault() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path()).expect("open vault");

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
        let vault = Vault::open(dir.path()).expect("open vault");

        let err = vault
            .write_file("../escape.md", "nope")
            .expect_err("outside-root write should fail");

        assert!(matches!(err, VaultError::InvalidPath(_)));
    }

    #[test]
    fn create_file_rejects_invalid_relative_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path()).expect("open vault");

        let err = vault
            .create_file("notes//today.md")
            .expect_err("invalid path should fail");

        assert!(matches!(err, VaultError::InvalidPath(_)));
    }

    #[test]
    fn create_file_rejects_duplicate_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let vault = Vault::open(dir.path()).expect("open vault");

        vault
            .create_file("notes/today.md")
            .expect("first create succeeds");

        let err = vault
            .create_file("notes/today.md")
            .expect_err("duplicate create should fail");

        assert!(
            matches!(err, VaultError::Io(io) if io.kind() == std::io::ErrorKind::AlreadyExists)
        );
    }
}
