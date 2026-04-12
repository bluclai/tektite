# AGENTS.md

## Tektite
- Local-first markdown workspace / agentic markdown workspace.
- Notes are plain `.md` files on disk.
- Core value: fast wiki-link navigation, backlinks, search, safe rename with link rewriting.

## Important dirs
- `src/` — frontend UI
- `src-tauri/` — Tauri app + Rust backend
- `src-tauri/crates/tektite-parser` — markdown parsing
- `src-tauri/crates/tektite-index` — SQLite index + link resolution
- `src-tauri/crates/tektite-vault` — vault I/O, scanning, rename apply
- `src-tauri/crates/tektite-search` — search
- `docs/` — product/design docs
- `plans/` — implementation plans

## Dev
```bash
bun install
bun run check
bun run test
bun run tauri dev
bun run tauri build
```

## Rules
- Preserve local-first behavior.
- Markdown files are the source of truth.
- Avoid introducing lock-in or hidden storage.
- Prefer small, focused changes.
- Keep UX calm, sharp, and fast.
