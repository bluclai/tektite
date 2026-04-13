import { describe, expect, it } from "vitest";

import {
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
  });

  it("returns mode-aware placeholder copy", () => {
    expect(placeholderForMode("files")).toBe("Search files...");
    expect(placeholderForMode("headings")).toBe("Search headings...");
    expect(placeholderForMode("templates")).toBe("Search templates...");
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

  it("formats palette errors safely", () => {
    expect(formatPaletteError(new Error("Boom"), "fallback")).toBe("Boom");
    expect(formatPaletteError("No vault open", "fallback")).toBe("No vault open");
    expect(formatPaletteError(null, "fallback")).toBe("fallback");
  });
});
