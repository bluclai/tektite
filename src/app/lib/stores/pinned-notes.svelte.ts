/**
 * Pinned notes store.
 *
 * User-pinned note paths shown above the vault tree in the Files sidebar.
 * Persisted per-vault to `<vault>/.tektite/pinned.json` via the
 * `pinned_load`/`pinned_save` Tauri commands.
 */
import { invoke } from "@tauri-apps/api/core";

interface PinnedState {
  version: number;
  paths: string[];
}

const PINNED_VERSION = 1;

let _paths = $state<string[]>([]);
let _loaded = $state(false);

let _saveTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleSave() {
  if (_saveTimer !== null) clearTimeout(_saveTimer);
  _saveTimer = setTimeout(() => {
    invoke("pinned_save", {
      state: {
        version: PINNED_VERSION,
        paths: _paths,
      } satisfies PinnedState,
    }).catch(() => {});
    _saveTimer = null;
  }, 200);
}

export const pinnedStore = {
  get paths(): string[] {
    return _paths;
  },

  get loaded(): boolean {
    return _loaded;
  },

  has(path: string): boolean {
    return _paths.includes(path);
  },

  add(path: string) {
    if (_paths.includes(path)) return;
    _paths = [..._paths, path];
    scheduleSave();
  },

  remove(path: string) {
    if (!_paths.includes(path)) return;
    _paths = _paths.filter((p) => p !== path);
    scheduleSave();
  },

  toggle(path: string) {
    if (_paths.includes(path)) {
      this.remove(path);
    } else {
      this.add(path);
    }
  },

  /** Load pinned paths for the currently open vault. Call after vault open. */
  async load(): Promise<void> {
    try {
      const raw = await invoke<PinnedState>("pinned_load");
      if (raw && raw.version === PINNED_VERSION && Array.isArray(raw.paths)) {
        _paths = raw.paths.filter((p): p is string => typeof p === "string");
      } else {
        _paths = [];
      }
    } catch {
      _paths = [];
    } finally {
      _loaded = true;
    }
  },

  /** Drop pinned state — call on vault close. */
  reset() {
    _paths = [];
    _loaded = false;
  },

  /** Reflect a rename in pinned paths (keeps the pin if the file moves). */
  renamePath(oldPath: string, newPath: string) {
    const idx = _paths.indexOf(oldPath);
    if (idx === -1) return;
    const next = [..._paths];
    next[idx] = newPath;
    _paths = next;
    scheduleSave();
  },

  /** Drop any pinned entries that no longer exist in the provided path set. */
  pruneMissing(knownPaths: Set<string>) {
    const next = _paths.filter((p) => knownPaths.has(p));
    if (next.length !== _paths.length) {
      _paths = next;
      scheduleSave();
    }
  },
};
