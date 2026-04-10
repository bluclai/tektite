import { filesStore } from "$lib/stores/files.svelte";
import { invoke } from "@tauri-apps/api/core";

export interface VaultEntry {
  path: string;
  name: string;
}

let _path = $state<string | null>(null);
let _name = $state<string>("");

export const vaultStore = {
  get path(): string | null {
    return _path;
  },
  get name(): string {
    return _name;
  },
};

export async function openVault(vaultPath: string): Promise<void> {
  const entry = await invoke<VaultEntry>("vault_open", { path: vaultPath });
  _path = entry.path;
  _name = entry.name;

  // Start listening for external filesystem changes, then load initial tree.
  await filesStore.startWatching();
  await filesStore.refresh();
}

export async function getRecentVaults(): Promise<VaultEntry[]> {
  return await invoke<VaultEntry[]>("vault_get_recent");
}
