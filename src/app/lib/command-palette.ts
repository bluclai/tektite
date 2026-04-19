import type { CommandAction } from "$lib/stores/commands.svelte";

export type PaletteMode = "files" | "commands" | "headings" | "tags" | "templates" | "semantic";

const PREFIX_TO_MODE: Record<string, PaletteMode> = {
  ">": "commands",
  "#": "headings",
  "@": "tags",
  "/": "templates",
  "?": "semantic",
};

const MODE_PLACEHOLDERS: Record<PaletteMode, string> = {
  files: "Search files...",
  commands: "Type a command...",
  headings: "Search headings...",
  tags: "Search tags...",
  templates: "Search templates...",
  semantic: "Search by meaning…",
};

export function parsePaletteQuery(query: string): { mode: PaletteMode; searchTerm: string } {
  const prefix = query[0];
  const mode = PREFIX_TO_MODE[prefix] ?? "files";
  const searchTerm = mode === "files" ? query : query.slice(1).trimStart();

  return { mode, searchTerm };
}

export function placeholderForMode(mode: PaletteMode): string {
  return MODE_PLACEHOLDERS[mode];
}

export function filterCommands(commands: CommandAction[], searchTerm: string): CommandAction[] {
  const normalized = searchTerm.trim().toLowerCase();
  if (!normalized) return commands;

  return commands.filter((command) => {
    const haystacks = [command.label, command.id, command.category ?? ""];
    return haystacks.some((value) => value.toLowerCase().includes(normalized));
  });
}

export function formatPaletteError(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.trim().length > 0) {
    return error.message;
  }

  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }

  return fallback;
}

/**
 * Semantic search returns chunk-level hits; the palette shows one row per
 * file. This collapses consecutive hits by `file_path`, keeping the first
 * (highest-scoring) occurrence, then slices to `limit` so over-fetching at
 * the backend is always pared down to a fixed display count.
 */
export function dedupeSemanticHitsByFile<T extends { file_path: string }>(
  hits: readonly T[],
  limit: number,
): T[] {
  const seen = new Set<string>();
  const out: T[] = [];
  for (const hit of hits) {
    if (seen.has(hit.file_path)) continue;
    seen.add(hit.file_path);
    out.push(hit);
    if (out.length >= limit) break;
  }
  return out;
}
