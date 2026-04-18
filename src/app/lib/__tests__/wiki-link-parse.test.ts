import { describe, it, expect } from "vitest";

import {
  parseWikiLinks,
  rootNotePathForTarget,
  initialContentForTarget,
} from "../editor/wiki-link-parse";

// ---------------------------------------------------------------------------
// parseWikiLinks
// ---------------------------------------------------------------------------

describe("parseWikiLinks", () => {
  it("parses a simple wiki-link", () => {
    const links = parseWikiLinks("Hello [[note]] world");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("note");
    expect(links[0].fragment).toBeNull();
    expect(links[0].alias).toBeNull();
    expect(links[0].from).toBe(6);
    expect(links[0].to).toBe(14);
    expect(links[0].raw).toBe("[[note]]");
  });

  it("parses a wiki-link with a heading fragment", () => {
    const links = parseWikiLinks("[[note#heading]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("note");
    expect(links[0].fragment).toBe("heading");
    expect(links[0].alias).toBeNull();
  });

  it("parses a wiki-link with a display alias", () => {
    const links = parseWikiLinks("[[note|display text]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("note");
    expect(links[0].fragment).toBeNull();
    expect(links[0].alias).toBe("display text");
  });

  it("parses a wiki-link with fragment and alias", () => {
    const links = parseWikiLinks("[[note#heading|display]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("note");
    expect(links[0].fragment).toBe("heading");
    expect(links[0].alias).toBe("display");
  });

  it("parses multiple wiki-links in one string", () => {
    const links = parseWikiLinks("[[alpha]] and [[beta#sec|display]]");
    expect(links).toHaveLength(2);
    expect(links[0].target).toBe("alpha");
    expect(links[1].target).toBe("beta");
    expect(links[1].fragment).toBe("sec");
    expect(links[1].alias).toBe("display");
  });

  it("returns empty array for text with no links", () => {
    expect(parseWikiLinks("Just plain text")).toEqual([]);
  });

  it("returns empty array for empty string", () => {
    expect(parseWikiLinks("")).toEqual([]);
  });

  it("does not match single brackets", () => {
    expect(parseWikiLinks("[not a link]")).toEqual([]);
  });

  it("handles triple brackets — matches innermost [[…]]", () => {
    // [[[note]]] — the regex matches [[[note]] with target [note
    // because the first `[[` starts at index 0 and `]]` ends at index 9
    const links = parseWikiLinks("[[[note]]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("[note");
  });

  it("handles path-qualified targets", () => {
    const links = parseWikiLinks("[[folder/subfolder/note]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("folder/subfolder/note");
  });

  it("respects the offset parameter", () => {
    const links = parseWikiLinks("[[note]]", 100);
    expect(links[0].from).toBe(100);
    expect(links[0].to).toBe(108);
  });

  it("parses adjacent links", () => {
    const links = parseWikiLinks("[[a]][[b]]");
    expect(links).toHaveLength(2);
    expect(links[0].target).toBe("a");
    expect(links[1].target).toBe("b");
  });

  // --- Edge cases for rejected / malformed patterns ---

  it("rejects empty target [[]]", () => {
    // The regex requires [^\]#|]+? (at least one char) in the target group
    expect(parseWikiLinks("[[]]")).toEqual([]);
  });

  it("rejects unclosed brackets [[note (no closing]])", () => {
    expect(parseWikiLinks("[[note")).toEqual([]);
  });

  it("rejects missing target with alias [[|alias]]", () => {
    // Target group requires at least one char; | is excluded from target
    expect(parseWikiLinks("[[|alias]]")).toEqual([]);
  });

  // --- Spaces and Unicode ---

  it("parses targets with spaces", () => {
    const links = parseWikiLinks("[[my note with spaces]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("my note with spaces");
  });

  it("parses Unicode / non-ASCII targets", () => {
    const links = parseWikiLinks("[[café]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("café");
  });

  // --- Newlines ---

  it("parses wiki-links containing newlines (current regex behavior)", () => {
    // [^\]#|] matches \n, so [[foo\nbar]] currently parses.
    // Pinning this behavior so we catch if the regex changes.
    const links = parseWikiLinks("[[foo\nbar]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("foo\nbar");
  });

  // --- Empty fragment / alias ---

  it("parses [[note#]] with empty fragment", () => {
    const links = parseWikiLinks("[[note#]]");
    // Fragment capture is [^\]|]*? — zero-or-more. The # is consumed,
    // so target is "note" and fragment is empty string (not null).
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("note");
    // fragment is empty string (matched but zero-length)
    expect(links[0].fragment).toBe("");
    expect(links[0].alias).toBeNull();
  });

  it("parses [[note|]] with empty alias", () => {
    const links = parseWikiLinks("[[note|]]");
    expect(links).toHaveLength(1);
    expect(links[0].target).toBe("note");
    expect(links[0].fragment).toBeNull();
    // alias is empty string (matched but zero-length)
    expect(links[0].alias).toBe("");
  });

  // --- Offset + multi-link ---

  it("offset is consistent across multiple matches", () => {
    const text = "See [[alpha]] and [[beta#sec|display]]";
    const links = parseWikiLinks(text, 50);
    expect(links).toHaveLength(2);
    // from/to should be offset positions that match the source string
    expect(text.indexOf("[[alpha]]") + 50).toBe(links[0].from);
    expect(links[0].from).toBe(50 + 4); // "See " is 4 chars
    expect(links[1].from).toBe(50 + 18); // "See [[alpha]] and " is 18 chars
  });
});

// ---------------------------------------------------------------------------
// rootNotePathForTarget
// ---------------------------------------------------------------------------

describe("rootNotePathForTarget", () => {
  it("appends .md to a simple name", () => {
    expect(rootNotePathForTarget("My Note")).toBe("My Note.md");
  });

  it("does not double-append .md", () => {
    expect(rootNotePathForTarget("My Note.md")).toBe("My Note.md");
  });

  it("does not double-append .MD (case-insensitive)", () => {
    expect(rootNotePathForTarget("My Note.MD")).toBe("My Note.MD");
  });

  it("extracts basename from a path-qualified target", () => {
    expect(rootNotePathForTarget("folder/My Note")).toBe("My Note.md");
  });

  it("returns null for empty string", () => {
    expect(rootNotePathForTarget("")).toBeNull();
  });

  it("returns null for whitespace-only string", () => {
    expect(rootNotePathForTarget("   ")).toBeNull();
  });

  it("returns null for .", () => {
    expect(rootNotePathForTarget(".")).toBeNull();
  });

  it("returns null for ..", () => {
    expect(rootNotePathForTarget("..")).toBeNull();
  });

  it("returns null for path ending in .", () => {
    expect(rootNotePathForTarget("folder/.")).toBeNull();
  });

  it("returns null for path ending in ..", () => {
    expect(rootNotePathForTarget("folder/..")).toBeNull();
  });

  it("trims whitespace from the target", () => {
    expect(rootNotePathForTarget("  Note  ")).toBe("Note.md");
  });

  // --- Asymmetry edge cases ---

  it("rootNotePathForTarget('.md') returns '.md' (bare extension, no stem)", () => {
    // .md alone — basename is ".md", which is truthy, so it's returned as-is.
    // This is a known asymmetry: initialContentForTarget('.md') returns null.
    expect(rootNotePathForTarget(".md")).toBe(".md");
  });

  it("initialContentForTarget('.md') returns null (title becomes empty)", () => {
    // Stripping .md from ".md" yields "", which fails the !title check.
    expect(initialContentForTarget(".md")).toBeNull();
  });

  it("rootNotePathForTarget('folder/.md') — basename is .md, returns .md", () => {
    expect(rootNotePathForTarget("folder/.md")).toBe(".md");
  });

  it("initialContentForTarget('folder/.md') returns null", () => {
    expect(initialContentForTarget("folder/.md")).toBeNull();
  });

  it("backslashes in target are treated as part of the name (no Windows path support)", () => {
    // Wiki-links use / as path separator; backslashes are just characters.
    // rootNotePathForTarget splits on /, so folder\note becomes the basename.
    expect(rootNotePathForTarget(String.raw`folder\note`)).toBe(String.raw`folder\note.md`);
  });
});

// ---------------------------------------------------------------------------
// initialContentForTarget
// ---------------------------------------------------------------------------

describe("initialContentForTarget", () => {
  it("creates heading from a simple name", () => {
    expect(initialContentForTarget("My Note")).toBe("# My Note\n\n");
  });

  it("strips .md extension from the heading", () => {
    expect(initialContentForTarget("My Note.md")).toBe("# My Note\n\n");
  });

  it("strips .MD extension (case-insensitive)", () => {
    expect(initialContentForTarget("My Note.MD")).toBe("# My Note\n\n");
  });

  it("extracts basename from a path-qualified target", () => {
    expect(initialContentForTarget("folder/My Note")).toBe("# My Note\n\n");
  });

  it("returns null for empty string", () => {
    expect(initialContentForTarget("")).toBeNull();
  });

  it("returns null for whitespace-only string", () => {
    expect(initialContentForTarget("   ")).toBeNull();
  });

  it("returns null for .", () => {
    expect(initialContentForTarget(".")).toBeNull();
  });

  it("returns null for ..", () => {
    expect(initialContentForTarget("..")).toBeNull();
  });

  it("trims whitespace from the target", () => {
    expect(initialContentForTarget("  Note  ")).toBe("# Note\n\n");
  });

  it("handles path-qualified target with .md", () => {
    expect(initialContentForTarget("daily/journal.md")).toBe("# journal\n\n");
  });
});
