import { invoke } from "@tauri-apps/api/core";

// ---------------------------------------------------------------------------
// Panel (sidebar)
// ---------------------------------------------------------------------------

export type Panel = "files" | "search" | "backlinks" | "settings";

// ---------------------------------------------------------------------------
// Pane types
// ---------------------------------------------------------------------------

export interface PaneTab {
  id: string;
  path: string;
  /** Filename extracted from path for display */
  name: string;
}

export interface LeafPane {
  type: "leaf";
  id: string;
  tabs: PaneTab[];
  activeTabId: string | null;
}

export interface SplitPane {
  type: "split";
  id: string;
  direction: "horizontal" | "vertical";
  a: PaneLayout;
  b: PaneLayout;
  /** Percentages [aSize, bSize]; should sum to 100 */
  sizes: [number, number];
}

export type PaneLayout = LeafPane | SplitPane;

// ---------------------------------------------------------------------------
// Factory helpers
// ---------------------------------------------------------------------------

function nameFromPath(path: string): string {
  return path.split("/").pop() ?? path.split("\\").pop() ?? path;
}

function makeTab(path: string): PaneTab {
  return { id: crypto.randomUUID(), path, name: nameFromPath(path) };
}

function makeLeaf(): LeafPane {
  return { type: "leaf", id: crypto.randomUUID(), tabs: [], activeTabId: null };
}

// ---------------------------------------------------------------------------
// Pure transforms over LeafPane
// ---------------------------------------------------------------------------

function leafOpenTab(pane: LeafPane, path: string): LeafPane {
  const existing = pane.tabs.find((t) => t.path === path);
  if (existing) return { ...pane, activeTabId: existing.id };
  const tab = makeTab(path);
  return { ...pane, tabs: [...pane.tabs, tab], activeTabId: tab.id };
}

function leafCloseTab(pane: LeafPane, tabId: string): LeafPane {
  const idx = pane.tabs.findIndex((t) => t.id === tabId);
  if (idx === -1) return pane;
  const tabs = pane.tabs.filter((t) => t.id !== tabId);
  let activeTabId = pane.activeTabId;
  if (activeTabId === tabId) {
    const next = tabs[idx] ?? tabs[idx - 1] ?? null;
    activeTabId = next?.id ?? null;
  }
  return { ...pane, tabs, activeTabId };
}

function leafActivateTab(pane: LeafPane, tabId: string): LeafPane {
  return { ...pane, activeTabId: tabId };
}

// ---------------------------------------------------------------------------
// Pure transforms over PaneLayout
// ---------------------------------------------------------------------------

/** Apply an updater to the matching leaf. Returns new tree (structurally shared). */
function mapLeaf(
  layout: PaneLayout,
  paneId: string,
  updater: (p: LeafPane) => LeafPane,
): PaneLayout {
  if (layout.type === "leaf") {
    return layout.id === paneId ? updater(layout) : layout;
  }
  const a = mapLeaf(layout.a, paneId, updater);
  const b = mapLeaf(layout.b, paneId, updater);
  if (a === layout.a && b === layout.b) return layout;
  return { ...layout, a, b };
}

/** ID of the leftmost leaf — used as fallback active pane. */
function firstLeafId(layout: PaneLayout): string {
  if (layout.type === "leaf") return layout.id;
  return firstLeafId(layout.a);
}

/** Collect all leaf panes in left-to-right order. */
export function allLeaves(layout: PaneLayout): LeafPane[] {
  if (layout.type === "leaf") return [layout];
  return [...allLeaves(layout.a), ...allLeaves(layout.b)];
}

/**
 * Split the target leaf into a SplitPane. Returns [newTree, newLeafId].
 * Returns null if the target was not found.
 */
function splitLayout(
  layout: PaneLayout,
  targetId: string,
  direction: "horizontal" | "vertical",
): [PaneLayout, string] | null {
  if (layout.type === "leaf") {
    if (layout.id !== targetId) return null;
    const newLeaf = makeLeaf();
    const split: SplitPane = {
      type: "split",
      id: crypto.randomUUID(),
      direction,
      a: layout,
      b: newLeaf,
      sizes: [50, 50],
    };
    return [split, newLeaf.id];
  }
  const resA = splitLayout(layout.a, targetId, direction);
  if (resA) return [{ ...layout, a: resA[0] }, resA[1]];
  const resB = splitLayout(layout.b, targetId, direction);
  if (resB) return [{ ...layout, b: resB[0] }, resB[1]];
  return null;
}

/**
 * Remove a pane from the tree, collapsing the parent SplitPane into the
 * surviving sibling. Returns null if the whole tree was the removed pane.
 */
function removePane(layout: PaneLayout, paneId: string): PaneLayout | null {
  if (layout.type === "leaf") {
    return layout.id === paneId ? null : layout;
  }
  const a = removePane(layout.a, paneId);
  const b = removePane(layout.b, paneId);
  if (a === null) return b;
  if (b === null) return a;
  return { ...layout, a, b };
}

/** Update sizes on a specific SplitPane by its ID. */
function resizeSplitInTree(
  layout: PaneLayout,
  splitId: string,
  sizes: [number, number],
): PaneLayout {
  if (layout.type === "leaf") return layout;
  if (layout.id === splitId) return { ...layout, sizes };
  const a = resizeSplitInTree(layout.a, splitId, sizes);
  const b = resizeSplitInTree(layout.b, splitId, sizes);
  if (a === layout.a && b === layout.b) return layout;
  return { ...layout, a, b };
}

function renamePathValue(path: string, oldPath: string, newPath: string): string {
  if (path === oldPath) {
    return newPath;
  }

  const oldPrefix = `${oldPath}/`;
  if (path.startsWith(oldPrefix)) {
    return `${newPath}/${path.slice(oldPrefix.length)}`;
  }

  return path;
}

function renamePathsInTree(layout: PaneLayout, oldPath: string, newPath: string): PaneLayout {
  if (layout.type === "leaf") {
    let changed = false;
    const tabs = layout.tabs.map((tab) => {
      const nextPath = renamePathValue(tab.path, oldPath, newPath);
      if (nextPath === tab.path) {
        return tab;
      }

      changed = true;
      return {
        ...tab,
        path: nextPath,
        name: nameFromPath(nextPath),
      };
    });

    return changed ? { ...layout, tabs } : layout;
  }

  const a = renamePathsInTree(layout.a, oldPath, newPath);
  const b = renamePathsInTree(layout.b, oldPath, newPath);
  if (a === layout.a && b === layout.b) return layout;
  return { ...layout, a, b };
}

// ---------------------------------------------------------------------------
// Workspace persistence shape (version-guarded)
// ---------------------------------------------------------------------------

export interface WorkspaceState {
  version: number;
  activePanel: Panel;
  sidebarOpen: boolean;
  sidebarWidth: number;
  activePaneId: string;
  paneTree: PaneLayout;
}

const WORKSPACE_VERSION = 1;
const SIDEBAR_DEFAULT_WIDTH = 240;
const SIDEBAR_MIN_WIDTH = 180;
const SIDEBAR_MAX_WIDTH = 480;
const MIN_PANE_PCT = 15; // minimum percentage per pane side

// ---------------------------------------------------------------------------
// Reactive state
// ---------------------------------------------------------------------------

const _initialLeaf = makeLeaf();

let _activePanel = $state<Panel>("files");
let _sidebarOpen = $state<boolean>(true);
let _sidebarWidth = $state<number>(SIDEBAR_DEFAULT_WIDTH);
let _activePaneId = $state<string>(_initialLeaf.id);
let _paneTree = $state<PaneLayout>(_initialLeaf);

// Memoized lookup of the active leaf's active tab path. Recomputes only
// when _paneTree or _activePaneId change rather than on every getter read.
const _activeFilePath = $derived.by<string | null>(() => {
  const leaf = allLeaves(_paneTree).find((l) => l.id === _activePaneId) ?? null;
  if (!leaf || !leaf.activeTabId) return null;
  return leaf.tabs.find((t) => t.id === leaf.activeTabId)?.path ?? null;
});

// ---------------------------------------------------------------------------
// Debounced persistence
// ---------------------------------------------------------------------------

let _saveTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleSave() {
  if (_saveTimer !== null) clearTimeout(_saveTimer);
  _saveTimer = setTimeout(() => {
    invoke("workspace_save", {
      state: {
        version: WORKSPACE_VERSION,
        activePanel: _activePanel,
        sidebarOpen: _sidebarOpen,
        sidebarWidth: _sidebarWidth,
        activePaneId: _activePaneId,
        paneTree: _paneTree,
      } satisfies WorkspaceState,
    }).catch(() => {});
    _saveTimer = null;
  }, 400);
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const workspaceStore = {
  // --- Sidebar ---
  get activePanel() {
    return _activePanel;
  },
  get sidebarOpen() {
    return _sidebarOpen;
  },
  get sidebarWidth() {
    return _sidebarWidth;
  },

  setActivePanel(panel: Panel) {
    _activePanel = panel;
    scheduleSave();
  },

  toggleSidebar() {
    _sidebarOpen = !_sidebarOpen;
    scheduleSave();
  },

  openSidebar() {
    _sidebarOpen = true;
    scheduleSave();
  },

  closeSidebar() {
    _sidebarOpen = false;
    scheduleSave();
  },

  /** Called during drag — no persistence */
  setSidebarWidthImmediate(width: number) {
    _sidebarWidth = Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, width));
  },

  /** Called on mouseup — commits and persists */
  commitSidebarWidth(width: number) {
    _sidebarWidth = Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, width));
    scheduleSave();
  },

  // --- Pane tree ---
  get paneTree(): PaneLayout {
    return _paneTree;
  },

  get activePaneId(): string {
    return _activePaneId;
  },

  /** Vault-relative path of the active tab in the active pane, or null if none. */
  get activeFilePath(): string | null {
    return _activeFilePath;
  },

  setActivePane(paneId: string) {
    _activePaneId = paneId;
    scheduleSave();
  },

  /** Open a tab in the currently active pane. Used by FileExplorer and other callers. */
  openTab(path: string) {
    _paneTree = mapLeaf(_paneTree, _activePaneId, (p) => leafOpenTab(p, path));
    scheduleSave();
  },

  /** Open a tab in a specific pane by ID. */
  openTabInPane(paneId: string, path: string) {
    _paneTree = mapLeaf(_paneTree, paneId, (p) => leafOpenTab(p, path));
    _activePaneId = paneId;
    scheduleSave();
  },

  closeTab(paneId: string, tabId: string) {
    let becameEmpty = false;
    _paneTree = mapLeaf(_paneTree, paneId, (p) => {
      const updated = leafCloseTab(p, tabId);
      becameEmpty = updated.tabs.length === 0;
      return updated;
    });
    // Collapse the split when a non-root pane empties
    if (becameEmpty && _paneTree.type === "split") {
      const pruned = removePane(_paneTree, paneId);
      if (pruned !== null) {
        _paneTree = pruned;
        if (_activePaneId === paneId) {
          _activePaneId = firstLeafId(_paneTree);
        }
      }
    }
    scheduleSave();
  },

  activateTab(paneId: string, tabId: string) {
    _paneTree = mapLeaf(_paneTree, paneId, (p) => leafActivateTab(p, tabId));
    _activePaneId = paneId;
    // Tab activation isn't persisted — restored state is good enough
  },

  /** Split the target pane. The new (empty) pane becomes active. */
  splitPane(paneId: string, direction: "horizontal" | "vertical") {
    const result = splitLayout(_paneTree, paneId, direction);
    if (!result) return;
    const [newTree, newPaneId] = result;
    _paneTree = newTree;
    _activePaneId = newPaneId;
    scheduleSave();
  },

  /** Update split sizes during drag — no persistence (avoids write-per-pixel). */
  resizeSplitImmediate(splitId: string, sizes: [number, number]) {
    const clamped: [number, number] = [
      Math.min(100 - MIN_PANE_PCT, Math.max(MIN_PANE_PCT, sizes[0])),
      Math.min(100 - MIN_PANE_PCT, Math.max(MIN_PANE_PCT, sizes[1])),
    ];
    _paneTree = resizeSplitInTree(_paneTree, splitId, clamped);
  },

  /** Commit final split sizes on drag end. */
  commitSplitResize(splitId: string, sizes: [number, number]) {
    this.resizeSplitImmediate(splitId, sizes);
    scheduleSave();
  },

  renamePath(oldPath: string, newPath: string) {
    _paneTree = renamePathsInTree(_paneTree, oldPath, newPath);
    scheduleSave();
  },

  // --- Boot ---
  async load() {
    try {
      const raw = await invoke<WorkspaceState>("workspace_load");
      if (!raw || raw.version !== WORKSPACE_VERSION) return;
      _activePanel = raw.activePanel ?? "files";
      _sidebarOpen = raw.sidebarOpen ?? true;
      _sidebarWidth = Math.min(
        SIDEBAR_MAX_WIDTH,
        Math.max(SIDEBAR_MIN_WIDTH, raw.sidebarWidth ?? SIDEBAR_DEFAULT_WIDTH),
      );
      if (raw.paneTree) {
        _paneTree = raw.paneTree;
        const leaves = allLeaves(_paneTree);
        const activeExists = leaves.some((l) => l.id === raw.activePaneId);
        _activePaneId = activeExists ? raw.activePaneId : firstLeafId(_paneTree);
      }
    } catch {
      // Missing or corrupt workspace.json — use defaults silently
    }
  },
};

export { SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH };
