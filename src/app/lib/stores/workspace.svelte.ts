import { invoke } from "@tauri-apps/api/core";

import {
  type PaneTab,
  type LeafPane,
  type SplitPane,
  type PaneLayout,
  type ViewKind,
  nameFromPath,
  makeLeaf,
  leafOpenTab,
  leafOpenViewTab,
  leafSwapActiveTab,
  leafSetTabDirty,
  leafCloseTab,
  leafActivateTab,
  mapLeaf,
  firstLeafId,
  allLeaves,
  splitLayout,
  removePane,
  resizeSplitInTree,
  renamePathsInTree,
} from "./workspace-tree";

// Re-export types so existing consumers don't need to change imports
export type { PaneTab, LeafPane, SplitPane, PaneLayout };
export { allLeaves };

// ---------------------------------------------------------------------------
// Panel (sidebar)
// ---------------------------------------------------------------------------

export type Panel = "files" | "search" | "backlinks" | "related" | "unresolved" | "settings";

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
  focusMode?: boolean;
}

const WORKSPACE_VERSION = 3;
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
let _focusMode = $state<boolean>(false);

// Memoized lookup of the active leaf's active tab path. Recomputes only
// when _paneTree or _activePaneId change rather than on every getter read.
const _activeFilePath = $derived.by<string | null>(() => {
  const leaf = allLeaves(_paneTree).find((l) => l.id === _activePaneId) ?? null;
  if (!leaf || !leaf.activeTabId) return null;
  const tab = leaf.tabs.find((t) => t.id === leaf.activeTabId);
  if (!tab || tab.kind !== "file") return null;
  return tab.path;
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
        focusMode: _focusMode,
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

  get focusMode() {
    return _focusMode;
  },

  toggleFocusMode() {
    _focusMode = !_focusMode;
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

  /**
   * Open a tab in the currently active pane.
   *
   * Default (β plain-swap): if the active tab is swappable, mutates its
   * path in place instead of appending a new tab. Pass `{ forceNew: true }`
   * to skip the swap and always append (Cmd/Ctrl+click, double-click,
   * "open in new tab" actions).
   */
  openTab(path: string, opts?: { forceNew?: boolean }) {
    const forceNew = opts?.forceNew ?? false;
    _paneTree = mapLeaf(_paneTree, _activePaneId, (p) =>
      forceNew ? leafOpenTab(p, path) : leafSwapActiveTab(p, path),
    );
    scheduleSave();
  },

  /**
   * Open (or focus) a singleton view tab in the active pane. View tabs never
   * β-swap — if one of the same kind exists in the pane, it's activated in
   * place; otherwise a new view tab is appended.
   */
  openViewTab(view: ViewKind, name: string) {
    _paneTree = mapLeaf(_paneTree, _activePaneId, (p) => leafOpenViewTab(p, view, name));
    scheduleSave();
  },

  /** Convenience for the whole-vault graph view. */
  openGraphTab() {
    this.openViewTab("graph", "Graph");
  },

  /** Open a tab in a specific pane by ID. */
  openTabInPane(paneId: string, path: string, opts?: { forceNew?: boolean }) {
    const forceNew = opts?.forceNew ?? false;
    _paneTree = mapLeaf(_paneTree, paneId, (p) =>
      forceNew ? leafOpenTab(p, path) : leafSwapActiveTab(p, path),
    );
    _activePaneId = paneId;
    scheduleSave();
  },

  /**
   * Set the dirty flag on a tab. Dirty tabs are ineligible for β-swap —
   * subsequent `openTab` calls land as appends until the tab becomes clean.
   */
  setTabDirty(paneId: string, tabId: string, dirty: boolean) {
    _paneTree = mapLeaf(_paneTree, paneId, (p) => leafSetTabDirty(p, tabId, dirty));
    // Dirty toggles during editing — don't thrash persistence; the next
    // meaningful change (close, activate, resize) will flush.
  },

  /** Set dirty by path across the active pane. Convenience for editor wiring. */
  setTabDirtyByPath(path: string, dirty: boolean) {
    for (const leaf of allLeaves(_paneTree)) {
      const tab = leaf.tabs.find((t) => t.kind === "file" && t.path === path);
      if (tab) {
        _paneTree = mapLeaf(_paneTree, leaf.id, (p) => leafSetTabDirty(p, tab.id, dirty));
      }
    }
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

  /** Close all tabs in the pane except the one with keepTabId. */
  closeOtherTabs(paneId: string, keepTabId: string) {
    _paneTree = mapLeaf(_paneTree, paneId, (p) => {
      const tabs = p.tabs.filter((t) => t.id === keepTabId);
      return { ...p, tabs, activeTabId: keepTabId };
    });
    scheduleSave();
  },

  /** Close all tabs to the right of the tab with tabId. */
  closeTabsToRight(paneId: string, tabId: string) {
    _paneTree = mapLeaf(_paneTree, paneId, (p) => {
      const idx = p.tabs.findIndex((t) => t.id === tabId);
      if (idx === -1) return p;
      const tabs = p.tabs.slice(0, idx + 1);
      const activeStillOpen = tabs.some((t) => t.id === p.activeTabId);
      return {
        ...p,
        tabs,
        activeTabId: activeStillOpen ? p.activeTabId : tabId,
      };
    });
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

  /** Close all tabs across all panes that match a given path. */
  closeTabsByPath(path: string) {
    function closeInLeaf(leaf: LeafPane): LeafPane {
      const tabs = leaf.tabs.filter((t) => !(t.kind === "file" && t.path === path));
      const activeStillOpen = tabs.some((t) => t.id === leaf.activeTabId);
      const fallbackTab = tabs[tabs.length - 1] ?? null;
      return {
        ...leaf,
        tabs,
        activeTabId: activeStillOpen ? leaf.activeTabId : (fallbackTab?.id ?? null),
      };
    }

    _paneTree = mapLeaf(_paneTree, _activePaneId, closeInLeaf);

    // Also close in all non-active panes
    const allLeafIds = allLeaves(_paneTree)
      .map((l) => l.id)
      .filter((id) => id !== _activePaneId);
    for (const leafId of allLeafIds) {
      _paneTree = mapLeaf(_paneTree, leafId, closeInLeaf);
    }

    // Collapse any panes that became empty
    let pruned: PaneLayout = _paneTree;
    let changed = false;
    for (const leafId of allLeafIds) {
      const leaf = allLeaves(pruned).find((l) => l.id === leafId);
      if (leaf && leaf.tabs.length === 0 && pruned.type === "split") {
        const result = removePane(pruned, leafId);
        if (result !== null) {
          pruned = result;
          changed = true;
        }
      }
    }
    if (changed) {
      _paneTree = pruned;
      if (!allLeaves(_paneTree).some((l) => l.id === _activePaneId)) {
        _activePaneId = firstLeafId(_paneTree);
      }
    }

    scheduleSave();
  },

  /**
   * Drop file tabs whose path is not in the given set. View tabs (graph,
   * etc.) are always kept. Used after vault open to purge tabs that
   * persisted state points at but which no longer exist on disk.
   */
  pruneMissingFileTabs(knownPaths: Set<string>) {
    function closeInLeaf(leaf: LeafPane): LeafPane {
      const tabs = leaf.tabs.filter((t) => t.kind !== "file" || knownPaths.has(t.path));
      if (tabs.length === leaf.tabs.length) return leaf;
      const activeStillOpen = tabs.some((t) => t.id === leaf.activeTabId);
      const fallbackTab = tabs[tabs.length - 1] ?? null;
      return {
        ...leaf,
        tabs,
        activeTabId: activeStillOpen ? leaf.activeTabId : (fallbackTab?.id ?? null),
      };
    }

    let changedAny = false;
    for (const leaf of allLeaves(_paneTree)) {
      const updated = closeInLeaf(leaf);
      if (updated !== leaf) {
        _paneTree = mapLeaf(_paneTree, leaf.id, () => updated);
        changedAny = true;
      }
    }
    if (!changedAny) return;

    let pruned: PaneLayout = _paneTree;
    for (const leaf of allLeaves(pruned)) {
      if (leaf.tabs.length === 0 && pruned.type === "split") {
        const result = removePane(pruned, leaf.id);
        if (result !== null) pruned = result;
      }
    }
    _paneTree = pruned;
    if (!allLeaves(_paneTree).some((l) => l.id === _activePaneId)) {
      _activePaneId = firstLeafId(_paneTree);
    }
    scheduleSave();
  },

  /** Close all tabs whose path starts with a given prefix (for folder deletion). */
  closeTabsByPathPrefix(prefix: string) {
    function closeInLeaf(leaf: LeafPane): LeafPane {
      const tabs = leaf.tabs.filter(
        (t) => t.kind !== "file" || (t.path !== prefix && !t.path.startsWith(prefix + "/")),
      );
      const activeStillOpen = tabs.some((t) => t.id === leaf.activeTabId);
      const fallbackTab = tabs[tabs.length - 1] ?? null;
      return {
        ...leaf,
        tabs,
        activeTabId: activeStillOpen ? leaf.activeTabId : (fallbackTab?.id ?? null),
      };
    }

    // Apply to all leaves
    const leaves = allLeaves(_paneTree);
    for (const leaf of leaves) {
      _paneTree = mapLeaf(_paneTree, leaf.id, closeInLeaf);
    }

    // Collapse any panes that became empty
    const updatedLeaves = allLeaves(_paneTree);
    let pruned: PaneLayout = _paneTree;
    let changed = false;
    for (const leaf of updatedLeaves) {
      if (leaf.tabs.length === 0 && pruned.type === "split") {
        const result = removePane(pruned, leaf.id);
        if (result !== null) {
          pruned = result;
          changed = true;
        }
      }
    }
    if (changed) {
      _paneTree = pruned;
      if (!allLeaves(_paneTree).some((l) => l.id === _activePaneId)) {
        _activePaneId = firstLeafId(_paneTree);
      }
    }

    scheduleSave();
  },

  /**
   * Close every tab across every pane and collapse the pane tree back to a
   * single empty leaf. Used when switching vaults — tabs hold paths relative
   * to the old vault and would be invalid against the new one.
   */
  resetPanes() {
    const leaf = makeLeaf();
    _paneTree = leaf;
    _activePaneId = leaf.id;
    scheduleSave();
  },

  // --- Boot ---
  async load() {
    try {
      const raw = await invoke<WorkspaceState & { version: number }>("workspace_load");
      if (!raw) return;

      // Forward-compat: future versions should win silently rather than
      // being mangled by an older migration. Only v1–v3 are handled.
      if (raw.version !== 1 && raw.version !== 2 && raw.version !== WORKSPACE_VERSION) return;

      // `graph` sidebar panel retired — migrate stale state to `files`.
      const legacyPanel = raw.activePanel as string | undefined;
      _activePanel = legacyPanel === "graph" || !legacyPanel ? "files" : (legacyPanel as Panel);
      _sidebarOpen = raw.sidebarOpen ?? true;
      _sidebarWidth = Math.min(
        SIDEBAR_MAX_WIDTH,
        Math.max(SIDEBAR_MIN_WIDTH, raw.sidebarWidth ?? SIDEBAR_DEFAULT_WIDTH),
      );
      _focusMode = raw.focusMode ?? false;
      if (raw.paneTree) {
        // v1 → v2: stamp kind="file" on every tab.
        // v2 → v3: structurally compatible (v3 only *permits* view tabs; v2
        // state never has them) so no transform needed.
        _paneTree = raw.version === 1 ? migrateTreeV1ToV2(raw.paneTree) : raw.paneTree;
        const leaves = allLeaves(_paneTree);
        const activeExists = leaves.some((l) => l.id === raw.activePaneId);
        _activePaneId = activeExists ? raw.activePaneId : firstLeafId(_paneTree);
      }

      if (raw.version !== WORKSPACE_VERSION) scheduleSave(); // persist the migrated shape
    } catch {
      // Missing or corrupt workspace.json — use defaults silently
    }
  },
};

/**
 * Migrate v1 tabs (no `kind`) to v2 by stamping every tab with `kind: 'file'`.
 * Input is the persisted v1 shape, so we type it as unknown and rebuild.
 */
function migrateTreeV1ToV2(layout: unknown): PaneLayout {
  const node = layout as { type: string } & Record<string, unknown>;
  if (node.type === "leaf") {
    const leaf = node as unknown as {
      type: "leaf";
      id: string;
      tabs: Array<{ id: string; path: string; name?: string }>;
      activeTabId: string | null;
    };
    const tabs: PaneTab[] = leaf.tabs.map((t) => ({
      kind: "file" as const,
      id: t.id,
      path: t.path,
      name: t.name ?? nameFromPath(t.path),
    }));
    return { type: "leaf", id: leaf.id, tabs, activeTabId: leaf.activeTabId };
  }
  const split = node as unknown as {
    type: "split";
    id: string;
    direction: "horizontal" | "vertical";
    a: unknown;
    b: unknown;
    sizes: [number, number];
  };
  return {
    type: "split",
    id: split.id,
    direction: split.direction,
    a: migrateTreeV1ToV2(split.a),
    b: migrateTreeV1ToV2(split.b),
    sizes: split.sizes,
  };
}

export { SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH };
