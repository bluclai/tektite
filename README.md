# Tektite

Tektite is a local-first, agentic markdown workspace for the agent era.

It is being built for people who want their notes to become durable context: a knowledge base that humans and AI tools can both work from. Tektite combines the freedom of plain markdown files with the speed, structure, and polish of a real desktop engine.

Built with Tauri, SvelteKit, TypeScript, and Rust, the v1 focus is the core substrate: fast editing, split panes, wiki-links, backlinks, search, safe renames, and responsive vault syncing.

## Why Tektite?

Most note apps were designed for solo human writing.
Most AI tools are built around disposable chat threads.

Tektite sits in the gap between them.

It is opinionated about a few things:

- Your markdown files on disk are the source of truth
- Notes should become durable context, not disposable chat history
- Humans and agents should be able to work from the same knowledge base
- Desktop UX should feel fast, focused, and native
- Link-heavy note systems need stronger semantics than string-matching hacks
- Search, backlinks, and rename safety should come from a real index
- Local-first should not mean fragile
- Your knowledge should outlive any single model, tool, or app

Tektite is not trying to be a chatbot wrapper around notes. The v1 goal is to build a reliable, local-first markdown substrate for human-and-agent work.

## V1 at a glance

Planned v1 capabilities:

- Real markdown editing with CodeMirror 6
- Autosave and explicit save flows
- File explorer backed by the real vault
- Split-pane editing with workspace restore
- Wiki-link autocomplete and navigation
- Search panel and command palette
- Backlinks panel
- Rename preview/apply flow with link updates
- Live preview mode
- External file change detection and conflict handling

The v1 emphasis is foundation first: make markdown durable, connected, and fast enough to support serious agentic workflows later.

## Core product principles

From the reconciled PRD and current positioning, these are the decisions shaping Tektite:

- Markdown files remain authoritative
- `.tektite/` stores app metadata, workspace state, and index artifacts
- The shell layout is titlebar -> body -> status bar
- The workspace layout is activity bar -> sidebar -> editor area
- Files, Search, and Backlinks are the main sidebar panels
- Multi-document editing uses recursive split panes with one active pane
- CodeMirror 6 is the editor runtime
- The Rust backend owns parsing, indexing, search, backlinks, rename planning, and filesystem watching
- Durable context matters more than ephemeral chat history
- The product is designed for human + agent workflows, even where v1 focuses on the substrate rather than in-app AI features

## Architecture

### Frontend

Built with:

- SvelteKit
- TypeScript
- Tauri 2
- CodeMirror 6
- Tailwind CSS 4
- bits-ui / shadcn-svelte style components

Frontend responsibilities:

- App shell and pane layout
- Editor presentation and interactions
- Sidebar panels for files, search, and backlinks
- Command palette and diagnostics surfaces
- Workspace state and active-pane behavior
- A calm workspace for writing, navigating, and curating durable context

### Backend

The backend is organized as a Rust workspace under `src-tauri/crates/`:

- `tektite-parser` — markdown metadata extraction
- `tektite-index` — SQLite schema, ingest, query, resolve, rename
- `tektite-vault` — vault I/O, scans, watcher, rename propagation
- `tektite-search` — search ranking and fuzzy matching

Backend responsibilities:

- Vault file I/O
- Markdown parsing
- SQLite indexing
- Link resolution
- Backlinks
- Rename preview/apply workflows
- Filesystem watching and refresh
- Maintaining durable, portable context that can support both human and agent workflows

## Data model

Tektite uses a local-first architecture with a proper index:

- Markdown files in the vault are the source of truth
- The SQLite index lives at `.tektite/index.db`
- Workspace state is versioned in `workspace.json`
- File identity in the live index is UUID-based
- Aliases are normalized into indexed storage
- Link resolution is case-insensitive and can explicitly surface ambiguity
- Notes remain portable across tools instead of being trapped in one AI interface

## Product direction

Tektite is not just a faster markdown editor.

It is a local-first note-taking app for the agent era: a workspace where notes, specs, plans, and project knowledge can become durable working context.

That means aiming for a product where:

- humans can write, think, and refine
- agents can read from the same knowledge base
- context survives beyond a single session or chat
- notes remain plain files you actually own
- the system gets stronger as it grows instead of messier

## Roadmap

The reconciled v1 plan is split into 10 phases:

1. Crate scaffold + parser hardening
2. Index identity + alias normalization
3. Editor open/save vertical slice
4. File explorer vertical slice
5. Split panes + full workspace restore
6. Link resolution + rename planning
7. Wiki-link foundation
8. Search panel + command palette
9. Backlinks + rename apply UI
10. Live preview + external change handling

Primary planning docs:

- `plans/app-v1-reconciled.md`
- `docs/prd-app-v1-reconciled.md`
- `docs/core-engine-hardening.md`

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
├── docs/                        # PRDs, design docs, engine notes
├── plans/                       # Implementation plans and phase breakdowns
└── README.md
```

## Getting started

Prerequisites:

- Bun
- Rust toolchain
- Tauri system dependencies for your OS

Install dependencies:

```bash
bun install
```

Start the app in development:

```bash
bun run tauri dev
```

Build the frontend:

```bash
bun run build
```

Build the desktop app:

```bash
bun run tauri build
```

## Useful scripts

```bash
bun run dev          # Vite dev server
bun run build        # Production frontend build
bun run check        # Svelte + TypeScript checks
bun run lint         # Oxlint
bun run format       # Oxfmt for src/
bun run tauri dev    # Tauri desktop dev
bun run tauri build  # Tauri desktop build
```

## Status

This README reflects the v1 reconciled PRD plus the newer agentic positioning direction.

In practice, that means v1 is still centered on the core markdown substrate and desktop engine, while the broader product thesis is human + agent knowledge work built on durable local-first notes.

For exact implementation scope and sequencing, use the plan files as the source of truth.
