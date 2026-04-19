import { auraStore } from "$lib/stores/aura.svelte";
import { editorNavigationStore } from "$lib/stores/editor-navigation.svelte";
import { editorStore } from "$lib/stores/editor.svelte";
import { embedStatusStore } from "$lib/stores/embedStatus.svelte";
import { filesStore, type TreeEntry } from "$lib/stores/files.svelte";
import { indexStatsStore } from "$lib/stores/indexStats.svelte";
import { operationStore } from "$lib/stores/operationStore.svelte";
import { pinnedStore } from "$lib/stores/pinned-notes.svelte";
import { workspaceStore } from "$lib/stores/workspace.svelte";
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
    // Switching vaults: any open tabs reference the previous vault's paths
    // and would be invalid here. First open (from null) keeps restored state.
    const switching = _path !== null && _path !== entry.path;
    _path = entry.path;
    _name = entry.name;
    if (switching) {
      // Tabs, editor status, in-flight suggestions, pinned notes, index
      // counts, and embed progress all describe the previous vault. Wipe
      // them to an empty baseline before resubscribing below.
      workspaceStore.resetPanes();
      editorStore.clearStatus();
      editorNavigationStore.clear();
      auraStore.dismiss();
      pinnedStore.reset();
      indexStatsStore.stop();
      embedStatusStore.stop();
      operationStore.stop();
    }

    // Start listening for external filesystem changes, then load initial tree.
    await filesStore.startWatching();
    await filesStore.refresh();

    // Drop any restored tabs pointing at files that no longer exist on disk
    // (manually deleted/moved while Tektite was closed).
    const knownPaths = new Set<string>();
    collectFilePaths(filesStore.tree, knownPaths);
    workspaceStore.pruneMissingFileTabs(knownPaths);

    // Subscribe to index stats updates.
    await indexStatsStore.start();

    // Subscribe to transient operation signals (agent runs).
    await operationStore.start();

    // Subscribe to semantic-index health + backlog progress.
    await embedStatusStore.start();

    // Load pinned notes for this vault, then drop any that reference
    // files no longer on disk.
    await pinnedStore.load();
    pinnedStore.pruneMissing(knownPaths);
  } catch (error) {
    _openError = error instanceof Error ? error.message : String(error);
    throw error;
  }
}

function collectFilePaths(entries: TreeEntry[], out: Set<string>) {
  for (const entry of entries) {
    if (!entry.is_dir) out.add(entry.path);
    if (entry.children.length > 0) collectFilePaths(entry.children, out);
  }
}

export async function getRecentVaults(): Promise<VaultEntry[]> {
  return await invoke<VaultEntry[]>("vault_get_recent");
}
