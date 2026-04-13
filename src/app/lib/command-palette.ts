import type { CommandAction } from "$lib/stores/commands.svelte";

export type PaletteMode = "files" | "commands" | "headings" | "tags" | "templates";

const PREFIX_TO_MODE: Record<string, PaletteMode> = {
  ">": "commands",
  "#": "headings",
  "@": "tags",
  "/": "templates",
};

const MODE_PLACEHOLDERS: Record<PaletteMode, string> = {
  files: "Search files...",
  commands: "Type a command...",
  headings: "Search headings...",
  tags: "Search tags...",
  templates: "Search templates...",
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
