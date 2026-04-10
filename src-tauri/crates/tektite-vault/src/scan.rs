//! Vault tree scanning via `walkdir`.
//!
//! Builds a nested [`VaultTreeEntry`] tree from the vault root directory.
//! Hidden entries (names starting with `.`) are skipped — this excludes
//! `.tektite/`, `.git/`, and similar metadata directories.

use std::path::Path;

use walkdir::WalkDir;

use crate::{VaultError, VaultTreeEntry};

/// Builds the complete vault tree starting from `root`.
///
/// Returns a flat list of top-level entries; each directory entry contains
/// its children recursively.
///
/// Entries are sorted: directories first (alphabetically), then files
/// (alphabetically). Hidden entries (`.` prefix) are excluded.
pub fn build_tree(root: &Path) -> Result<Vec<VaultTreeEntry>, VaultError> {
    // Collect all non-hidden entries under root in a flat list first.
    // depth=1 gives us the immediate children; we recurse by building the
    // tree level by level.
    build_level(root, root)
}

/// Recursively builds entries for all immediate children of `dir`.
fn build_level(root: &Path, dir: &Path) -> Result<Vec<VaultTreeEntry>, VaultError> {
    let mut dirs: Vec<VaultTreeEntry> = Vec::new();
    let mut files: Vec<VaultTreeEntry> = Vec::new();

    // WalkDir with min_depth=1 max_depth=1 gives immediate children only.
    for entry in WalkDir::new(dir)
        .min_depth(1)
        .max_depth(1)
        .sort_by_file_name()
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip hidden entries.
        if name.starts_with('.') {
            continue;
        }

        let abs_path = entry.path();
        let rel_path = abs_path
            .strip_prefix(root)
            .unwrap_or(abs_path)
            .to_string_lossy()
            // Use forward slashes on all platforms for consistency.
            .replace('\\', "/");

        if entry.file_type().is_dir() {
            let children = build_level(root, abs_path)?;
            dirs.push(VaultTreeEntry {
                path: rel_path,
                name,
                is_dir: true,
                children,
            });
        } else {
            files.push(VaultTreeEntry {
                path: rel_path,
                name,
                is_dir: false,
                children: vec![],
            });
        }
    }

    // Directories first, then files — both groups already sorted by name
    // because WalkDir uses sort_by_file_name.
    dirs.extend(files);
    Ok(dirs)
}
