import { workspaceStore } from "$lib/stores/workspace.svelte";

/**
 * Emitted when the user triggers "Continue with Aura" from the command
 * palette. EditorPane listens on this global event and requests a
 * suggestion against its own view — the command store does not reach into
 * editor state directly so it can stay decoupled from CM6.
 */
export const AURA_CONTINUE_EVENT = "tektite:aura-continue";

export interface CommandAction {
  id: string;
  label: string;
  category?: string;
  shortcut?: string;
  action: () => void | Promise<void>;
}

let _commands = $state<CommandAction[]>([]);

function sortCommands(commands: CommandAction[]): CommandAction[] {
  return [...commands].sort((a, b) => a.label.localeCompare(b.label));
}

function ensureSeeded() {
  if (_commands.length > 0) return;
  _commands = sortCommands([
    {
      id: "sidebar.toggle",
      label: "Toggle Sidebar",
      category: "View",
      shortcut: "⌘B",
      action: () => workspaceStore.toggleSidebar(),
    },
    {
      id: "panel.files",
      label: "Go to Files panel",
      category: "Pane",
      action: () => workspaceStore.setActivePanel("files"),
    },
    {
      id: "panel.search",
      label: "Go to Search panel",
      category: "Pane",
      action: () => workspaceStore.setActivePanel("search"),
    },
    {
      id: "panel.backlinks",
      label: "Go to Backlinks panel",
      category: "Pane",
      action: () => workspaceStore.setActivePanel("backlinks"),
    },
    {
      id: "panel.unresolved",
      label: "Go to Unresolved Links panel",
      category: "Pane",
      action: () => workspaceStore.setActivePanel("unresolved"),
    },
    {
      id: "panel.graph",
      label: "Go to Graph panel",
      category: "Pane",
      action: () => {
        workspaceStore.setActivePanel("graph");
        workspaceStore.openSidebar();
      },
    },
    {
      id: "aura.continue",
      label: "Continue with Aura",
      category: "Aura",
      shortcut: "⌘/",
      action: () => {
        if (typeof window !== "undefined") {
          window.dispatchEvent(new CustomEvent(AURA_CONTINUE_EVENT));
        }
      },
    },
  ]);
}

ensureSeeded();

export const commandStore = {
  get commands(): CommandAction[] {
    return _commands;
  },

  register(cmd: CommandAction) {
    const existingIndex = _commands.findIndex((candidate) => candidate.id === cmd.id);
    if (existingIndex >= 0) {
      const next = [..._commands];
      next[existingIndex] = cmd;
      _commands = sortCommands(next);
      return;
    }

    _commands = sortCommands([..._commands, cmd]);
  },

  unregister(id: string) {
    _commands = _commands.filter((cmd) => cmd.id !== id);
  },

  get(id: string): CommandAction | undefined {
    return _commands.find((cmd) => cmd.id === id);
  },

  findByCategory(category: string): CommandAction[] {
    return _commands.filter((cmd) => cmd.category === category);
  },
};
