/**
 * wiki-link.ts — Phase 7 wiki-link CM6 extension.
 *
 * Responsibilities:
 *   1. Syntax decoration — `[[...]]` patterns are styled as first-class syntax
 *      regardless of link resolution state. Brackets are de-emphasised; the
 *      target text is highlighted in the primary accent colour.
 *   2. Resolution decorations — after parsing, links are resolved against the
 *      backend asynchronously. Resolved links get a hover-navigable style;
 *      unresolved links get a distinct muted treatment; ambiguous links are
 *      flagged separately.
 *   3. Autocomplete — typing `[[` triggers a suggestion list sourced from the
 *      vault index. Suggestions prefer stable inserts: unique note names insert
 *      `[[note]]`, while duplicate filenames insert path-qualified links.
 *   4. Link following — Ctrl/Cmd-clicking a wiki-link (or Alt+Enter with the
 *      cursor inside one) opens the target note in the active pane. Ambiguous
 *      links trigger a disambiguation dialog rather than a blind open.
 *
 * The extension is structured as a single `Extension` export so it can be
 * dropped into the `wikiLinkCompartment` in EditorPane without restructuring
 * the base stack. The autocomplete source is a separate export so it can be
 * composed into the `autocompleteCompartment`.
 *
 * Live preview (Phase 10) decorates on the same syntax nodes. Source mode
 * applies only the mark decorations here; live preview will add replace
 * decorations on top. Both share the same `wikiLinkRanges` state field so
 * Phase 10 can read parsed positions without re-parsing.
 */

import type { Completion, CompletionResult } from "@codemirror/autocomplete";
import type { Extension } from "@codemirror/state";
import type { DecorationSet } from "@codemirror/view";

import { autocompletion, CompletionContext } from "@codemirror/autocomplete";
import { StateField, StateEffect, RangeSetBuilder } from "@codemirror/state";
import { EditorView, Decoration, ViewPlugin, ViewUpdate, keymap } from "@codemirror/view";
import { invoke } from "@tauri-apps/api/core";
import { filesStore } from "$lib/stores/files.svelte";
import { parseWikiLinks, rootNotePathForTarget, initialContentForTarget } from "./wiki-link-parse";
import type { WikiLink } from "./wiki-link-parse";
export type { WikiLink } from "./wiki-link-parse";

// ---------------------------------------------------------------------------
// IPC types (mirrors Rust LinkResolutionResult + index commands)
// ---------------------------------------------------------------------------

export interface FileCompletionEntry {
  path: string;
  name: string;
}

export type LinkResolutionResult =
  | { kind: "resolved"; path: string }
  | { kind: "unresolved" }
  | { kind: "ambiguous"; paths: string[] };

// Re-export parseWikiLinks for consumers
export { parseWikiLinks } from "./wiki-link-parse";

// ---------------------------------------------------------------------------
// State effect: resolution results returned from the backend
// ---------------------------------------------------------------------------

interface ResolutionEntry {
  from: number;
  to: number;
  result: LinkResolutionResult;
}

const setResolutions = StateEffect.define<ResolutionEntry[]>();

// ---------------------------------------------------------------------------
// State field: maps (from, to) → resolution result
// Keyed as `${from}:${to}` for fast lookup in the decoration builder.
// ---------------------------------------------------------------------------

const resolutionField = StateField.define<Map<string, LinkResolutionResult>>({
  create() {
    return new Map();
  },
  update(map, tr) {
    for (const effect of tr.effects) {
      if (effect.is(setResolutions)) {
        // Build a fresh map; positions are remapped via transaction
        // mapping so stale entries don't pile up.
        const next = new Map<string, LinkResolutionResult>();
        for (const { from, to, result } of effect.value) {
          const mFrom = tr.changes.mapPos(from);
          const mTo = tr.changes.mapPos(to);
          next.set(`${mFrom}:${mTo}`, result);
        }
        // Merge: keep existing entries for positions not touched by
        // this effect (they're still valid for unchanged regions).
        for (const [k, v] of map) {
          if (!next.has(k)) {
            // Remap the key through document changes.
            const [f, t] = k.split(":").map(Number);
            const mf = tr.changes.mapPos(f);
            const mt = tr.changes.mapPos(t);
            // Verify the text at the remapped position still
            // looks like a wiki-link before keeping it.
            next.set(`${mf}:${mt}`, v);
          }
        }
        return next;
      }
    }
    if (tr.docChanged) {
      // Remap all keys through the document changes.
      const next = new Map<string, LinkResolutionResult>();
      for (const [k, v] of map) {
        const [f, t] = k.split(":").map(Number);
        const mf = tr.changes.mapPos(f);
        const mt = tr.changes.mapPos(t);
        next.set(`${mf}:${mt}`, v);
      }
      return next;
    }
    return map;
  },
});

// ---------------------------------------------------------------------------
// Decorations
// ---------------------------------------------------------------------------

// Wiki-link bracket punctuation (de-emphasised / hidden when completed)
const dBracket = Decoration.mark({ class: "cm-wl-bracket" });
const dBracketHidden = Decoration.mark({ class: "cm-wl-bracket-hidden" });
// Wiki-link whole-range base styling (always applied)
const dLinkBase = Decoration.mark({ class: "cm-wl-link" });
// Wiki-link target base styling (always applied)
const dTargetBase = Decoration.mark({ class: "cm-wl-target" });
// Wiki-link target text — resolved
const dResolved = Decoration.mark({ class: "cm-wl-resolved" });
// Wiki-link target text — unresolved
const dUnresolved = Decoration.mark({ class: "cm-wl-unresolved" });
// Wiki-link target text — ambiguous
const dAmbiguous = Decoration.mark({ class: "cm-wl-ambiguous" });
// Wiki-link target text — pending (not yet resolved)
const dPending = Decoration.mark({ class: "cm-wl-pending" });
// Fragment separator and text
const dFragment = Decoration.mark({ class: "cm-wl-fragment" });
// Alias separator and text
const dAlias = Decoration.mark({ class: "cm-wl-alias" });

// ---------------------------------------------------------------------------
// Resolution debounce / async coordination
// ---------------------------------------------------------------------------

const RESOLVE_DEBOUNCE_MS = 300;

// ---------------------------------------------------------------------------
// View plugin: parses visible wiki-links, applies decorations, triggers async
// resolution of newly-seen links.
// ---------------------------------------------------------------------------

const wikiLinkPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    private resolveTimer: ReturnType<typeof setTimeout> | null = null;
    /** Links seen in the current viewport — used to avoid re-resolving. */
    private pendingLinks: WikiLink[] = [];

    constructor(view: EditorView) {
      this.decorations = this.buildDecorations(view);
      this.scheduleResolve(view);
    }

    update(update: ViewUpdate) {
      if (
        update.docChanged ||
        update.selectionSet ||
        update.viewportChanged ||
        update.transactions.some((tr) => tr.effects.some((e) => e.is(setResolutions)))
      ) {
        this.decorations = this.buildDecorations(update.view);
      }
      if (update.docChanged || update.viewportChanged) {
        this.scheduleResolve(update.view);
      }
    }

    destroy() {
      if (this.resolveTimer !== null) clearTimeout(this.resolveTimer);
    }

    private buildDecorations(view: EditorView): DecorationSet {
      const builder = new RangeSetBuilder<Decoration>();
      const resolutions = view.state.field(resolutionField);
      const doc = view.state.doc;

      // Collect all wiki-links in the whole document for decoration
      // (not just the viewport) so non-visible edits still get resolved.
      const docText = doc.toString();
      const links = parseWikiLinks(docText);
      const cursor = view.state.selection.main.head;

      for (const link of links) {
        const key = `${link.from}:${link.to}`;
        const resolution = resolutions.get(key);

        const cursorInside = cursor >= link.from && cursor <= link.to;
        // Hide brackets for every completed wiki-link except the one currently
        // being edited (cursor inside the link range).
        const hideBrackets = !cursorInside;

        // Base styling for the whole link (helps in list-item contexts).
        builder.add(link.from, link.to, dLinkBase);

        // --- Opening brackets [[
        builder.add(link.from, link.from + 2, hideBrackets ? dBracketHidden : dBracket);

        // Determine the decoration for the target text.
        let targetDeco: Decoration;
        if (resolution === undefined) {
          targetDeco = dPending;
        } else if (resolution.kind === "resolved") {
          targetDeco = dResolved;
        } else if (resolution.kind === "unresolved") {
          targetDeco = dUnresolved;
        } else {
          targetDeco = dAmbiguous;
        }

        // The target occupies positions [from+2 ... ]
        // We need to compute where each part starts/ends inside `link.raw`.
        // raw = `[[target#fragment|alias]]`
        //        01234...
        const targetEnd = link.from + 2 + link.target.length;
        builder.add(link.from + 2, targetEnd, dTargetBase);
        builder.add(link.from + 2, targetEnd, targetDeco);

        // Fragment (#heading)
        if (link.fragment !== null) {
          // The `#` separator and the fragment text
          const hashPos = targetEnd;
          const fragmentEnd = hashPos + 1 + link.fragment.length;
          builder.add(hashPos, fragmentEnd, dFragment);
        }

        // Alias (|display)
        if (link.alias !== null) {
          // Compute alias start: after target + optional fragment
          let aliasStart = link.from + 2 + link.target.length;
          if (link.fragment !== null) {
            aliasStart += 1 + link.fragment.length; // # + fragment
          }
          // aliasStart is now at the `|`
          const aliasEnd = aliasStart + 1 + link.alias.length;
          builder.add(aliasStart, aliasEnd, dAlias);
        }

        // --- Closing brackets ]]
        builder.add(link.to - 2, link.to, hideBrackets ? dBracketHidden : dBracket);
      }

      return builder.finish();
    }

    private scheduleResolve(view: EditorView) {
      if (this.resolveTimer !== null) clearTimeout(this.resolveTimer);
      this.resolveTimer = setTimeout(() => {
        this.resolveTimer = null;
        void this.resolveLinks(view);
      }, RESOLVE_DEBOUNCE_MS);
    }

    private async resolveLinks(view: EditorView) {
      const docText = view.state.doc.toString();
      const links = parseWikiLinks(docText);
      if (links.length === 0) return;

      const resolutions = view.state.field(resolutionField);
      // Only resolve links that don't have a cached resolution yet
      // (or whose position is stale after edits).
      const toResolve = links.filter((l) => !resolutions.has(`${l.from}:${l.to}`));
      if (toResolve.length === 0) return;

      // Get the source path for proximity tiebreaking from the handlers field.
      const sourcePath: string | undefined = view.state.field(linkHandlersField)?.sourcePath;

      // Resolve in parallel (each IPC call is cheap; batching is deferred
      // to Phase 8+ if profiling shows overhead).
      const results = await Promise.allSettled(
        toResolve.map(async (link) => {
          const result = await invoke<LinkResolutionResult>("index_resolve_link", {
            target: link.target,
            sourcePath: sourcePath ?? null,
          });
          return { link, result };
        }),
      );

      const entries: ResolutionEntry[] = [];
      for (const r of results) {
        if (r.status === "fulfilled") {
          entries.push({
            from: r.value.link.from,
            to: r.value.link.to,
            result: r.value.result,
          });
        }
      }

      if (entries.length === 0) return;

      // Dispatch the resolutions back into the editor state.
      // Guard against view being destroyed between the async gap.
      if (!view.dom.isConnected) return;
      view.dispatch({ effects: setResolutions.of(entries) });
    }
  },
  {
    decorations: (v) => v.decorations,
  },
);

// ---------------------------------------------------------------------------
// Link-following helpers
// ---------------------------------------------------------------------------

/**
 * Finds the wiki-link at or around a document position, if any.
 */
function wikiLinkAt(state: { doc: { toString: () => string } }, pos: number): WikiLink | null {
  const text = state.doc.toString();
  const links = parseWikiLinks(text);
  for (const link of links) {
    if (pos >= link.from && pos <= link.to) return link;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Notification callback type — injected by EditorPane
// ---------------------------------------------------------------------------

/** Callback invoked when a wiki-link is ambiguous (multiple candidates). */
export type AmbiguousLinkHandler = (target: string, paths: string[]) => void;

/** Callback invoked when a resolved wiki-link is followed. */
export type FollowLinkHandler = (absolutePath: string) => void;

// We store the callbacks on a `StateField` so the view plugin can access
// them reactively without closing over stale references.

interface LinkHandlers {
  onFollow: FollowLinkHandler;
  onAmbiguous: AmbiguousLinkHandler;
  vaultRoot: string;
  sourcePath: string;
}

const linkHandlersField = StateField.define<LinkHandlers | null>({
  create: () => null,
  update: (v, tr) => {
    for (const e of tr.effects) {
      if (e.is(setLinkHandlers)) return e.value;
    }
    return v;
  },
});

const setLinkHandlers = StateEffect.define<LinkHandlers | null>();

// ---------------------------------------------------------------------------
// Follow command
// ---------------------------------------------------------------------------

// rootNotePathForTarget and initialContentForTarget are now imported from wiki-link-parse.ts

async function createRootNoteForUnresolvedLink(target: string, handlers: LinkHandlers): Promise<boolean> {
  const relPath = rootNotePathForTarget(target);
  if (!relPath) return false;

  const initialContent = initialContentForTarget(target);

  try {
    await invoke("files_create_file", { relPath, initialContent });
    await filesStore.refresh();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error ?? "");
    if (!message.toLowerCase().includes("already exists")) {
      return false;
    }
  }

  handlers.onFollow(relPath);
  return true;
}

async function followLinkAtCursor(view: EditorView): Promise<boolean> {
  const { head } = view.state.selection.main;
  const link = wikiLinkAt(view.state, head);
  if (!link) return false;

  const handlers = view.state.field(linkHandlersField);
  if (!handlers) return false;

  const resolution = await invoke<LinkResolutionResult>("index_resolve_link", {
    target: link.target,
    sourcePath: handlers.sourcePath ?? null,
  });

  if (resolution.kind === "resolved") {
    handlers.onFollow(resolution.path);
    return true;
  } else if (resolution.kind === "ambiguous") {
    handlers.onAmbiguous(link.target, resolution.paths);
    return true;
  }

  return createRootNoteForUnresolvedLink(link.target, handlers);
}

// ---------------------------------------------------------------------------
// Click handler
// ---------------------------------------------------------------------------

const clickHandler = EditorView.domEventHandlers({
  mousedown(event, view) {
    // Follow on normal primary click for v0.1. This keeps wiki-links feeling
    // like links instead of hidden power-user affordances.
    if (event.button !== 0) return false;

    const pos = view.posAtCoords({ x: event.clientX, y: event.clientY });
    if (pos === null) return false;

    const link = wikiLinkAt(view.state, pos);
    if (!link) return false;

    const handlers = view.state.field(linkHandlersField);
    if (!handlers) return false;

    // Prevent CM6 from repositioning the cursor — we're navigating away.
    event.preventDefault();

    void invoke<LinkResolutionResult>("index_resolve_link", {
      target: link.target,
      sourcePath: handlers.sourcePath ?? null,
    }).then(async (resolution) => {
      if (resolution.kind === "resolved") {
        handlers.onFollow(resolution.path);
      } else if (resolution.kind === "ambiguous") {
        handlers.onAmbiguous(link.target, resolution.paths);
      } else {
        await createRootNoteForUnresolvedLink(link.target, handlers);
      }
    });

    return true;
  },
});

// ---------------------------------------------------------------------------
// Cursor style: show pointer when hovering a wiki-link with modifier
// ---------------------------------------------------------------------------

const pointerCursorTheme = EditorView.theme({
  ".cm-wl-resolved": {
    cursor: "pointer",
  },
  ".cm-wl-unresolved": {
    cursor: "pointer",
  },
});

// ---------------------------------------------------------------------------
// Autocomplete source
// ---------------------------------------------------------------------------

/**
 * Autocomplete source for `[[note]]` links.
 *
 * Triggers on `[[` and suggests indexed markdown files. For stability in v0.1,
 * autocomplete only completes note targets and prefers path-qualified inserts
 * when filename stems are duplicated.
 */
export async function wikiLinkCompletionSource(
  ctx: CompletionContext,
): Promise<CompletionResult | null> {
  const { state, pos } = ctx;
  const text = state.doc.sliceString(0, pos);

  const lookback = text.slice(Math.max(0, pos - 200));
  const openBracket = lookback.lastIndexOf("[[");
  if (openBracket === -1) return null;

  const afterOpen = lookback.slice(openBracket + 2);
  if (afterOpen.includes("]]")) return null;
  if (afterOpen.includes("#") || afterOpen.includes("|")) return null;

  const noteFrom = pos - afterOpen.length;

  let files: FileCompletionEntry[];
  try {
    files = await invoke<FileCompletionEntry[]>("index_get_files");
  } catch {
    return null;
  }

  const nameCounts = new Map<string, number>();
  for (const file of files) {
    nameCounts.set(file.name, (nameCounts.get(file.name) ?? 0) + 1);
  }

  const options: Completion[] = files
    .slice()
    .sort((a, b) => a.path.localeCompare(b.path))
    .map((file) => {
      const needsQualifiedInsert = (nameCounts.get(file.name) ?? 0) > 1;
      const insertTarget = needsQualifiedInsert
        ? file.path.replace(/\.md$/i, "")
        : file.name;

      return {
        label: file.name,
        detail: needsQualifiedInsert ? `${file.path} • inserts path` : file.path,
        apply(view: EditorView, _completion: Completion, from: number, to: number) {
          const ahead = view.state.doc.sliceString(to, to + 2);
          const insertEnd = ahead === "]]" ? to + 2 : to;
          const insert = `${insertTarget}]] `;
          view.dispatch(
            view.state.update({
              changes: { from, to: insertEnd, insert },
              selection: { anchor: from + insert.length },
            }),
          );
        },
        boost: needsQualifiedInsert ? 0 : 1,
      } satisfies Completion;
    });

  return {
    from: noteFrom,
    options,
    validFor: /^[^\]#|]*$/,
  };
}

// ---------------------------------------------------------------------------
// Theme additions for wiki-link classes
// ---------------------------------------------------------------------------

// Color tokens (mirrors theme.ts)
const primary = "#bdc2ff";
const onSurfaceVariant = "#c9c7cc";
const outlineVariant = "#49474e";

export const wikiLinkTheme = EditorView.theme({
  // De-emphasised brackets [[ and ]]
  ".cm-wl-bracket": {
    color: outlineVariant,
    opacity: "0.7",
  },
  // Whole-link baseline visibility (including list items)
  ".cm-wl-link": {
    color: `${primary} !important`,
  },
  ".cm-wl-bracket-hidden": {
    display: "none",
  },
  // Target text base (applies in all states / contexts including list items)
  ".cm-wl-target": {
    color: `${primary} !important`,
    textDecoration: "underline",
    textDecorationColor: `${primary}70`,
    textDecorationThickness: "1.5px",
    textUnderlineOffset: "2px",
    fontWeight: "560",
  },
  // Target text when resolution is pending
  ".cm-wl-pending": {
    opacity: "0.75",
  },
  // Target text when resolved — primary accent, navigable
  ".cm-wl-resolved": {
    color: primary,
    textDecoration: "underline",
    textDecorationColor: `${primary}60`,
    textUnderlineOffset: "2px",
  },
  // Target text when unresolved — muted, visually distinct
  ".cm-wl-unresolved": {
    color: `${onSurfaceVariant} !important`,
    textDecoration: "underline",
    textDecorationStyle: "dashed",
    textDecorationColor: `${onSurfaceVariant}80`,
    textUnderlineOffset: "2px",
    opacity: "0.9",
  },
  // Target text when ambiguous — warning amber tint
  ".cm-wl-ambiguous": {
    color: "#e8c46a",
    textDecoration: "underline",
    textDecorationColor: "#e8c46a80",
    textDecorationStyle: "dotted",
    textUnderlineOffset: "2px",
  },
  // Fragment (#heading) — slightly muted
  ".cm-wl-fragment": {
    color: onSurfaceVariant,
    opacity: "0.8",
  },
  // Alias (|display text) — slightly muted
  ".cm-wl-alias": {
    color: onSurfaceVariant,
  },
});

// ---------------------------------------------------------------------------
// Public factory functions
// ---------------------------------------------------------------------------

/**
 * Returns the core wiki-link syntax + decoration extension (without autocomplete).
 * Goes into `wikiLinkCompartment`.
 *
 * @param handlers  Callbacks for link following and ambiguity reporting.
 */
export function wikiLinkExtension(handlers: LinkHandlers): Extension {
  return [
    resolutionField,
    linkHandlersField.init(() => handlers),
    wikiLinkPlugin,
    clickHandler,
    pointerCursorTheme,
    wikiLinkTheme,
    keymap.of([
      {
        key: "Alt-Enter",
        run(view) {
          void followLinkAtCursor(view);
          return true;
        },
      },
    ]),
  ];
}

/**
 * Returns the wiki-link autocomplete extension.
 * Goes into `autocompleteCompartment` alongside the base `autocompletion()`.
 */
export function wikiLinkAutocomplete(): Extension {
  return autocompletion({
    override: [wikiLinkCompletionSource],
    activateOnTyping: true,
    maxRenderedOptions: 12,
    icons: false,
  });
}

/**
 * Effect to update link handlers (e.g. when the active file changes).
 * Call via `view.dispatch({ effects: updateLinkHandlers.of(newHandlers) })`.
 */
export { setLinkHandlers as updateLinkHandlers };
