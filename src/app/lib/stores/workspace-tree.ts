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
  return { id: crypto.randomUUID(), path, name: nameFromPath(path) };
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

export function renamePathsInTree(layout: PaneLayout, oldPath: string, newPath: string): PaneLayout {
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