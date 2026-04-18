export interface NoteTemplateAction {
  id: string;
  label: string;
  detail: string;
  build: (context?: { existingPaths?: Iterable<string>; now?: Date }) => {
    path: string;
    content: string;
    successDetail: string;
  };
}

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

function slugFromTitle(value: string): string {
  return (
    value
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || "note"
  );
}

function flattenExistingPaths(existingPaths?: Iterable<string>): Set<string> {
  return new Set(existingPaths ?? []);
}

export function makeUniqueMarkdownPath(
  desiredPath: string,
  existingPaths?: Iterable<string>,
): string {
  const existing = flattenExistingPaths(existingPaths);
  if (!existing.has(desiredPath)) {
    return desiredPath;
  }

  const match = /^(.*?)(\.md)$/i.exec(desiredPath);
  const base = match?.[1] ?? desiredPath;
  const ext = match?.[2] ?? ".md";

  for (let index = 2; index < 1000; index += 1) {
    const candidate = `${base}-${index}${ext}`;
    if (!existing.has(candidate)) {
      return candidate;
    }
  }

  throw new Error(`Couldn't generate a unique path for ${desiredPath}`);
}

export function collectMarkdownPaths(
  tree: Array<{ path: string; is_dir: boolean; is_markdown: boolean; children?: unknown[] }>,
): string[] {
  const results: string[] = [];

  function visit(
    nodes: Array<{ path: string; is_dir: boolean; is_markdown: boolean; children?: unknown[] }>,
  ) {
    for (const node of nodes) {
      if (node.is_dir) {
        visit(Array.isArray(node.children) ? (node.children as typeof nodes) : []);
        continue;
      }

      if (node.is_markdown) {
        results.push(node.path);
      }
    }
  }

  visit(tree);
  return results;
}

function buildBlankNote(existingPaths?: Iterable<string>) {
  const path = makeUniqueMarkdownPath("Untitled Note.md", existingPaths);
  const title = path.replace(/\.md$/i, "");

  return {
    path,
    content: `# ${title}\n\n`,
    successDetail: "Created blank note from template",
  };
}

function buildDailyNote(now: Date, existingPaths?: Iterable<string>) {
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const path = makeUniqueMarkdownPath(`daily/${date}.md`, existingPaths);

  return {
    path,
    content: `# ${date}\n\n## Notes\n\n## Tasks\n- [ ] \n`,
    successDetail: "Created daily note from template",
  };
}

function buildMeetingNote(now: Date, existingPaths?: Iterable<string>) {
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}`;
  const title = `${date} Meeting`;
  const path = makeUniqueMarkdownPath(`meetings/${date}-${time}-meeting.md`, existingPaths);

  return {
    path,
    content:
      `# ${title}\n\n` +
      `## Attendees\n- \n\n` +
      `## Agenda\n- \n\n` +
      `## Notes\n\n` +
      `## Decisions\n- \n\n` +
      `## Action Items\n- [ ] \n`,
    successDetail: "Created meeting note from template",
  };
}

function buildScratchpad(now: Date, existingPaths?: Iterable<string>) {
  const date = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
  const time = `${pad(now.getHours())}-${pad(now.getMinutes())}`;
  const title = `${date} ${time} Scratchpad`;
  const path = makeUniqueMarkdownPath(
    `scratch/${date}-${time}-${slugFromTitle("scratchpad")}.md`,
    existingPaths,
  );

  return {
    path,
    content: `# ${title}\n\n`,
    successDetail: "Created scratchpad from template",
  };
}

export const noteTemplates: NoteTemplateAction[] = [
  {
    id: "template.blank-note",
    label: "Blank note",
    detail: "Create a clean markdown note at the vault root.",
    build: ({ existingPaths } = {}) => buildBlankNote(existingPaths),
  },
  {
    id: "template.daily-note",
    label: "Daily note",
    detail: "Create today's daily note with notes and tasks sections.",
    build: ({ existingPaths, now = new Date() } = {}) => buildDailyNote(now, existingPaths),
  },
  {
    id: "template.meeting-note",
    label: "Meeting note",
    detail: "Create a meeting note with agenda, notes, and action items.",
    build: ({ existingPaths, now = new Date() } = {}) => buildMeetingNote(now, existingPaths),
  },
  {
    id: "template.scratchpad",
    label: "Scratchpad",
    detail: "Create a timestamped scratch note for quick thinking.",
    build: ({ existingPaths, now = new Date() } = {}) => buildScratchpad(now, existingPaths),
  },
];
