//! Integration tests for `tektite-index`.
//!
//! These tests verify the full pipeline:
//!   parse (tektite-parser) → upsert → query → resolve
//!
//! Each test opens a fresh in-memory index, so there is no shared state.

use tektite_index::{GraphFilters, Index, LinkResolution, UnresolvedTargetKind, NODE_CAP};
use tektite_parser::parse;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse markdown and upsert into the index. Returns the NoteId.
fn ingest(idx: &mut Index, path: &str, content: &str) -> String {
    let note = parse(content);
    idx.upsert(path, 0, &note).expect("upsert failed")
}

/// Parse markdown and upsert with an explicit mtime. Returns the NoteId.
fn ingest_at(idx: &mut Index, path: &str, mtime: i64, content: &str) -> String {
    let note = parse(content);
    idx.upsert(path, mtime, &note).expect("upsert failed")
}

// ---------------------------------------------------------------------------
// Schema & identity
// ---------------------------------------------------------------------------

#[test]
fn files_table_uses_uuid_v4_primary_key() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "notes/hello.md", "# Hello\n");
    // UUID v4 format: xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx
    assert_eq!(id.len(), 36);
    let parts: Vec<&str> = id.split('-').collect();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[2].chars().next(), Some('4')); // version nibble
}

#[test]
fn path_is_unique_indexed_column() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/a.md", "# A\n");
    // Upserting the same path again must succeed (update, not insert).
    ingest(&mut idx, "notes/a.md", "# A updated\n");
    let files = idx.all_files().unwrap();
    assert_eq!(files.len(), 1);
}

#[test]
fn id_is_preserved_on_re_upsert() {
    let mut idx = Index::open_in_memory().unwrap();
    let id1 = ingest(&mut idx, "notes/stable.md", "# First\n");
    let id2 = ingest(&mut idx, "notes/stable.md", "# Second\n");
    assert_eq!(
        id1, id2,
        "ID must not change across upserts of the same path"
    );
}

#[test]
fn new_uuid_minted_for_new_file() {
    let mut idx = Index::open_in_memory().unwrap();
    let id1 = ingest(&mut idx, "notes/a.md", "# A\n");
    let id2 = ingest(&mut idx, "notes/b.md", "# B\n");
    assert_ne!(id1, id2);
}

// ---------------------------------------------------------------------------
// All child FKs reference files(id)
// ---------------------------------------------------------------------------

#[test]
fn child_records_reference_file_id_not_path() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/rich.md",
        "---\ntitle: Rich\naliases:\n  - Alias One\n---\n# Heading\n\n[[other]] #tag\n\n- [ ] Task\n",
    );

    // Headings reference the file by ID.
    let headings = idx.get_headings(&id).unwrap();
    assert_eq!(headings.len(), 1);
    assert_eq!(headings[0].file_id, id);

    // Tags reference the file by ID.
    let tags = idx.get_tags(&id).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].file_id, id);

    // Tasks reference the file by ID.
    let tasks = idx.get_tasks(&id).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].file_id, id);

    // Links reference the source file by ID.
    let links = idx.get_links(&id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].source_id, id);
}

// ---------------------------------------------------------------------------
// Aliases table
// ---------------------------------------------------------------------------

#[test]
fn aliases_table_populated_during_upsert() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/aliased.md",
        "---\naliases:\n  - Short Name\n  - Alt Title\n---\nBody.\n",
    );
    let aliases = idx.get_aliases(&id).unwrap();
    assert_eq!(aliases.len(), 2);
    assert!(aliases.contains(&"Short Name".to_string()));
    assert!(aliases.contains(&"Alt Title".to_string()));
}

#[test]
fn aliases_table_replaced_on_re_upsert() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/a.md",
        "---\naliases:\n  - Old Alias\n---\n",
    );
    // Re-upsert with different aliases.
    ingest(
        &mut idx,
        "notes/a.md",
        "---\naliases:\n  - New Alias\n---\n",
    );
    let aliases = idx.get_aliases(&id).unwrap();
    assert_eq!(aliases.len(), 1);
    assert_eq!(aliases[0], "New Alias");
}

#[test]
fn files_by_alias_queries_aliases_table_case_insensitively() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/note.md",
        "---\naliases:\n  - My Note\n---\n",
    );
    // Exact case.
    let found = idx.files_by_alias("My Note").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, id);

    // All-caps.
    let found_upper = idx.files_by_alias("MY NOTE").unwrap();
    assert_eq!(found_upper.len(), 1);
    assert_eq!(found_upper[0].id, id);

    // All-lower.
    let found_lower = idx.files_by_alias("my note").unwrap();
    assert_eq!(found_lower.len(), 1);
    assert_eq!(found_lower[0].id, id);
}

#[test]
fn files_by_alias_returns_empty_when_no_match() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/note.md", "---\naliases:\n  - Known\n---\n");
    let found = idx.files_by_alias("Unknown").unwrap();
    assert!(found.is_empty());
}

// ---------------------------------------------------------------------------
// End-to-end: parse → upsert → query
// ---------------------------------------------------------------------------

#[test]
fn upsert_and_query_headings() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/doc.md",
        "# Title\n## Section One\n### Subsection\n",
    );
    let headings = idx.get_headings(&id).unwrap();
    assert_eq!(headings.len(), 3);
    assert_eq!(headings[0].level, 1);
    assert_eq!(headings[0].text, "Title");
    assert_eq!(headings[1].level, 2);
    assert_eq!(headings[1].text, "Section One");
    assert_eq!(headings[2].level, 3);
    assert_eq!(headings[2].text, "Subsection");
}

#[test]
fn upsert_and_query_links() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/source.md",
        "See [[target]] and [[folder/deep#heading|Display]].\n",
    );
    let links = idx.get_links(&id).unwrap();
    assert_eq!(links.len(), 2);

    let plain = links.iter().find(|l| l.target == "target").unwrap();
    assert_eq!(plain.fragment, None);
    assert_eq!(plain.alias, None);

    let rich = links.iter().find(|l| l.target == "folder/deep").unwrap();
    assert_eq!(rich.fragment.as_deref(), Some("heading"));
    assert_eq!(rich.alias.as_deref(), Some("Display"));
}

#[test]
fn upsert_and_query_tags() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "notes/tagged.md", "Hello #rust #testing world.\n");
    let tags = idx.get_tags(&id).unwrap();
    let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"rust"));
    assert!(names.contains(&"testing"));
}

#[test]
fn search_tags_returns_matching_rows_with_paths() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "notes/tagged.md", "Hello #rust #testing world.\n");

    let rows = idx.search_tags("rust", 10).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].file_id, id);
    assert_eq!(rows[0].file_path, "notes/tagged.md");
    assert_eq!(rows[0].name, "rust");
}

#[test]
fn upsert_and_query_tasks() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/tasks.md",
        "- [ ] Open task\n- [x] Done task\n",
    );
    let tasks = idx.get_tasks(&id).unwrap();
    assert_eq!(tasks.len(), 2);
    let open = tasks.iter().find(|t| t.text == "Open task").unwrap();
    assert!(!open.done);
    let done = tasks.iter().find(|t| t.text == "Done task").unwrap();
    assert!(done.done);
}

#[test]
fn upsert_and_query_frontmatter_aliases() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/fm.md",
        "---\ntitle: My Doc\naliases:\n  - Short\n  - Alt\n---\nBody.\n",
    );
    let aliases = idx.get_aliases(&id).unwrap();
    assert_eq!(aliases.len(), 2);
    assert!(aliases.contains(&"Short".to_string()));
    assert!(aliases.contains(&"Alt".to_string()));
}

// ---------------------------------------------------------------------------
// files_by_stem — case-insensitive, sub-directory and root
// ---------------------------------------------------------------------------

#[test]
fn files_by_stem_matches_sub_directory_file() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "folder/sub/note.md", "# Note\n");
    let found = idx.files_by_stem("note").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, id);
}

#[test]
fn files_by_stem_matches_root_level_file() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "root.md", "# Root\n");
    let found = idx.files_by_stem("root").unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, id);
}

#[test]
fn files_by_stem_is_case_insensitive() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "Notes/MyNote.md", "# My Note\n");
    let found_lower = idx.files_by_stem("mynote").unwrap();
    assert_eq!(found_lower.len(), 1);
    assert_eq!(found_lower[0].id, id);

    let found_upper = idx.files_by_stem("MYNOTE").unwrap();
    assert_eq!(found_upper.len(), 1);
    assert_eq!(found_upper[0].id, id);
}

#[test]
fn files_by_stem_returns_multiple_matches_for_same_stem() {
    let mut idx = Index::open_in_memory().unwrap();
    let id_a = ingest(&mut idx, "a/note.md", "# A\n");
    let id_b = ingest(&mut idx, "b/note.md", "# B\n");
    let found = idx.files_by_stem("note").unwrap();
    assert_eq!(found.len(), 2);
    let ids: Vec<&str> = found.iter().map(|f| f.id.as_str()).collect();
    assert!(ids.contains(&id_a.as_str()));
    assert!(ids.contains(&id_b.as_str()));
}

// ---------------------------------------------------------------------------
// remove_file
// ---------------------------------------------------------------------------

#[test]
fn remove_file_deletes_file_and_child_records() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/gone.md",
        "---\naliases:\n  - Gone\n---\n# Heading\n\n#tag\n",
    );
    idx.remove_file("notes/gone.md").unwrap();

    assert!(idx.id_for_path("notes/gone.md").unwrap().is_none());
    assert!(idx.get_headings(&id).unwrap().is_empty());
    assert!(idx.get_aliases(&id).unwrap().is_empty());
    assert!(idx.get_tags(&id).unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Link resolution end-to-end
// ---------------------------------------------------------------------------

#[test]
fn resolve_link_finds_file_by_stem() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "notes/target.md", "# Target\n");
    let result = idx.resolve_link("target", None).unwrap();
    assert_eq!(result, LinkResolution::Resolved(id));
}

#[test]
fn resolve_link_finds_file_by_alias() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(
        &mut idx,
        "notes/aliased.md",
        "---\naliases:\n  - My Alias\n---\n",
    );
    let result = idx.resolve_link("My Alias", None).unwrap();
    assert_eq!(result, LinkResolution::Resolved(id));
}

#[test]
fn resolve_link_is_unresolved_for_unknown_target() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/something.md", "# Something\n");
    let result = idx.resolve_link("nonexistent", None).unwrap();
    assert_eq!(result, LinkResolution::Unresolved);
}

#[test]
fn resolve_link_returns_ambiguous_when_multiple_stems_and_no_source() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "a/note.md", "# A\n");
    ingest(&mut idx, "b/note.md", "# B\n");
    let result = idx.resolve_link("note", None).unwrap();
    assert!(matches!(result, LinkResolution::Ambiguous(_)));
}

#[test]
fn resolve_link_keeps_multiple_stems_ambiguous_by_default_even_with_source_path() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/note.md", "# Close\n");
    ingest(&mut idx, "other/note.md", "# Far\n");
    let result = idx.resolve_link("note", Some("notes/source.md")).unwrap();
    assert!(matches!(result, LinkResolution::Ambiguous(_)));
}

#[test]
fn resolve_link_can_use_proximity_when_explicitly_enabled() {
    let mut idx = Index::open_in_memory().unwrap();
    idx.proximity_enabled = true;
    let id_close = ingest(&mut idx, "notes/note.md", "# Close\n");
    let _id_far = ingest(&mut idx, "other/note.md", "# Far\n");
    let result = idx.resolve_link("note", Some("notes/source.md")).unwrap();
    assert_eq!(result, LinkResolution::Resolved(id_close));
}

#[test]
fn resolve_link_case_insensitive_stem() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "notes/MyNote.md", "# My Note\n");
    let result = idx.resolve_link("mynote", None).unwrap();
    assert_eq!(result, LinkResolution::Resolved(id));
}

#[test]
fn resolve_link_path_qualified_case_insensitive() {
    let mut idx = Index::open_in_memory().unwrap();
    let id = ingest(&mut idx, "Folder/Note.md", "# Note\n");
    let result = idx.resolve_link("folder/note", None).unwrap();
    assert_eq!(result, LinkResolution::Resolved(id));
}

#[test]
fn resolve_link_path_qualified_does_not_fall_back_to_plain_stem_match() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "note.md", "# Plain\n");
    let result = idx.resolve_link("missing/note", None).unwrap();
    assert_eq!(result, LinkResolution::Unresolved);
}

#[test]
fn resolve_link_path_qualified_requires_exact_path_not_prefix_match() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "folder/note-long.md", "# Long\n");
    let result = idx.resolve_link("folder/note", None).unwrap();
    assert_eq!(result, LinkResolution::Unresolved);
}

// ---------------------------------------------------------------------------
// Schema version mismatch triggers full rebuild
// ---------------------------------------------------------------------------

#[test]
fn schema_version_mismatch_drops_and_recreates_tables() {
    use rusqlite::Connection;

    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.db");

    // Open an index so the DB and schema are created.
    {
        let mut idx = Index::open(&db_path).unwrap();
        ingest(&mut idx, "notes/a.md", "# A\n");
        assert_eq!(idx.all_files().unwrap().len(), 1);
    }

    // Corrupt the schema version in the DB directly.
    {
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "UPDATE meta SET value = '999' WHERE key = 'schema_version'",
            [],
        )
        .unwrap();
    }

    // Reopen — version mismatch should trigger a full rebuild (empty DB).
    {
        let idx = Index::open(&db_path).unwrap();
        assert_eq!(
            idx.all_files().unwrap().len(),
            0,
            "DB should have been wiped on version mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// get_mtime and id_for_path
// ---------------------------------------------------------------------------

#[test]
fn get_mtime_returns_stored_value() {
    let mut idx = Index::open_in_memory().unwrap();
    let note = parse("# A\n");
    idx.upsert("notes/a.md", 1_700_000_000, &note).unwrap();
    assert_eq!(idx.get_mtime("notes/a.md").unwrap(), Some(1_700_000_000));
}

#[test]
fn get_mtime_returns_none_for_unknown_path() {
    let idx = Index::open_in_memory().unwrap();
    assert_eq!(idx.get_mtime("nonexistent.md").unwrap(), None);
}

#[test]
fn id_for_path_returns_none_for_unknown_path() {
    let idx = Index::open_in_memory().unwrap();
    assert!(idx.id_for_path("nope.md").unwrap().is_none());
}

// ---------------------------------------------------------------------------
// resolved_target_id — populated on upsert, updated on add/remove
// ---------------------------------------------------------------------------

#[test]
fn resolved_target_id_populated_after_upsert() {
    let mut idx = Index::open_in_memory().unwrap();
    let target_id = ingest(&mut idx, "notes/target.md", "# Target\n");
    let source_id = ingest(&mut idx, "notes/source.md", "See [[target]].\n");

    let links = idx.get_links(&source_id).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(
        links[0].resolved_target_id.as_deref(),
        Some(target_id.as_str()),
        "resolved_target_id must be populated after upsert"
    );
}

#[test]
fn resolved_target_id_populated_when_target_added_after_source() {
    // Source is indexed first — target doesn't exist yet.
    let mut idx = Index::open_in_memory().unwrap();
    let source_id = ingest(&mut idx, "notes/source.md", "See [[future]].\n");

    // Link is unresolved initially.
    let links_before = idx.get_links(&source_id).unwrap();
    assert!(links_before[0].resolved_target_id.is_none());

    // Now add the target.
    let target_id = ingest(&mut idx, "notes/future.md", "# Future\n");

    // Link should now be resolved.
    let links_after = idx.get_links(&source_id).unwrap();
    assert_eq!(
        links_after[0].resolved_target_id.as_deref(),
        Some(target_id.as_str()),
        "resolved_target_id must update when target file is added"
    );
}

#[test]
fn resolved_target_id_cleared_when_target_removed() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/target.md", "# Target\n");
    let source_id = ingest(&mut idx, "notes/source.md", "See [[target]].\n");

    // Confirm it resolves.
    let links = idx.get_links(&source_id).unwrap();
    assert!(links[0].resolved_target_id.is_some());

    // Remove the target.
    idx.remove_file("notes/target.md").unwrap();

    // Resolved ID should be NULL (set by ON DELETE SET NULL).
    let links_after = idx.get_links(&source_id).unwrap();
    assert!(links_after[0].resolved_target_id.is_none());
}

#[test]
fn ambiguous_link_resolves_when_one_candidate_removed() {
    let mut idx = Index::open_in_memory().unwrap();
    idx.proximity_enabled = false;
    ingest(&mut idx, "a/note.md", "# A\n");
    ingest(&mut idx, "b/note.md", "# B\n");
    let source_id = ingest(&mut idx, "src/source.md", "[[note]]\n");

    // Should be ambiguous (proximity disabled, two matches).
    let links = idx.get_links(&source_id).unwrap();
    assert!(links[0].resolved_target_id.is_none(), "should be ambiguous");

    // Remove one candidate.
    let surviving_id = idx.id_for_path("b/note.md").unwrap().unwrap();
    idx.remove_file("a/note.md").unwrap();

    // Should now resolve to the surviving file.
    let links_after = idx.get_links(&source_id).unwrap();
    assert_eq!(
        links_after[0].resolved_target_id.as_deref(),
        Some(surviving_id.as_str()),
        "previously ambiguous link should resolve after one candidate removed"
    );
}

// ---------------------------------------------------------------------------
// unresolved-link report
// ---------------------------------------------------------------------------

#[test]
fn report_unresolved_groups_counts_and_sorts_rows() {
    let mut idx = Index::open_in_memory().unwrap();

    ingest(&mut idx, "notes/source-a.md", "[[ghost]] [[ghost]] [[phantom]]\n");
    ingest(&mut idx, "notes/source-b.md", "[[Ghost]] [[zeta]]\n");
    ingest(&mut idx, "notes/source-c.md", "[[phantom]]\n");
    ingest(&mut idx, "notes/resolved.md", "# Resolved\n");
    ingest(&mut idx, "notes/source-d.md", "[[resolved]]\n");

    let report = idx.report_unresolved(500).unwrap();
    assert_eq!(report.total_count, 3);
    assert_eq!(report.rows.len(), 3);

    assert_eq!(report.rows[0].target, "ghost");
    assert_eq!(report.rows[0].kind, UnresolvedTargetKind::Unresolved);
    assert_eq!(report.rows[0].reference_count, 3);
    assert_eq!(
        report.rows[0].sample_sources,
        vec!["notes/source-a.md", "notes/source-a.md", "notes/source-b.md"]
    );
    assert!(!report.rows[0].has_more_sources);

    assert_eq!(report.rows[1].target, "phantom");
    assert_eq!(report.rows[1].reference_count, 2);
    assert_eq!(
        report.rows[1].sample_sources,
        vec!["notes/source-a.md", "notes/source-c.md"]
    );

    assert_eq!(report.rows[2].target, "zeta");
    assert_eq!(report.rows[2].reference_count, 1);
}

#[test]
fn report_unresolved_applies_limit_and_keeps_pre_limit_total_count() {
    let mut idx = Index::open_in_memory().unwrap();

    ingest(
        &mut idx,
        "notes/source.md",
        "[[alpha]] [[alpha]] [[alpha]] [[beta]] [[beta]] [[gamma]] [[delta]]\n",
    );

    let report = idx.report_unresolved(2).unwrap();
    assert_eq!(report.total_count, 4);
    assert_eq!(report.rows.len(), 2);
    assert_eq!(report.rows[0].target, "alpha");
    assert_eq!(report.rows[0].reference_count, 3);
    assert_eq!(report.rows[1].target, "beta");
    assert_eq!(report.rows[1].reference_count, 2);
}

#[test]
fn report_unresolved_classifies_ambiguous_targets() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "a/shared.md", "# A\n");
    ingest(&mut idx, "b/shared.md", "# B\n");
    ingest(&mut idx, "notes/source.md", "[[shared]] [[missing]]\n");

    let report = idx.report_unresolved(500).unwrap();
    let shared = report.rows.iter().find(|row| row.target == "shared").unwrap();
    let missing = report.rows.iter().find(|row| row.target == "missing").unwrap();

    assert_eq!(shared.kind, UnresolvedTargetKind::Ambiguous);
    assert_eq!(missing.kind, UnresolvedTargetKind::Unresolved);
}

#[test]
fn unresolved_target_sources_returns_deterministic_source_rows() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(
        &mut idx,
        "notes/alpha.md",
        "---\ntitle: Alpha Source\n---\n[[ghost|Shown]] [[ghost#frag]]\n",
    );
    ingest(&mut idx, "notes/beta.md", "[[Ghost]]\n");

    let rows = idx.unresolved_target_sources("ghost", 10).unwrap();
    assert_eq!(rows.len(), 3);

    assert_eq!(rows[0].source_path, "notes/alpha.md");
    assert_eq!(rows[0].source_title, "Alpha Source");
    assert_eq!(rows[0].target, "ghost");
    assert_eq!(rows[0].alias.as_deref(), Some("Shown"));
    assert_eq!(rows[0].fragment, None);

    assert_eq!(rows[1].source_path, "notes/alpha.md");
    assert_eq!(rows[1].fragment.as_deref(), Some("frag"));
    assert_eq!(rows[1].alias, None);

    assert_eq!(rows[2].source_path, "notes/beta.md");
    assert_eq!(rows[2].source_title, "notes/beta.md");

    let rows_again = idx.unresolved_target_sources("Ghost", 10).unwrap();
    assert_eq!(rows, rows_again);
}

#[test]
fn report_unresolved_drops_target_after_matching_file_is_added() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/source.md", "[[future-note]]\n");

    let before = idx.report_unresolved(500).unwrap();
    assert!(before.rows.iter().any(|row| row.target == "future-note"));

    ingest(&mut idx, "future-note.md", "# Future Note\n");

    let after = idx.report_unresolved(500).unwrap();
    assert!(!after.rows.iter().any(|row| row.target == "future-note"));
}

// ---------------------------------------------------------------------------
// plan_rename — edge cases
// ---------------------------------------------------------------------------

/// Helper: plan a rename and assert no index/disk mutation happened.
fn plan(idx: &Index, old: &str, new: &str) -> tektite_index::RenamePlan {
    idx.plan_rename(old, new).expect("plan_rename failed")
}

#[test]
fn rename_plain_stem_link_is_rewritten() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/old-name.md", "# Old\n");
    let source_id = ingest(&mut idx, "notes/source.md", "See [[old-name]].\n");

    // Confirm link resolves.
    let links = idx.get_links(&source_id).unwrap();
    assert!(links[0].resolved_target_id.is_some());

    let p = plan(&idx, "notes/old-name.md", "notes/new-name.md");
    assert_eq!(p.edits.len(), 1);
    assert_eq!(p.edits[0].before, "[[old-name]]");
    assert_eq!(p.edits[0].after, "[[new-name]]");
    assert_eq!(p.edits[0].file_path, "notes/source.md");
}

#[test]
fn rename_preserves_fragment_in_link() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/doc.md", "# Section\n");
    ingest(
        &mut idx,
        "notes/source.md",
        "[[doc#Section|see this]] and [[doc#Section]].\n",
    );

    let p = plan(&idx, "notes/doc.md", "notes/renamed-doc.md");

    // Both links should be rewritten with the fragment preserved.
    let befores: Vec<&str> = p.edits.iter().map(|e| e.before.as_str()).collect();
    let afters: Vec<&str> = p.edits.iter().map(|e| e.after.as_str()).collect();

    assert!(
        befores.contains(&"[[doc#Section|see this]]"),
        "fragment+alias link should be in edits"
    );
    assert!(
        afters.contains(&"[[renamed-doc#Section|see this]]"),
        "fragment+alias should be rewritten to new stem"
    );
    assert!(
        befores.contains(&"[[doc#Section]]"),
        "fragment-only link should be in edits"
    );
    assert!(
        afters.contains(&"[[renamed-doc#Section]]"),
        "fragment should be preserved after rename"
    );
}

#[test]
fn rename_preserves_display_alias_in_link() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/note.md", "# Note\n");
    ingest(&mut idx, "notes/source.md", "[[note|Custom Display]].\n");

    let p = plan(&idx, "notes/note.md", "notes/renamed.md");

    assert_eq!(p.edits.len(), 1);
    assert_eq!(p.edits[0].before, "[[note|Custom Display]]");
    assert_eq!(p.edits[0].after, "[[renamed|Custom Display]]");
}

#[test]
fn rename_skips_alias_based_links() {
    // The file has an alias "MyAlias". A link [[MyAlias]] resolves via alias
    // and must NOT be rewritten when the filename changes.
    let mut idx = Index::open_in_memory().unwrap();
    ingest(
        &mut idx,
        "notes/note.md",
        "---\naliases:\n  - MyAlias\n---\n# Note\n",
    );
    ingest(&mut idx, "notes/source.md", "[[MyAlias]] and [[note]].\n");

    let p = plan(&idx, "notes/note.md", "notes/renamed.md");

    // Only the stem-based link [[note]] should be rewritten.
    // [[MyAlias]] is alias-based and should be left alone.
    let alias_edit = p.edits.iter().find(|e| e.before == "[[MyAlias]]");
    let stem_edit = p.edits.iter().find(|e| e.before == "[[note]]");

    assert!(
        alias_edit.is_none(),
        "alias-based links must not be rewritten"
    );
    assert!(stem_edit.is_some(), "stem-based link must be rewritten");
    assert_eq!(stem_edit.unwrap().after, "[[renamed]]");
}

#[test]
fn rename_uses_path_qualified_target_when_new_stem_would_be_ambiguous() {
    // Two files will have the same stem after rename:
    //   notes/a.md → notes/note.md  (rename target)
    //   other/note.md               (already exists with same stem)
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/a.md", "# A\n");
    ingest(&mut idx, "other/note.md", "# Note\n");
    ingest(&mut idx, "src/source.md", "[[a]].\n");

    let p = plan(&idx, "notes/a.md", "notes/note.md");

    // [[a]] must become [[notes/note]] (path-qualified) to avoid ambiguity.
    assert_eq!(p.edits.len(), 1);
    assert_eq!(p.edits[0].before, "[[a]]");
    assert_eq!(p.edits[0].after, "[[notes/note]]");
}

#[test]
fn rename_rewrites_path_qualified_link() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "folder/note.md", "# Note\n");
    ingest(&mut idx, "src/source.md", "[[folder/note]].\n");

    let p = plan(&idx, "folder/note.md", "archive/note.md");

    assert_eq!(p.edits.len(), 1);
    assert_eq!(p.edits[0].before, "[[folder/note]]");
    assert_eq!(p.edits[0].after, "[[archive/note]]");
}

#[test]
fn rename_not_indexed_file_returns_empty_plan() {
    let idx = Index::open_in_memory().unwrap();
    let p = plan(&idx, "notes/ghost.md", "notes/new.md");
    assert!(p.edits.is_empty(), "plan for un-indexed file must be empty");
}

#[test]
fn rename_plan_is_idempotent_for_no_inbound_links() {
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/isolated.md", "# Isolated\n");
    // No other file links to isolated.md.
    let p = plan(&idx, "notes/isolated.md", "notes/renamed.md");
    assert!(p.edits.is_empty());
}

// ---------------------------------------------------------------------------
// plan_rename + apply_rename_index — end-to-end
// ---------------------------------------------------------------------------

#[test]
fn apply_rename_index_updates_path_and_resolves_links() {
    let mut idx = Index::open_in_memory().unwrap();
    let target_id = ingest(&mut idx, "notes/old.md", "# Old\n");
    let source_id = ingest(&mut idx, "notes/source.md", "[[old]].\n");

    let links = idx.get_links(&source_id).unwrap();
    assert_eq!(
        links[0].resolved_target_id.as_deref(),
        Some(target_id.as_str())
    );

    let plan = idx.plan_rename("notes/old.md", "notes/new.md").unwrap();

    // The source file's content after applying the edit.
    let new_source_content =
        tektite_index::rewrite_content("[[old]].\n", "notes/source.md", &plan.edits);
    assert_eq!(new_source_content, "[[new]].\n");

    idx.apply_rename_index(
        "notes/old.md",
        "notes/new.md",
        &[
            ("notes/new.md".to_string(), 0, "# Old\n".to_string()),
            ("notes/source.md".to_string(), 0, new_source_content),
        ],
    )
    .unwrap();

    // File path must be updated.
    assert!(idx.id_for_path("notes/old.md").unwrap().is_none());
    assert!(idx.id_for_path("notes/new.md").unwrap().is_some());

    // ID must be preserved.
    let new_id = idx.id_for_path("notes/new.md").unwrap().unwrap();
    assert_eq!(new_id, target_id);

    // Source link must resolve to the renamed file at its new path.
    let links_after = idx.get_links(&source_id).unwrap();
    assert_eq!(
        links_after[0].resolved_target_id.as_deref(),
        Some(new_id.as_str())
    );
}

#[test]
fn directory_rename_plan_covers_all_files() {
    // Three files in notes/: a.md, b.md, c.md
    // External files use path-qualified links — those need rewriting after a
    // directory rename. Stem-based links are unchanged (stems don't change).
    let mut idx = Index::open_in_memory().unwrap();
    ingest(&mut idx, "notes/a.md", "# A\n");
    ingest(&mut idx, "notes/b.md", "# B\n");
    ingest(&mut idx, "notes/c.md", "# C\n");
    ingest(&mut idx, "src/s1.md", "[[notes/a]] and [[b]].\n");
    ingest(&mut idx, "src/s2.md", "[[notes/c]].\n");

    let p = idx.plan_rename("notes", "archive").unwrap();

    // Path-qualified links need rewriting; files with only stem links do not.
    let files_edited: std::collections::HashSet<&str> =
        p.edits.iter().map(|e| e.file_path.as_str()).collect();
    assert!(
        files_edited.contains("src/s1.md"),
        "s1 has [[notes/a]] which must be rewritten"
    );
    assert!(
        files_edited.contains("src/s2.md"),
        "s2 has [[notes/c]] which must be rewritten"
    );

    // Verify the path-qualified rewrites.
    let edit_a = p.edits.iter().find(|e| e.before == "[[notes/a]]").unwrap();
    assert_eq!(edit_a.after, "[[archive/a]]");

    let edit_c = p.edits.iter().find(|e| e.before == "[[notes/c]]").unwrap();
    assert_eq!(edit_c.after, "[[archive/c]]");
}

// ---------------------------------------------------------------------------
// Graph neighborhood (Phase 0)
// ---------------------------------------------------------------------------

/// Builds a small linear graph:
///   a.md → b.md → c.md → d.md
/// with every note also having a trailing backlink to `hub.md` that links
/// back to `a.md` (to exercise backlink-driven expansion).
fn seed_linear(idx: &mut Index) -> [String; 5] {
    let a = ingest(idx, "a.md", "# A\n[[b]]\n");
    let b = ingest(idx, "b.md", "# B\n[[c]]\n");
    let c = ingest(idx, "c.md", "# C\n[[d]]\n");
    let d = ingest(idx, "d.md", "# D\n");
    let hub = ingest(idx, "hub.md", "# Hub\n[[a]]\n");
    [a, b, c, d, hub]
}

fn node_paths(data: &tektite_index::GraphData) -> Vec<&str> {
    data.nodes.iter().map(|n| n.path.as_str()).collect()
}

fn edge_tuples<'a>(
    data: &'a tektite_index::GraphData,
    id_to_path: &'a std::collections::HashMap<String, String>,
) -> Vec<(&'a str, &'a str)> {
    data.edges
        .iter()
        .map(|e| {
            (
                id_to_path.get(&e.source).map(String::as_str).unwrap_or("?"),
                id_to_path.get(&e.target).map(String::as_str).unwrap_or("?"),
            )
        })
        .collect()
}

fn path_map(ids_paths: &[(&str, &str)]) -> std::collections::HashMap<String, String> {
    ids_paths
        .iter()
        .map(|(id, p)| ((*id).to_string(), (*p).to_string()))
        .collect()
}

#[test]
fn neighborhood_depth_one_returns_immediate_neighbors() {
    let mut idx = Index::open_in_memory().unwrap();
    let [a, b, c, _d, hub] = seed_linear(&mut idx);

    let data = idx.neighborhood(&b, 1, &GraphFilters::default()).unwrap();

    // b is reached by: a→b (backlink), b→c (outgoing). hub is not adjacent.
    let paths = node_paths(&data);
    assert!(paths.contains(&"a.md"));
    assert!(paths.contains(&"b.md"));
    assert!(paths.contains(&"c.md"));
    assert!(!paths.contains(&"d.md"));
    assert!(!paths.contains(&"hub.md"));

    let map = path_map(&[(&a, "a.md"), (&b, "b.md"), (&c, "c.md"), (&hub, "hub.md")]);
    let edges = edge_tuples(&data, &map);
    assert!(edges.contains(&("a.md", "b.md")));
    assert!(edges.contains(&("b.md", "c.md")));
}

#[test]
fn neighborhood_depth_two_expands_further() {
    let mut idx = Index::open_in_memory().unwrap();
    let [a, _b, _c, _d, _hub] = seed_linear(&mut idx);

    let data = idx.neighborhood(&a, 2, &GraphFilters::default()).unwrap();
    let paths = node_paths(&data);
    // a → b (depth 1), b → c (depth 2), hub → a (depth 1 via backlink)
    assert!(paths.contains(&"a.md"));
    assert!(paths.contains(&"b.md"));
    assert!(paths.contains(&"c.md"));
    assert!(paths.contains(&"hub.md"));
    assert!(!paths.contains(&"d.md"));
}

#[test]
fn neighborhood_clamps_depth_above_max() {
    let mut idx = Index::open_in_memory().unwrap();
    let [a, _b, _c, _d, _hub] = seed_linear(&mut idx);
    // depth=99 must clamp to MAX_DEPTH=3; d.md is 3 hops from a.md.
    let data = idx.neighborhood(&a, 99, &GraphFilters::default()).unwrap();
    let paths = node_paths(&data);
    assert!(paths.contains(&"d.md"));
}

#[test]
fn neighborhood_dedups_duplicate_links() {
    let mut idx = Index::open_in_memory().unwrap();
    let a = ingest(&mut idx, "a.md", "[[b]] again [[b]] and once more [[b]]\n");
    ingest(&mut idx, "b.md", "# B\n");

    let data = idx.neighborhood(&a, 1, &GraphFilters::default()).unwrap();
    // Three link records to b, but only one edge should appear.
    assert_eq!(data.edges.len(), 1);
    assert_eq!(data.nodes.len(), 2);
}

#[test]
fn neighborhood_skips_self_loops() {
    let mut idx = Index::open_in_memory().unwrap();
    let a = ingest(&mut idx, "a.md", "[[a]] self-reference\n");

    let data = idx.neighborhood(&a, 1, &GraphFilters::default()).unwrap();
    assert_eq!(data.nodes.len(), 1);
    assert!(data.edges.is_empty());
}

#[test]
fn neighborhood_skips_unresolved_links() {
    let mut idx = Index::open_in_memory().unwrap();
    let a = ingest(&mut idx, "a.md", "[[does-not-exist]]\n");

    let data = idx.neighborhood(&a, 1, &GraphFilters::default()).unwrap();
    assert_eq!(data.nodes.len(), 1);
    assert!(data.edges.is_empty());
}

#[test]
fn neighborhood_returns_empty_for_missing_center() {
    let idx = Index::open_in_memory().unwrap();
    let data = idx
        .neighborhood("not-a-real-id", 1, &GraphFilters::default())
        .unwrap();
    assert!(data.nodes.is_empty());
    assert!(data.edges.is_empty());
}

#[test]
fn neighborhood_folder_filter_excludes_mismatched_paths() {
    let mut idx = Index::open_in_memory().unwrap();
    let a = ingest(&mut idx, "work/a.md", "[[work/b]] [[personal/c]]\n");
    ingest(&mut idx, "work/b.md", "# B\n");
    ingest(&mut idx, "personal/c.md", "# C\n");

    let filters = GraphFilters {
        folder: Some("work/".to_string()),
        ..GraphFilters::default()
    };
    let data = idx.neighborhood(&a, 1, &filters).unwrap();
    let paths = node_paths(&data);
    assert!(paths.contains(&"work/a.md"));
    assert!(paths.contains(&"work/b.md"));
    assert!(!paths.contains(&"personal/c.md"));
}

#[test]
fn neighborhood_tag_filter_keeps_matching_tags() {
    let mut idx = Index::open_in_memory().unwrap();
    let a = ingest(&mut idx, "a.md", "#hub\n[[b]] [[c]]\n");
    ingest(&mut idx, "b.md", "#keep\n# B\n");
    ingest(&mut idx, "c.md", "#skip\n# C\n");

    let filters = GraphFilters {
        tags: Some(vec!["keep".to_string(), "hub".to_string()]),
        ..GraphFilters::default()
    };
    let data = idx.neighborhood(&a, 1, &filters).unwrap();
    let paths = node_paths(&data);
    assert!(paths.contains(&"a.md"));
    assert!(paths.contains(&"b.md"));
    assert!(!paths.contains(&"c.md"));
}

#[test]
fn neighborhood_recency_filter_excludes_old_notes() {
    let mut idx = Index::open_in_memory().unwrap();
    let a = ingest_at(&mut idx, "a.md", 1_000_000, "[[b]] [[c]]\n");
    ingest_at(&mut idx, "b.md", 2_000_000, "# B\n");
    ingest_at(&mut idx, "c.md", 500_000, "# C\n");

    let filters = GraphFilters {
        modified_after: Some(900_000),
        ..GraphFilters::default()
    };
    let data = idx.neighborhood(&a, 1, &filters).unwrap();
    let paths = node_paths(&data);
    assert!(paths.contains(&"a.md"));
    assert!(paths.contains(&"b.md"));
    assert!(!paths.contains(&"c.md"));
}

#[test]
fn neighborhood_node_cap_keeps_center_and_prunes_lowest_link_count() {
    let mut idx = Index::open_in_memory().unwrap();

    // Center links to NODE_CAP + 10 leaves. Each leaf has only the incoming
    // edge (link_count = 1) so every leaf is a pruning candidate; the center
    // has link_count = NODE_CAP + 10 and must survive.
    let over = NODE_CAP + 10;
    let mut body = String::from("# Hub\n");
    for i in 0..over {
        body.push_str(&format!("[[leaf-{i}]]\n"));
    }
    let center = ingest(&mut idx, "hub.md", &body);
    for i in 0..over {
        ingest(&mut idx, &format!("leaf-{i}.md"), "# Leaf\n");
    }

    let data = idx.neighborhood(&center, 1, &GraphFilters::default()).unwrap();
    assert_eq!(data.nodes.len(), NODE_CAP);
    assert!(
        data.nodes.iter().any(|n| n.id == center),
        "center must survive cap enforcement"
    );
    // Every remaining edge must reference two surviving nodes.
    let ids: std::collections::HashSet<&str> =
        data.nodes.iter().map(|n| n.id.as_str()).collect();
    for edge in &data.edges {
        assert!(ids.contains(edge.source.as_str()));
        assert!(ids.contains(edge.target.as_str()));
    }
}

#[test]
fn neighborhood_node_metadata_includes_title_tags_mtime_and_link_count() {
    let mut idx = Index::open_in_memory().unwrap();
    let a = ingest_at(
        &mut idx,
        "a.md",
        42,
        "---\ntitle: Titled Note\n---\n#alpha #beta\n[[b]]\n",
    );
    ingest(&mut idx, "b.md", "# B\n");

    let data = idx.neighborhood(&a, 1, &GraphFilters::default()).unwrap();
    let node_a = data.nodes.iter().find(|n| n.id == a).unwrap();
    assert_eq!(node_a.title, "Titled Note");
    assert_eq!(node_a.modified, 42);
    assert!(node_a.tags.contains(&"alpha".to_string()));
    assert!(node_a.tags.contains(&"beta".to_string()));
    assert!(node_a.link_count >= 1);
}

// ---------------------------------------------------------------------------
// Phase 0 exit-criteria benchmark: <50ms on a 500-file vault at depth 2.
//
// Ignored by default so normal `cargo test` stays fast; run with:
//   cargo test -p tektite-index --test integration -- --ignored neighborhood_benchmark
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn neighborhood_benchmark_500_files_depth_two_under_50ms() {
    use std::time::Instant;

    let mut idx = Index::open_in_memory().unwrap();

    // 500 files in 10 folders. Each file links to the next two in the same
    // folder (ring) and to one file in the "next" folder — gives a realistic
    // link density where BFS has to do real work at depth 2.
    let file_count = 500usize;
    let folder_count = 10usize;
    let per_folder = file_count / folder_count;

    let mut first_id: Option<String> = None;
    for i in 0..file_count {
        let folder = i / per_folder;
        let idx_in_folder = i % per_folder;
        let next_a = (idx_in_folder + 1) % per_folder;
        let next_b = (idx_in_folder + 2) % per_folder;
        let cross_folder = (folder + 1) % folder_count;
        let body = format!(
            "[[folder-{folder}/note-{next_a}]] [[folder-{folder}/note-{next_b}]] [[folder-{cross_folder}/note-{idx_in_folder}]]\n",
        );
        let path = format!("folder-{folder}/note-{idx_in_folder}.md");
        let id = ingest(&mut idx, &path, &body);
        if first_id.is_none() {
            first_id = Some(id);
        }
    }
    let center = first_id.unwrap();

    // Warm up any lazy statement preparation.
    let _ = idx.neighborhood(&center, 2, &GraphFilters::default()).unwrap();

    let runs = 5;
    let mut total_us = 0u128;
    for _ in 0..runs {
        let start = Instant::now();
        let data = idx.neighborhood(&center, 2, &GraphFilters::default()).unwrap();
        total_us += start.elapsed().as_micros();
        assert!(!data.nodes.is_empty());
        assert!(data.nodes.len() <= NODE_CAP);
    }
    let avg_ms = (total_us as f64) / (runs as f64) / 1000.0;
    eprintln!("neighborhood avg: {avg_ms:.2} ms over {runs} runs (500 files, depth 2)");
    assert!(
        avg_ms < 50.0,
        "depth-2 neighborhood on 500 files should complete in <50ms, got {avg_ms:.2}ms",
    );
}
