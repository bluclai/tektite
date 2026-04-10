/**
 * File-tree store.
 *
 * Fetches the vault directory tree from the Rust backend and keeps it
 * reactive. The store is refreshed on demand (vault open, create, delete)
 * and whenever the backend emits a `file-tree-updated` event.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface TreeEntry {
  path: string;
  name: string;
  is_dir: boolean;
  children: TreeEntry[];
}

// ---------------------------------------------------------------------------
// Reactive state
// ---------------------------------------------------------------------------

let _tree = $state<TreeEntry[]>([]);
let _loading = $state(false);
let _unlistenFn: UnlistenFn | null = null;

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const filesStore = {
  get tree(): TreeEntry[] {
    return _tree;
  },
  get loading(): boolean {
    return _loading;
  },

  /** Fetch the tree from the backend and update state. */
  async refresh() {
    _loading = true;
    try {
      _tree = await invoke<TreeEntry[]>("files_get_tree");
    } catch {
      // Vault may not be open yet — silently ignore.
      _tree = [];
    } finally {
      _loading = false;
    }
  },

  /** Create a new markdown file at a vault-relative path and refresh. */
  async createFile(relPath: string): Promise<void> {
    await invoke<void>("files_create_file", { relPath });
    await filesStore.refresh();
  },

  /** Create a new folder at a vault-relative path and refresh. */
  async createFolder(relPath: string): Promise<void> {
    await invoke<void>("files_create_folder", { relPath });
    await filesStore.refresh();
  },

  /** Delete a file or folder at a vault-relative path and refresh. */
  async delete(relPath: string): Promise<void> {
    await invoke<void>("files_delete", { relPath });
    await filesStore.refresh();
  },

  /**
   * Subscribe to the backend `file-tree-updated` event.
   * Call once after vault open; returns an unlisten function.
   */
  async startWatching(): Promise<void> {
    // Cancel any existing listener before re-subscribing.
    if (_unlistenFn) {
      _unlistenFn();
      _unlistenFn = null;
    }
    _unlistenFn = await listen("file-tree-updated", () => {
      void filesStore.refresh();
    });
  },

  stopWatching() {
    if (_unlistenFn) {
      _unlistenFn();
      _unlistenFn = null;
    }
  },
};
