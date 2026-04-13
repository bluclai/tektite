import { describe, expect, it } from "vitest";

import { findMarkdownHeadingPosition, findMarkdownTagPosition } from "../editor-navigation";

describe("findMarkdownHeadingPosition", () => {
  it("finds an exact heading by text and level", () => {
    const content = [
      "# Intro",
      "",
      "## Alpha",
      "Body",
      "### Beta",
      "More",
    ].join("\n");

    expect(findMarkdownHeadingPosition(content, "Alpha", 2)).toBe(9);
    expect(findMarkdownHeadingPosition(content, "Beta", 3)).toBe(23);
  });

  it("normalizes whitespace and trailing closing hashes", () => {
    const content = "##   My   Heading   ##\nText";
    expect(findMarkdownHeadingPosition(content, "my heading", 2)).toBe(0);
  });

  it("falls back to the first text match when level differs", () => {
    const content = ["## Shared", "", "### Shared", ""].join("\n");
    expect(findMarkdownHeadingPosition(content, "Shared", 4)).toBe(0);
  });

  it("returns null when the heading is absent", () => {
    expect(findMarkdownHeadingPosition("# Intro\n", "Missing", 1)).toBeNull();
  });
});

describe("findMarkdownTagPosition", () => {
  it("finds the first matching markdown tag", () => {
    const content = "hello #rust world\n#testing";
    expect(findMarkdownTagPosition(content, "rust")).toBe(6);
    expect(findMarkdownTagPosition(content, "testing")).toBe(18);
  });

  it("matches case-insensitively and ignores a leading hash in the query", () => {
    expect(findMarkdownTagPosition("Use #Rust here", "#rust")).toBe(4);
  });

  it("does not match partial tags inside larger tags or words", () => {
    const content = "#rustacean alpha#rust beta #rust";
    expect(findMarkdownTagPosition(content, "rust")).toBe(27);
  });

  it("returns null when the tag is absent", () => {
    expect(findMarkdownTagPosition("#rust", "go")).toBeNull();
  });
});
