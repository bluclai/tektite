# Tektite

Tektite is a local-first markdown workspace. Your notes live as plain `.md` files on disk — Tektite adds a real index, wiki-link navigation, backlinks, search, and safe rename with link rewriting on top of them.

Built with Tauri, SvelteKit, TypeScript, and Rust.

---

## v0.1

Tektite v0.1 is the foundation release. It ships the core workflow:

1. Open a vault (a folder of markdown files)
2. Browse and create notes in the file explorer
3. Edit notes with autosave and explicit save
4. Follow `[[wiki-links]]` to navigate between notes
5. Search notes by content
6. Inspect backlinks for the current note
7. Rename a note with a preview of all affected links before applying

This is the substrate. Everything else builds on top.

---

## Getting started

Prerequisites:

- Bun
- Rust toolchain
- Tauri system dependencies for your OS (see [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/))

Install dependencies:

```bash
bun install
```

Start the app in development:

```bash
bun run tauri dev
```

Build the desktop app:

```bash
bun run tauri build
```

---

## Useful scripts

```bash
bun run dev          # Vite dev server (frontend only)
bun run build        # Production frontend build
bun run check        # Svelte + TypeScript checks
bun run lint         # Oxlint
bun run tauri dev    # Tauri desktop dev
bun run tauri build  # Tauri desktop build
```

---

## Architecture

### Frontend

SvelteKit, Svelte 5 runes, TypeScript, Tauri 2, CodeMirror 6, Tailwind CSS 4.

Frontend responsibilities:

- App shell and split-pane layout
- Editor presentation and interactions (CodeMirror 6, source mode)
- Sidebar panels: Files, Search, Backlinks
- Command palette (file jump, panel navigation)
- Workspace state and active-pane behavior

### Backend

Rust workspace under `src-tauri/crates/`:

- `tektite-parser` — markdown metadata extraction (headings, wiki-links, tags, tasks)
- `tektite-index` — SQLite schema, ingest, query, link resolution, rename planning
- `tektite-vault` — vault I/O, directory scanning, file watching, rename application
- `tektite-search` — search ranking and fuzzy file matching

Backend responsibilities:

- Vault file I/O
- Markdown parsing
- SQLite indexing and link resolution
- Backlinks
- Rename preview and apply with link rewriting
- Filesystem watching and index refresh

### Data model

- Markdown files are the source of truth
- The SQLite index lives at `.tektite/index.db`
- Workspace state is persisted in `workspace.json`
- Link resolution is case-insensitive with explicit ambiguity surfacing
- Notes stay portable plain files — no app lock-in

---

## Repository structure

```text
tektite/
├── src/                         # SvelteKit frontend
├── src-tauri/                   # Tauri app + Rust backend
│   ├── crates/
│   │   ├── tektite-parser/
│   │   ├── tektite-index/
│   │   ├── tektite-vault/
│   │   └── tektite-search/
├── docs/                        # PRDs and design docs
├── plans/                       # Implementation plans
└── README.md
```
