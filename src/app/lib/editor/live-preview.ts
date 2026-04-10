/**
 * live-preview.ts — Phase 10 live preview decorations.
 *
 * This is intentionally lightweight: it keeps CM6 fully editable while
 * layering presentational decorations on top of source markdown.
 *
 * We style common markdown constructs inline:
 * - headings (line-level hierarchy)
 * - blockquotes
 * - list markers + task checkboxes
 * - inline code
 * - strong / strikethrough spans
 *
 * No replacement widgets are used, so cursor movement and editing remain
 * source-first and predictable.
 */

import type { Extension } from "@codemirror/state";

import { RangeSetBuilder } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
} from "@codemirror/view";

// ---------------------------------------------------------------------------
// Regex helpers (line-oriented + inline token accents)
// ---------------------------------------------------------------------------

const HEADING_RE = /^(\s{0,3})(#{1,6})(\s+)(.*)$/;
const BLOCKQUOTE_RE = /^(\s{0,3}>)(\s?)/;
const LIST_RE = /^(\s*)([-*+]|\d+[.)])(\s+)/;
const TASK_RE = /\[(?: |x|X)\]/g;

const INLINE_CODE_RE = /`[^`\n]+`/g;
const STRONG_RE = /\*\*[^*\n]+\*\*|__[^_\n]+__/g;
const STRIKE_RE = /~~[^~\n]+~~/g;
const WIKILINK_RE = /\[\[[^\]\n]+?\]\]/g;

const hideMarker = Decoration.mark({ class: "cm-lp-hidden-marker" });

function buildDecorations(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const doc = view.state.doc;

  const cursor = view.state.selection.main.head;

  for (const { from, to } of view.visibleRanges) {
    let line = doc.lineAt(from);

    while (line.from <= to) {
      const text = line.text;

      // --- Heading line classes + marker accent ---
      const heading = text.match(HEADING_RE);
      if (heading) {
        const level = Math.min(6, heading[2].length);
        builder.add(line.from, line.from, Decoration.line({ class: `cm-lp-heading cm-lp-h${level}` }));

        const hashFrom = line.from + heading[1].length;
        const markerTo = hashFrom + heading[2].length + heading[3].length;
        builder.add(hashFrom, markerTo, hideMarker);
      }

      // --- Blockquote ---
      const quote = text.match(BLOCKQUOTE_RE);
      if (quote) {
        builder.add(line.from, line.from, Decoration.line({ class: "cm-lp-blockquote" }));
        const markerFrom = line.from + quote[1].length - 1;
        const markerTo = line.from + quote[0].length;
        builder.add(markerFrom, markerTo, hideMarker);
      }

      // --- List markers ---
      const list = text.match(LIST_RE);
      if (list) {
        builder.add(line.from, line.from, Decoration.line({ class: "cm-lp-list-line" }));
        const markerFrom = line.from + list[1].length;
        const markerTo = markerFrom + list[2].length + list[3].length;
        builder.add(markerFrom, markerTo, hideMarker);
      }

      // --- Task checkboxes ---
      TASK_RE.lastIndex = 0;
      let taskMatch: RegExpExecArray | null;
      while ((taskMatch = TASK_RE.exec(text)) !== null) {
        builder.add(
          line.from + taskMatch.index,
          line.from + taskMatch.index + taskMatch[0].length,
          Decoration.mark({ class: "cm-lp-task-box" }),
        );
      }

      // --- Inline code ---
      INLINE_CODE_RE.lastIndex = 0;
      let codeMatch: RegExpExecArray | null;
      while ((codeMatch = INLINE_CODE_RE.exec(text)) !== null) {
        builder.add(
          line.from + codeMatch.index,
          line.from + codeMatch.index + codeMatch[0].length,
          Decoration.mark({ class: "cm-lp-inline-code" }),
        );
      }

      // --- Strong ---
      STRONG_RE.lastIndex = 0;
      let strongMatch: RegExpExecArray | null;
      while ((strongMatch = STRONG_RE.exec(text)) !== null) {
        builder.add(
          line.from + strongMatch.index,
          line.from + strongMatch.index + strongMatch[0].length,
          Decoration.mark({ class: "cm-lp-strong" }),
        );
      }

      // --- Strikethrough ---
      STRIKE_RE.lastIndex = 0;
      let strikeMatch: RegExpExecArray | null;
      while ((strikeMatch = STRIKE_RE.exec(text)) !== null) {
        builder.add(
          line.from + strikeMatch.index,
          line.from + strikeMatch.index + strikeMatch[0].length,
          Decoration.mark({ class: "cm-lp-strike" }),
        );
      }

      // --- Wiki-link brackets ---
      // Hide [[ and ]] only once the link is complete and the cursor has
      // moved outside the link (e.g. after pressing space to continue typing).
      WIKILINK_RE.lastIndex = 0;
      let wikiMatch: RegExpExecArray | null;
      while ((wikiMatch = WIKILINK_RE.exec(text)) !== null) {
        const absFrom = line.from + wikiMatch.index;
        const absTo = absFrom + wikiMatch[0].length;
        const trailing = text[wikiMatch.index + wikiMatch[0].length] ?? "";
        const completed = trailing === "" || /\s/.test(trailing);
        const cursorInside = cursor >= absFrom && cursor <= absTo;
        if (!completed || cursorInside) continue;

        builder.add(absFrom, absFrom + 2, hideMarker); // [[
        builder.add(absTo - 2, absTo, hideMarker); // ]]
      }

      if (line.to + 1 > to) break;
      line = doc.lineAt(line.to + 1);
    }
  }

  return builder.finish();
}

const livePreviewPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildDecorations(view);
    }

    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildDecorations(update.view);
      }
    }
  },
  {
    decorations: (v) => v.decorations,
  },
);

export const livePreviewTheme = EditorView.theme({
  ".cm-lp-hidden-marker": {
    display: "none",
  },
});

/**
 * Live preview extension for `livePreviewCompartment`.
 *
 * Keep this composable so source mode can turn it on/off via reconfigure.
 */
export function livePreviewExtension(): Extension {
  return [
    // Lets us style the whole editor differently while preview is enabled.
    EditorView.editorAttributes.of({ class: "cm-live-preview" }),
    livePreviewPlugin,
    livePreviewTheme,
  ];
}
