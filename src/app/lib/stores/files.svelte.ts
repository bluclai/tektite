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
  is_markdown: boolean;
  children: TreeEntry[];
}

// ---------------------------------------------------------------------------
// Reactive state
// ---------------------------------------------------------------------------

let _tree = $state<TreeEntry[]>([]);
let _loading = $state(false);
let _error = $state<string | null>(null);
let _unlistenFn: UnlistenFn | null = null;
let _refreshToken = 0;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function normalizeError(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) {
    return error.message;
  }

  if (typeof error === "string" && error.trim().length > 0) {
    return error;
  }

  return fallback;
}

function applyTree(tree: TreeEntry[]) {
  _tree = tree;
  _error = null;
}

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
  get error(): string | null {
    return _error;
  },

  clearError() {
    _error = null;
  },

  /** Fetch the tree from the backend and update state. */
  async refresh() {
    const refreshToken = ++_refreshToken;
    _loading = true;

    try {
      const tree = await invoke<TreeEntry[]>("files_get_tree");
      if (refreshToken === _refreshToken) {
        applyTree(tree);
      }
    } catch (error) {
      if (refreshToken !== _refreshToken) {
        return;
      }

      const message = normalizeError(error, "Failed to load files.");
      if (message === "No vault open") {
        _tree = [];
        _error = null;
      } else {
        _error = message;
      }
    } finally {
      if (refreshToken === _refreshToken) {
        _loading = false;
      }
    }
  },

  /** Create a new markdown file at a vault-relative path and apply the backend tree snapshot. */
  async createFile(relPath: string): Promise<void> {
    const tree = await invoke<TreeEntry[]>("files_create_file", { relPath });
    applyTree(tree);
  },

  /** Create a new folder at a vault-relative path and apply the backend tree snapshot. */
  async createFolder(relPath: string): Promise<void> {
    const tree = await invoke<TreeEntry[]>("files_create_folder", { relPath });
    applyTree(tree);
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
