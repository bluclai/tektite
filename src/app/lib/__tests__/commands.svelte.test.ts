import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

type CommandsModule = typeof import("../stores/commands.svelte");

async function freshStore(): Promise<CommandsModule> {
  vi.resetModules();
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  return await import("../stores/commands.svelte");
}

describe("commandStore", () => {
  let mod: CommandsModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("seeds the core command registry", () => {
    const ids = mod.commandStore.commands.map((cmd) => cmd.id);

    expect(ids).toEqual([
      "panel.backlinks",
      "panel.files",
      "panel.search",
      "panel.unresolved",
      "sidebar.toggle",
    ]);
  });

  it("get returns a command by stable id", () => {
    const cmd = mod.commandStore.get("sidebar.toggle");
    expect(cmd?.label).toBe("Toggle Sidebar");
  });

  it("findByCategory returns matching commands", () => {
    const paneCommands = mod.commandStore.findByCategory("Pane");
    expect(paneCommands.map((cmd) => cmd.id)).toEqual([
      "panel.backlinks",
      "panel.files",
      "panel.search",
      "panel.unresolved",
    ]);
  });

  it("register adds a new command and unregister removes it", () => {
    mod.commandStore.register({
      id: "vault.open",
      label: "Open Vault",
      category: "Vault",
      action: () => {},
    });

    expect(mod.commandStore.get("vault.open")?.label).toBe("Open Vault");

    mod.commandStore.unregister("vault.open");
    expect(mod.commandStore.get("vault.open")).toBeUndefined();
  });

  it("register replaces an existing id instead of duplicating it", () => {
    mod.commandStore.register({
      id: "panel.files",
      label: "Jump to Files",
      category: "Pane",
      action: () => {},
    });

    const filesCommands = mod.commandStore.commands.filter((cmd) => cmd.id === "panel.files");
    expect(filesCommands).toHaveLength(1);
    expect(filesCommands[0].label).toBe("Jump to Files");
  });
});
