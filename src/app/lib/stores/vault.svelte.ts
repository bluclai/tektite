import { filesStore } from "$lib/stores/files.svelte";
import { indexStatsStore } from "$lib/stores/indexStats.svelte";
import { operationStore } from "$lib/stores/operationStore.svelte";
import { invoke } from "@tauri-apps/api/core";

export interface VaultEntry {
  path: string;
  name: string;
}

let _path = $state<string | null>(null);
let _name = $state<string>("");
let _openError = $state<string | null>(null);

export const vaultStore = {
  get path(): string | null {
    return _path;
  },
  get name(): string {
    return _name;
  },

  get openError(): string | null {
    return _openError;
  },

  clearOpenError() {
    _openError = null;
  },
};

export async function openVault(vaultPath: string): Promise<void> {
  _openError = null;
  try {
    const entry = await invoke<VaultEntry>("vault_open", { path: vaultPath });
    _path = entry.path;
    _name = entry.name;

    // Start listening for external filesystem changes, then load initial tree.
    await filesStore.startWatching();
    await filesStore.refresh();

    // Subscribe to index stats updates.
    await indexStatsStore.start();

    // Subscribe to transient operation signals (embed progress, agent runs).
    await operationStore.start();
  } catch (error) {
    _openError = error instanceof Error ? error.message : String(error);
    throw error;
  }
}

export async function getRecentVaults(): Promise<VaultEntry[]> {
  return await invoke<VaultEntry[]>("vault_get_recent");
}
