/**
 * workspace-tree.ts — Pure tree-manipulation functions for the pane layout.
 *
 * All functions are pure (no Svelte runes, no side effects) so they can be
 * unit-tested without a rendering context. The workspace store imports and
 * composes these helpers.
 */

// ---------------------------------------------------------------------------
// Pane types
// ---------------------------------------------------------------------------

export interface PaneTab {
  id: string;
  path: string;
  /** Filename extracted from path for display */
  name: string;
  /**
   * Discriminator — Phase 1 only uses `'file'`. Phase 2 extends to
   * `'view'` (graph view, etc.) for non-file tabs that should never
   * be the target of a swap.
   */
  kind: "file";
  /**
   * True while the tab has unsaved edits. Controls whether the tab is
   * eligible for β-swap: dirty tabs never get their path mutated out from
   * under the user (dirty-sticky safety).
   */
  dirty?: boolean;
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

export function nameFromPath(path: string): string {
  const lastSlash = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  if (lastSlash === -1) return path;
  return path.slice(lastSlash + 1);
}

export function makeTab(path: string): PaneTab {
  return { id: crypto.randomUUID(), path, name: nameFromPath(path), kind: "file" };
}

/**
 * A tab is swappable when its content can be replaced in-place without
 * losing user work. Phase 1: swappable iff not dirty (view-kind tabs land
 * in Phase 2 and will also be rejected).
 */
export function isSwappable(tab: PaneTab): boolean {
  return tab.kind === "file" && !tab.dirty;
}

export function makeLeaf(): LeafPane {
  return { type: "leaf", id: crypto.randomUUID(), tabs: [], activeTabId: null };
}

// ---------------------------------------------------------------------------
// Pure transforms over LeafPane
// ---------------------------------------------------------------------------

export function leafOpenTab(pane: LeafPane, path: string): LeafPane {
  const existing = pane.tabs.find((t) => t.path === path);
  if (existing) return { ...pane, activeTabId: existing.id };
  const tab = makeTab(path);
  return { ...pane, tabs: [...pane.tabs, tab], activeTabId: tab.id };
}

export function leafCloseTab(pane: LeafPane, tabId: string): LeafPane {
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

export function leafActivateTab(pane: LeafPane, tabId: string): LeafPane {
  return { ...pane, activeTabId: tabId };
}

/**
 * β plain-swap: if the active tab is swappable, replace its path+name in
 * place; otherwise fall back to appending a new tab. If the path is already
 * open as any tab in the pane, just activate that tab (dedupe).
 */
export function leafSwapActiveTab(pane: LeafPane, path: string): LeafPane {
  const existing = pane.tabs.find((t) => t.path === path);
  if (existing) return { ...pane, activeTabId: existing.id };

  const active = pane.tabs.find((t) => t.id === pane.activeTabId) ?? null;
  if (!active || !isSwappable(active)) {
    return leafOpenTab(pane, path);
  }

  const tabs = pane.tabs.map((t) =>
    t.id === active.id ? { ...t, path, name: nameFromPath(path) } : t,
  );
  return { ...pane, tabs };
}

/** Set the dirty flag on a specific tab. */
export function leafSetTabDirty(pane: LeafPane, tabId: string, dirty: boolean): LeafPane {
  const idx = pane.tabs.findIndex((t) => t.id === tabId);
  if (idx === -1) return pane;
  const tab = pane.tabs[idx];
  const currentDirty = tab.dirty ?? false;
  if (currentDirty === dirty) return pane;
  const tabs = pane.tabs.slice();
  tabs[idx] = { ...tab, dirty };
  return { ...pane, tabs };
}

// ---------------------------------------------------------------------------
// Pure transforms over PaneLayout
// ---------------------------------------------------------------------------

/** Apply an updater to the matching leaf. Returns new tree (structurally shared). */
export function mapLeaf(
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
export function firstLeafId(layout: PaneLayout): string {
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
export function splitLayout(
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
export function removePane(layout: PaneLayout, paneId: string): PaneLayout | null {
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
export function resizeSplitInTree(
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

export function renamePathValue(path: string, oldPath: string, newPath: string): string {
  if (path === oldPath) {
    return newPath;
  }

  const oldPrefix = `${oldPath}/`;
  if (path.startsWith(oldPrefix)) {
    return `${newPath}/${path.slice(oldPrefix.length)}`;
  }

  return path;
}

export function renamePathsInTree(
  layout: PaneLayout,
  oldPath: string,
  newPath: string,
): PaneLayout {
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
