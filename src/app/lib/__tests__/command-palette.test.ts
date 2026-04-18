import { describe, expect, it } from "vitest";

import {
  dedupeSemanticHitsByFile,
  filterCommands,
  formatPaletteError,
  parsePaletteQuery,
  placeholderForMode,
} from "../command-palette";

describe("command palette helpers", () => {
  it("detects files mode by default", () => {
    expect(parsePaletteQuery("meeting notes")).toEqual({
      mode: "files",
      searchTerm: "meeting notes",
    });
  });

  it("detects prefixed palette modes and trims the search term", () => {
    expect(parsePaletteQuery(">   sidebar")).toEqual({
      mode: "commands",
      searchTerm: "sidebar",
    });

    expect(parsePaletteQuery("# heading")).toEqual({
      mode: "headings",
      searchTerm: "heading",
    });

    expect(parsePaletteQuery("@ tag")).toEqual({
      mode: "tags",
      searchTerm: "tag",
    });

    expect(parsePaletteQuery("/ template")).toEqual({
      mode: "templates",
      searchTerm: "template",
    });

    expect(parsePaletteQuery("? auth flow")).toEqual({
      mode: "semantic",
      searchTerm: "auth flow",
    });

    // `?` with no search term — should still switch modes.
    expect(parsePaletteQuery("?")).toEqual({
      mode: "semantic",
      searchTerm: "",
    });
  });

  it("returns mode-aware placeholder copy", () => {
    expect(placeholderForMode("files")).toBe("Search files...");
    expect(placeholderForMode("headings")).toBe("Search headings...");
    expect(placeholderForMode("templates")).toBe("Search templates...");
    expect(placeholderForMode("semantic")).toBe("Search by meaning…");
  });

  it("filters commands by label, id, and category", () => {
    const commands = [
      {
        id: "sidebar.toggle",
        label: "Toggle Sidebar",
        category: "View",
        action: () => {},
      },
      {
        id: "panel.unresolved",
        label: "Go to Unresolved Links panel",
        category: "Pane",
        action: () => {},
      },
    ];

    expect(filterCommands(commands, "toggle").map((command) => command.id)).toEqual([
      "sidebar.toggle",
    ]);
    expect(filterCommands(commands, "panel.unresolved").map((command) => command.id)).toEqual([
      "panel.unresolved",
    ]);
    expect(filterCommands(commands, "pane").map((command) => command.id)).toEqual([
      "panel.unresolved",
    ]);
  });

  it("dedupes semantic hits by file and keeps first-seen", () => {
    const hits = [
      { file_path: "a.md", chunk_id: "a1", score: 0.9 },
      { file_path: "a.md", chunk_id: "a2", score: 0.8 }, // dropped — a.md already taken
      { file_path: "b.md", chunk_id: "b1", score: 0.7 },
      { file_path: "c.md", chunk_id: "c1", score: 0.6 },
      { file_path: "b.md", chunk_id: "b2", score: 0.5 }, // dropped
      { file_path: "d.md", chunk_id: "d1", score: 0.4 },
    ];

    const out = dedupeSemanticHitsByFile(hits, 10);
    expect(out.map((h) => h.chunk_id)).toEqual(["a1", "b1", "c1", "d1"]);
  });

  it("slices deduped semantic hits to the limit", () => {
    const hits = Array.from({ length: 25 }, (_, i) => ({
      file_path: `f${i}.md`,
      chunk_id: `c${i}`,
    }));

    expect(dedupeSemanticHitsByFile(hits, 10)).toHaveLength(10);
    expect(dedupeSemanticHitsByFile(hits, 10)[0].chunk_id).toBe("c0");
  });

  it("formats palette errors safely", () => {
    expect(formatPaletteError(new Error("Boom"), "fallback")).toBe("Boom");
    expect(formatPaletteError("No vault open", "fallback")).toBe("No vault open");
    expect(formatPaletteError(null, "fallback")).toBe("fallback");
  });
});
