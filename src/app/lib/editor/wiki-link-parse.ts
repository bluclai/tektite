/**
 * wiki-link-parse.ts — Pure wiki-link parsing helpers.
 *
 * Extracted from wiki-link.ts so they can be unit-tested without CodeMirror
 * or Tauri dependencies.
 */

// ---------------------------------------------------------------------------
// Parsed wiki-link representation
// ---------------------------------------------------------------------------

export interface WikiLink {
  /** Start position of the opening `[[` */
  from: number;
  /** End position of the closing `]]` */
  to: number;
  /** Raw target text (before `#` or `|`) */
  target: string;
  /** Optional heading fragment (after `#`) */
  fragment: string | null;
  /** Optional display alias (after `|`) */
  alias: string | null;
  /** Full raw text inside the brackets (target + fragment + alias) */
  raw: string;
}

// ---------------------------------------------------------------------------
// Regex for wiki-link detection
// Captures: target (required), fragment (after #), alias (after |)
// ---------------------------------------------------------------------------

const WIKI_LINK_RE = /\[\[([^\]#|]+?)(?:#([^\]|]*?))?(?:\|([^\]]*?))?\]\]/g;

/** Parse all wiki-links in a string, returning their positions. */
export function parseWikiLinks(text: string, offset: number = 0): WikiLink[] {
  const links: WikiLink[] = [];
  WIKI_LINK_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = WIKI_LINK_RE.exec(text)) !== null) {
    links.push({
      from: offset + m.index,
      to: offset + m.index + m[0].length,
      target: m[1],
      fragment: m[2] ?? null,
      alias: m[3] ?? null,
      raw: m[0],
    });
  }
  return links;
}

/**
 * Derives the vault-relative path for a root-level note created from an
 * unresolved wiki-link target. Returns null for invalid/empty targets.
 */
export function rootNotePathForTarget(target: string): string | null {
  const trimmed = target.trim();
  if (!trimmed) return null;

  const base = trimmed.split("/").filter(Boolean).pop();
  if (!base || base === "." || base === "..") return null;

  return base.toLowerCase().endsWith(".md") ? base : `${base}.md`;
}

/**
 * Derives the initial `# Heading` content for a note created from an
 * unresolved wiki-link. Uses the link's base name (sans `.md`) so the
 * heading matches the file's displayed title in the sidebar.
 */
export function initialContentForTarget(target: string): string | null {
  const trimmed = target.trim();
  if (!trimmed) return null;

  const base = trimmed.split("/").filter(Boolean).pop();
  if (!base || base === "." || base === "..") return null;

  const title = base.replace(/\.md$/i, "").trim();
  if (!title) return null;

  return `# ${title}\n\n`;
}
