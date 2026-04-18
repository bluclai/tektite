import { describe, expect, it } from "vitest";

import { collectMarkdownPaths, makeUniqueMarkdownPath, noteTemplates } from "../note-templates";

describe("makeUniqueMarkdownPath", () => {
  it("returns the desired path when unused", () => {
    expect(makeUniqueMarkdownPath("Untitled Note.md", ["other.md"])).toBe("Untitled Note.md");
  });

  it("adds numeric suffixes when needed", () => {
    expect(
      makeUniqueMarkdownPath("Untitled Note.md", ["Untitled Note.md", "Untitled Note-2.md"]),
    ).toBe("Untitled Note-3.md");
  });
});

describe("collectMarkdownPaths", () => {
  it("walks nested tree nodes and returns markdown file paths", () => {
    const tree = [
      {
        path: "daily",
        is_dir: true,
        is_markdown: false,
        children: [
          {
            path: "daily/2026-04-13.md",
            is_dir: false,
            is_markdown: true,
            children: [],
          },
        ],
      },
      {
        path: "assets/logo.png",
        is_dir: false,
        is_markdown: false,
        children: [],
      },
    ];

    expect(collectMarkdownPaths(tree)).toEqual(["daily/2026-04-13.md"]);
  });
});

describe("noteTemplates", () => {
  const now = new Date(2026, 3, 13, 9, 42, 0);

  it("builds a daily note in the daily folder", () => {
    const daily = noteTemplates.find((template) => template.id === "template.daily-note");
    const built = daily?.build({ now, existingPaths: [] });

    expect(built).toEqual({
      path: "daily/2026-04-13.md",
      content: "# 2026-04-13\n\n## Notes\n\n## Tasks\n- [ ] \n",
      successDetail: "Created daily note from template",
    });
  });

  it("builds a unique meeting note path when one already exists", () => {
    const meeting = noteTemplates.find((template) => template.id === "template.meeting-note");
    const built = meeting?.build({
      now,
      existingPaths: ["meetings/2026-04-13-0942-meeting.md"],
    });

    expect(built?.path).toBe("meetings/2026-04-13-0942-meeting-2.md");
    expect(built?.content).toContain("## Action Items");
  });

  it("builds a scratchpad in the scratch folder", () => {
    const scratch = noteTemplates.find((template) => template.id === "template.scratchpad");
    const built = scratch?.build({ now, existingPaths: [] });

    expect(built?.path).toBe("scratch/2026-04-13-09-42-scratchpad.md");
    expect(built?.content).toContain("# 2026-04-13 09-42 Scratchpad");
  });
});
