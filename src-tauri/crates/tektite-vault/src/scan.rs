//! Vault tree scanning via `walkdir`.
//!
//! Builds a nested [`VaultTreeEntry`] tree from the vault root directory.
//! Hidden entries (names starting with `.`) are skipped — this excludes
//! `.tektite/`, `.git/`, and similar metadata directories.

use std::cmp::Ordering;
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
        .sort_by(|a, b| compare_entry_names(a.file_name(), b.file_name()))
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
                is_markdown: false,
                children,
            });
        } else {
            files.push(VaultTreeEntry {
                path: rel_path,
                name,
                is_dir: false,
                is_markdown: abs_path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("md")),
                children: vec![],
            });
        }
    }

    // Directories first, then files — both groups already sorted by name
    // because WalkDir uses sort_by_file_name.
    dirs.extend(files);
    Ok(dirs)
}

fn compare_entry_names(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> Ordering {
    let a = a.to_string_lossy();
    let b = b.to_string_lossy();
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| a.cmp(&b))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::build_tree;

    #[test]
    fn build_tree_skips_hidden_entries_and_marks_markdown_files() {
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("docs")).expect("create docs dir");
        std::fs::create_dir_all(dir.path().join(".git")).expect("create hidden dir");
        std::fs::write(dir.path().join("docs/Note.md"), "# note").expect("write md");
        std::fs::write(dir.path().join("docs/image.png"), "png").expect("write png");
        std::fs::write(dir.path().join(".env"), "hidden").expect("write hidden file");

        let tree = build_tree(dir.path()).expect("build tree");

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].path, "docs");
        assert!(tree[0].is_dir);
        assert!(!tree[0].is_markdown);
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].name, "image.png");
        assert!(!tree[0].children[0].is_markdown);
        assert_eq!(tree[0].children[1].name, "Note.md");
        assert!(tree[0].children[1].is_markdown);
    }

    #[test]
    fn build_tree_sorts_case_insensitively_with_directories_first() {
        let dir = tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("beta")).expect("create beta dir");
        std::fs::create_dir_all(dir.path().join("Zoo")).expect("create zoo dir");
        std::fs::write(dir.path().join("alpha.md"), "# alpha").expect("write alpha");
        std::fs::write(dir.path().join("Notes.txt"), "notes").expect("write notes");

        let tree = build_tree(dir.path()).expect("build tree");
        let names: Vec<_> = tree.iter().map(|entry| entry.name.as_str()).collect();

        assert_eq!(names, vec!["beta", "Zoo", "alpha.md", "Notes.txt"]);
    }
}
