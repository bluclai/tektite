function normalizeHeadingText(text: string): string {
  return text.replace(/\s+/g, " ").trim().toLowerCase();
}

function isTagBoundary(char: string | undefined): boolean {
  if (!char) return true;
  return !/[A-Za-z0-9_/-]/.test(char);
}

export function findMarkdownHeadingPosition(
  content: string,
  headingText: string,
  level?: number,
): number | null {
  const target = normalizeHeadingText(headingText);
  if (!target) return null;

  const lines = content.split("\n");
  let offset = 0;
  let fallback: number | null = null;

  for (const line of lines) {
    const match = /^ {0,3}(#{1,6})[ \t]+(.+?)\s*$/.exec(line);
    if (match) {
      const hashes = match[1];
      const rawText = match[2].replace(/[ \t]+#+\s*$/, "").trim();
      const normalized = normalizeHeadingText(rawText);

      if (normalized === target) {
        if (level === undefined || hashes.length === level) {
          return offset;
        }

        if (fallback === null) {
          fallback = offset;
        }
      }
    }

    offset += line.length + 1;
  }

  return fallback;
}

export function findMarkdownTagPosition(content: string, tagName: string): number | null {
  const target = tagName.trim().replace(/^#+/, "").toLowerCase();
  if (!target) return null;

  for (let index = 0; index < content.length; index += 1) {
    if (content[index] !== "#") continue;

    const start = index + 1;
    let end = start;
    while (end < content.length && /[A-Za-z0-9_/-]/.test(content[end])) {
      end += 1;
    }

    if (end === start) continue;

    const candidate = content.slice(start, end).toLowerCase();
    if (candidate !== target) continue;
    if (!isTagBoundary(content[index - 1])) continue;
    if (!isTagBoundary(content[end])) continue;

    return index;
  }

  return null;
}
