/**
 * Clay CM6 Theme
 *
 * Maps the Obsidian Monolith design tokens to a CodeMirror 6 EditorView theme.
 * All values are drawn directly from the CSS custom properties defined in
 * app.css — keeping a single source of truth for the palette.
 *
 * The theme is built as a static `Extension` so it can be included in the
 * ordered extension array alongside future compartments (syntax, wiki-links,
 * live preview decorations) without restructuring the base stack.
 */

import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags } from "@lezer/highlight";

// ---------------------------------------------------------------------------
// Color tokens (mirrors app.css :root)
// ---------------------------------------------------------------------------
const surface = "#131314";
const surfaceContainerLow = "#1a1a1b";
const surfaceContainerHigh = "#2a2a2b";
const onSurface = "#e8e8ea";
const onSurfaceVariant = "#c9c7cc";
const primary = "#bdc2ff";
const outline = "#9391a0";
const outlineVariant = "#49474e";

// ---------------------------------------------------------------------------
// Base editor theme
// ---------------------------------------------------------------------------
export const clayBaseTheme = EditorView.theme(
  {
    // The editor DOM wrapper — transparent so the Svelte parent controls bg
    "&": {
      height: "100%",
      background: surface,
      color: onSurface,
      fontFamily: '"Bitter", Georgia, serif',
      fontSize: "15px",
      lineHeight: "1.65",
    },
    // Scrollable content area — owns the prose column width.
    // max-width + auto margins centre the column; overflow-x hidden
    // prevents any horizontal scroll (lineWrapping handles the rest).
    ".cm-scroller": {
      fontFamily: "inherit",
      lineHeight: "inherit",
      overflowX: "hidden",
    },
    // Inner content column — constrained to a readable prose width.
    // The box-sizing trick lets padding be included in the max-width
    // budget so the text itself stays within ~72ch at 15px.
    ".cm-content": {
      boxSizing: "border-box",
      maxWidth: "860px",
      width: "100%",
      margin: "0 auto",
      padding: "24px 64px",
      caretColor: primary,
      wordBreak: "break-word",
      letterSpacing: "0.012em",
    },
    // Line elements must also not overflow.
    ".cm-line": {
      padding: "0",
    },
    // Cursor
    ".cm-cursor, .cm-dropCursor": {
      borderLeftColor: primary,
      borderLeftWidth: "2px",
    },
    // Selection
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground": {
      background: `${primary}22`,
    },
    "&.cm-focused .cm-selectionBackground": {
      background: `${primary}33`,
    },
    // Active line — very subtle tonal lift
    ".cm-activeLine": {
      backgroundColor: `${surfaceContainerLow}80`,
    },
    // Active line gutter number highlight
    ".cm-activeLineGutter": {
      backgroundColor: "transparent",
      color: onSurfaceVariant,
    },
    // Gutters (line numbers)
    ".cm-gutters": {
      background: surface,
      border: "none",
      color: outlineVariant,
      paddingRight: "8px",
      minWidth: "40px",
    },
    ".cm-lineNumbers .cm-gutterElement": {
      padding: "0 8px 0 4px",
      fontSize: "12px",
    },
    // Search match highlight
    ".cm-searchMatch": {
      backgroundColor: `${primary}30`,
      outline: `1px solid ${primary}60`,
    },
    ".cm-searchMatch.cm-searchMatch-selected": {
      backgroundColor: `${primary}50`,
    },
    // Matching bracket
    ".cm-matchingBracket": {
      backgroundColor: `${primary}20`,
      outline: `1px solid ${primary}60`,
    },
    // Fold placeholder
    ".cm-foldPlaceholder": {
      background: surfaceContainerHigh,
      border: "none",
      color: onSurfaceVariant,
      borderRadius: "3px",
      padding: "0 4px",
    },
    // Tooltip (autocomplete, lint)
    ".cm-tooltip": {
      background: surfaceContainerHigh,
      border: `1px solid ${outlineVariant}40`,
      borderRadius: "6px",
      boxShadow: "0 8px 32px rgba(0,0,0,0.24)",
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      background: `${primary}22`,
      color: onSurface,
    },
    // Panel (e.g. search panel at bottom)
    ".cm-panels": {
      background: surfaceContainerLow,
      borderTop: `1px solid ${outlineVariant}20`,
    },
    ".cm-panel": {
      padding: "8px 12px",
    },
    // Placeholder text
    ".cm-placeholder": {
      color: outline,
      fontStyle: "italic",
    },
  },
  { dark: true },
);

// ---------------------------------------------------------------------------
// Syntax highlight style
// ---------------------------------------------------------------------------
export const clayHighlightStyle = HighlightStyle.define([
  // Markdown headings — scale and weight
  {
    tag: tags.heading1,
    color: onSurface,
    fontFamily: '"Inter Variable", "Inter", sans-serif',
    fontWeight: "600",
    fontSize: "1.35em",
    lineHeight: "1.3",
  },
  {
    tag: tags.heading2,
    color: onSurface,
    fontFamily: '"Inter Variable", "Inter", sans-serif',
    fontWeight: "600",
    fontSize: "1.18em",
  },
  {
    tag: tags.heading3,
    color: onSurface,
    fontFamily: '"Inter Variable", "Inter", sans-serif',
    fontWeight: "600",
    fontSize: "1.06em",
  },
  {
    tag: [tags.heading4, tags.heading5, tags.heading6],
    color: onSurface,
    fontFamily: '"Inter Variable", "Inter", sans-serif',
    fontWeight: "500",
  },

  // Emphasis
  { tag: tags.emphasis, fontStyle: "italic", color: onSurface },
  { tag: tags.strong, fontWeight: "700", color: onSurface },
  { tag: tags.strikethrough, textDecoration: "line-through", color: onSurfaceVariant },

  // Links — primary accent
  {
    tag: tags.link,
    color: primary,
    textDecoration: "underline",
    textDecorationColor: `${primary}60`,
  },
  { tag: tags.url, color: primary },

  // Code
  {
    tag: tags.monospace,
    fontFamily: '"JetBrains Mono", "Fira Code", monospace',
    fontSize: "0.9em",
    color: "#c8b4ff",
  },

  // Blockquote / quote markers
  { tag: tags.quote, color: onSurfaceVariant, fontStyle: "italic" },

  // List markers
  { tag: tags.list, color: onSurfaceVariant },

  // HR / thematic break
  { tag: tags.contentSeparator, color: outlineVariant },

  // Comments (in code blocks)
  { tag: tags.comment, color: outline, fontStyle: "italic" },

  // Keywords, operators in code blocks
  { tag: tags.keyword, color: primary },
  { tag: tags.operator, color: onSurfaceVariant },
  { tag: tags.string, color: "#9ecbff" },
  { tag: tags.number, color: "#79c0ff" },
  { tag: tags.bool, color: primary },
  { tag: tags.null, color: outline },
  { tag: tags.typeName, color: "#cebdff" },
  { tag: tags.function(tags.variableName), color: "#d2a8ff" },

  // Frontmatter delimiters / meta
  { tag: tags.meta, color: outlineVariant },
  { tag: tags.processingInstruction, color: outlineVariant },
  { tag: tags.atom, color: "#cebdff" },

  // Generic punctuation — de-emphasise
  { tag: tags.punctuation, color: outlineVariant },
  { tag: tags.bracket, color: onSurfaceVariant },

  // Default text
  { tag: tags.content, color: onSurface },
]);

export const clayTheme = [clayBaseTheme, syntaxHighlighting(clayHighlightStyle)];
