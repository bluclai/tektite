import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";

import type { PaneTab } from "../stores/workspace-tree";

// Hoisted mock survives vi.resetModules() because the factory captures it.
const invokeMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

type WorkspaceModule = typeof import("../stores/workspace.svelte");

/**
 * Narrow a tab to its file-kind form. All tabs in this test file are opened
 * via `openTab(path)`, which produces file tabs — so the cast is safe and
 * keeps assertions readable despite the discriminated union.
 */
function asFile(tab: PaneTab): { id: string; path: string; name: string } {
  if (tab.kind !== "file") throw new Error(`expected file tab, got ${tab.kind}`);
  return tab;
}

/**
 * Re-import the store with a fresh module state. The store is a module-level
 * singleton built around `$state` runes, so each test needs its own instance
 * to avoid bleed between tests.
 */
async function freshStore(): Promise<WorkspaceModule> {
  vi.resetModules();
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  return await import("../stores/workspace.svelte");
}

// ---------------------------------------------------------------------------
// Sidebar
// ---------------------------------------------------------------------------

describe("workspaceStore — sidebar", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("defaults: files panel, sidebar open, width 240", () => {
    expect(mod.workspaceStore.activePanel).toBe("files");
    expect(mod.workspaceStore.sidebarOpen).toBe(true);
    expect(mod.workspaceStore.sidebarWidth).toBe(240);
  });

  it("setActivePanel changes the panel", () => {
    mod.workspaceStore.setActivePanel("search");
    expect(mod.workspaceStore.activePanel).toBe("search");
  });

  it("toggleSidebar flips sidebarOpen", () => {
    mod.workspaceStore.toggleSidebar();
    expect(mod.workspaceStore.sidebarOpen).toBe(false);
    mod.workspaceStore.toggleSidebar();
    expect(mod.workspaceStore.sidebarOpen).toBe(true);
  });

  it("openSidebar / closeSidebar set sidebarOpen explicitly", () => {
    mod.workspaceStore.closeSidebar();
    expect(mod.workspaceStore.sidebarOpen).toBe(false);
    mod.workspaceStore.openSidebar();
    expect(mod.workspaceStore.sidebarOpen).toBe(true);
  });

  it("setSidebarWidthImmediate clamps to [MIN, MAX]", () => {
    mod.workspaceStore.setSidebarWidthImmediate(50);
    expect(mod.workspaceStore.sidebarWidth).toBe(mod.SIDEBAR_MIN_WIDTH);
    mod.workspaceStore.setSidebarWidthImmediate(9999);
    expect(mod.workspaceStore.sidebarWidth).toBe(mod.SIDEBAR_MAX_WIDTH);
    mod.workspaceStore.setSidebarWidthImmediate(300);
    expect(mod.workspaceStore.sidebarWidth).toBe(300);
  });

  it("setSidebarWidthImmediate does NOT schedule a save (drag path)", () => {
    mod.workspaceStore.setSidebarWidthImmediate(300);
    vi.advanceTimersByTime(1000);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("commitSidebarWidth clamps and schedules a save", () => {
    mod.workspaceStore.commitSidebarWidth(9999);
    expect(mod.workspaceStore.sidebarWidth).toBe(mod.SIDEBAR_MAX_WIDTH);
    vi.advanceTimersByTime(500);
    expect(invokeMock).toHaveBeenCalledWith(
      "workspace_save",
      expect.objectContaining({
        state: expect.objectContaining({ sidebarWidth: mod.SIDEBAR_MAX_WIDTH }),
      }),
    );
  });
});

// ---------------------------------------------------------------------------
// Pane tree basics
// ---------------------------------------------------------------------------

describe("workspaceStore — pane tree basics", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("initial state: single empty leaf is active", () => {
    const tree = mod.workspaceStore.paneTree;
    expect(tree.type).toBe("leaf");
    if (tree.type === "leaf") {
      expect(tree.tabs).toEqual([]);
      expect(mod.workspaceStore.activePaneId).toBe(tree.id);
    }
  });

  it("openTab adds a tab to the active pane and activates it", () => {
    mod.workspaceStore.openTab("a.md");
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    expect(tree.tabs).toHaveLength(1);
    expect(asFile(tree.tabs[0]).path).toBe("a.md");
    expect(tree.activeTabId).toBe(tree.tabs[0].id);
  });

  it("openTab de-duplicates — opening the same path just activates it", () => {
    // forceNew to explicitly request stacking (the default is β-swap now)
    mod.workspaceStore.openTab("a.md", { forceNew: true });
    mod.workspaceStore.openTab("b.md", { forceNew: true });
    mod.workspaceStore.openTab("a.md", { forceNew: true }); // re-open
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    expect(tree.tabs).toHaveLength(2);
    expect(tree.activeTabId).toBe(tree.tabs[0].id); // a.md
  });

  it("openTab default behavior: β-swap replaces the active tab's path in place", () => {
    mod.workspaceStore.openTab("a.md");
    const firstTabId = (() => {
      const tree = mod.workspaceStore.paneTree;
      if (tree.type !== "leaf") throw new Error("expected leaf");
      return tree.tabs[0].id;
    })();

    mod.workspaceStore.openTab("b.md"); // β-swap (no forceNew)

    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    expect(tree.tabs).toHaveLength(1);
    expect(tree.tabs[0].id).toBe(firstTabId); // same tab, swapped content
    expect(asFile(tree.tabs[0]).path).toBe("b.md");
  });

  it("openTab forceNew appends instead of swapping", () => {
    mod.workspaceStore.openTab("a.md");
    mod.workspaceStore.openTab("b.md", { forceNew: true });
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    expect(tree.tabs).toHaveLength(2);
    expect(tree.tabs.map((t) => asFile(t).path)).toEqual(["a.md", "b.md"]);
  });

  it("openTab on a dirty tab falls back to append (dirty-sticky safety)", () => {
    mod.workspaceStore.openTab("a.md");
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    mod.workspaceStore.setTabDirty(mod.workspaceStore.activePaneId, tree.tabs[0].id, true);

    mod.workspaceStore.openTab("b.md"); // would swap, but a is dirty → append

    const after = mod.workspaceStore.paneTree;
    if (after.type !== "leaf") throw new Error("expected leaf");
    expect(after.tabs.map((t) => asFile(t).path)).toEqual(["a.md", "b.md"]);
  });

  it("openTabInPane targets a specific pane and makes it active", () => {
    const initialPaneId = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(initialPaneId, "horizontal");
    // splitPane makes the new (right) pane active
    const rightPaneId = mod.workspaceStore.activePaneId;
    expect(rightPaneId).not.toBe(initialPaneId);

    mod.workspaceStore.openTabInPane(initialPaneId, "a.md");
    expect(mod.workspaceStore.activePaneId).toBe(initialPaneId);
  });

  it("setActivePane changes activePaneId", () => {
    const first = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(first, "horizontal");
    const second = mod.workspaceStore.activePaneId;
    mod.workspaceStore.setActivePane(first);
    expect(mod.workspaceStore.activePaneId).toBe(first);
    expect(second).not.toBe(first);
  });

  it("activateTab sets the active tab in the pane and makes the pane active", () => {
    mod.workspaceStore.openTab("a.md");
    mod.workspaceStore.openTab("b.md");
    const paneId = mod.workspaceStore.activePaneId;
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    const firstTabId = tree.tabs[0].id;
    mod.workspaceStore.activateTab(paneId, firstTabId);
    const after = mod.workspaceStore.paneTree;
    if (after.type === "leaf") {
      expect(after.activeTabId).toBe(firstTabId);
    }
    expect(mod.workspaceStore.activePaneId).toBe(paneId);
  });
});

// ---------------------------------------------------------------------------
// closeTab and collapse behavior
// ---------------------------------------------------------------------------

describe("workspaceStore — closeTab collapse", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("closing the last tab in a split pane collapses the tree", () => {
    mod.workspaceStore.openTab("a.md");
    const leftPaneId = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(leftPaneId, "horizontal");
    const rightPaneId = mod.workspaceStore.activePaneId;
    expect(rightPaneId).not.toBe(leftPaneId);
    mod.workspaceStore.openTab("b.md");

    const split = mod.workspaceStore.paneTree;
    if (split.type !== "split") throw new Error("expected split");
    const rightLeaf = split.b;
    if (rightLeaf.type !== "leaf") throw new Error("expected right leaf");
    const bTabId = rightLeaf.tabs[0].id;

    mod.workspaceStore.closeTab(rightPaneId, bTabId);

    // Tree collapsed back to a single leaf (the original left pane)
    const after = mod.workspaceStore.paneTree;
    expect(after.type).toBe("leaf");
    if (after.type === "leaf") {
      expect(after.id).toBe(leftPaneId);
    }
    // activePaneId should fall back to the surviving pane
    expect(mod.workspaceStore.activePaneId).toBe(leftPaneId);
  });

  it("closing a non-last tab does NOT collapse", () => {
    mod.workspaceStore.openTab("a.md", { forceNew: true });
    mod.workspaceStore.openTab("b.md", { forceNew: true });
    const paneId = mod.workspaceStore.activePaneId;
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    const aTabId = tree.tabs[0].id;

    mod.workspaceStore.closeTab(paneId, aTabId);

    const after = mod.workspaceStore.paneTree;
    expect(after.type).toBe("leaf");
    if (after.type === "leaf") {
      expect(after.tabs).toHaveLength(1);
      expect(asFile(after.tabs[0]).path).toBe("b.md");
    }
  });

  it("closing the last tab in the ROOT leaf leaves an empty leaf (does not delete)", () => {
    mod.workspaceStore.openTab("a.md");
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    const tabId = tree.tabs[0].id;

    mod.workspaceStore.closeTab(mod.workspaceStore.activePaneId, tabId);

    const after = mod.workspaceStore.paneTree;
    expect(after.type).toBe("leaf");
    if (after.type === "leaf") {
      expect(after.tabs).toEqual([]);
    }
  });
});

// ---------------------------------------------------------------------------
// closeOtherTabs / closeTabsToRight
// ---------------------------------------------------------------------------

describe("workspaceStore — closeOtherTabs / closeTabsToRight", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("closeOtherTabs keeps only the specified tab", () => {
    mod.workspaceStore.openTab("a.md", { forceNew: true });
    mod.workspaceStore.openTab("b.md", { forceNew: true });
    mod.workspaceStore.openTab("c.md", { forceNew: true });
    const paneId = mod.workspaceStore.activePaneId;
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    const keepId = tree.tabs[1].id; // b.md

    mod.workspaceStore.closeOtherTabs(paneId, keepId);

    const after = mod.workspaceStore.paneTree;
    if (after.type === "leaf") {
      expect(after.tabs).toHaveLength(1);
      expect(asFile(after.tabs[0]).path).toBe("b.md");
      expect(after.activeTabId).toBe(keepId);
    }
  });

  it("closeTabsToRight keeps the specified tab and everything before it", () => {
    mod.workspaceStore.openTab("a.md", { forceNew: true });
    mod.workspaceStore.openTab("b.md", { forceNew: true });
    mod.workspaceStore.openTab("c.md", { forceNew: true });
    mod.workspaceStore.openTab("d.md", { forceNew: true });
    const paneId = mod.workspaceStore.activePaneId;
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    const bId = tree.tabs[1].id;

    mod.workspaceStore.closeTabsToRight(paneId, bId);

    const after = mod.workspaceStore.paneTree;
    if (after.type === "leaf") {
      expect(after.tabs.map((t) => asFile(t).path)).toEqual(["a.md", "b.md"]);
    }
  });

  it("closeTabsToRight with non-existent tabId is a no-op", () => {
    mod.workspaceStore.openTab("a.md", { forceNew: true });
    mod.workspaceStore.openTab("b.md", { forceNew: true });
    const paneId = mod.workspaceStore.activePaneId;
    const before = mod.workspaceStore.paneTree;

    mod.workspaceStore.closeTabsToRight(paneId, "ghost");

    // mapLeaf returns the same leaf when findIndex is -1 → same reference
    expect(mod.workspaceStore.paneTree).toBe(before);
  });

  it("closeTabsToRight: if active tab is removed, the kept tab becomes active", () => {
    mod.workspaceStore.openTab("a.md", { forceNew: true });
    mod.workspaceStore.openTab("b.md", { forceNew: true });
    mod.workspaceStore.openTab("c.md", { forceNew: true }); // c is active
    const paneId = mod.workspaceStore.activePaneId;
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    const aId = tree.tabs[0].id;

    mod.workspaceStore.closeTabsToRight(paneId, aId);

    const after = mod.workspaceStore.paneTree;
    if (after.type === "leaf") {
      expect(after.tabs).toHaveLength(1);
      expect(after.activeTabId).toBe(aId);
    }
  });
});

// ---------------------------------------------------------------------------
// splitPane
// ---------------------------------------------------------------------------

describe("workspaceStore — splitPane", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("creates a SplitPane and the new (empty) pane becomes active", () => {
    const original = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(original, "horizontal");
    const tree = mod.workspaceStore.paneTree;
    expect(tree.type).toBe("split");
    if (tree.type === "split") {
      expect(tree.direction).toBe("horizontal");
    }
    expect(mod.workspaceStore.activePaneId).not.toBe(original);
  });

  it("splitPane with non-existent id is a no-op", () => {
    const before = mod.workspaceStore.paneTree;
    mod.workspaceStore.splitPane("ghost", "horizontal");
    expect(mod.workspaceStore.paneTree).toBe(before);
  });
});

// ---------------------------------------------------------------------------
// Resize
// ---------------------------------------------------------------------------

describe("workspaceStore — resizeSplit", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("resizeSplitImmediate clamps to [MIN_PANE_PCT, 100-MIN_PANE_PCT]", () => {
    mod.workspaceStore.splitPane(mod.workspaceStore.activePaneId, "horizontal");
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "split") throw new Error("expected split");
    const splitId = tree.id;

    mod.workspaceStore.resizeSplitImmediate(splitId, [5, 95]);

    const after = mod.workspaceStore.paneTree;
    if (after.type === "split") {
      expect(after.sizes).toEqual([15, 85]); // clamped
    }
  });

  it("resizeSplitImmediate does NOT persist", () => {
    mod.workspaceStore.splitPane(mod.workspaceStore.activePaneId, "horizontal");
    // Flush splitPane's scheduled save before asserting resize is silent.
    vi.advanceTimersByTime(500);
    invokeMock.mockClear();
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "split") throw new Error("expected split");

    mod.workspaceStore.resizeSplitImmediate(tree.id, [60, 40]);
    vi.advanceTimersByTime(1000);

    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("commitSplitResize persists", () => {
    mod.workspaceStore.splitPane(mod.workspaceStore.activePaneId, "horizontal");
    vi.advanceTimersByTime(500); // flush splitPane's save
    invokeMock.mockClear();
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "split") throw new Error("expected split");

    mod.workspaceStore.commitSplitResize(tree.id, [60, 40]);
    vi.advanceTimersByTime(500);

    expect(invokeMock).toHaveBeenCalledWith("workspace_save", expect.any(Object));
  });
});

// ---------------------------------------------------------------------------
// renamePath
// ---------------------------------------------------------------------------

describe("workspaceStore — renamePath", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("updates all tabs matching the old path", () => {
    mod.workspaceStore.openTab("notes/a.md", { forceNew: true });
    mod.workspaceStore.openTab("other/doc.md", { forceNew: true });

    mod.workspaceStore.renamePath("notes/a.md", "notes/b.md");

    const tree = mod.workspaceStore.paneTree;
    if (tree.type === "leaf") {
      expect(tree.tabs.map((t) => asFile(t).path)).toEqual(["notes/b.md", "other/doc.md"]);
      expect(tree.tabs[0].name).toBe("b.md");
    }
  });

  it("propagates directory renames to nested tabs", () => {
    mod.workspaceStore.openTab("notes/daily/2026/journal.md");
    mod.workspaceStore.renamePath("notes/daily", "notes/archive");
    const tree = mod.workspaceStore.paneTree;
    if (tree.type === "leaf") {
      expect(asFile(tree.tabs[0]).path).toBe("notes/archive/2026/journal.md");
    }
  });
});

// ---------------------------------------------------------------------------
// closeTabsByPath
// ---------------------------------------------------------------------------

describe("workspaceStore — closeTabsByPath", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("closes matching tabs across all panes", () => {
    mod.workspaceStore.openTab("a.md");
    mod.workspaceStore.openTab("shared.md");
    const left = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(left, "horizontal");
    mod.workspaceStore.openTab("b.md");
    mod.workspaceStore.openTab("shared.md");

    mod.workspaceStore.closeTabsByPath("shared.md");

    for (const leaf of mod.allLeaves(mod.workspaceStore.paneTree)) {
      for (const tab of leaf.tabs) {
        if (tab.kind === "file") expect(tab.path).not.toBe("shared.md");
      }
    }
  });

  it("collapses a non-active pane that becomes empty", () => {
    // Put the target tab in the LEFT (non-active) pane — the collapse loop
    // in closeTabsByPath only iterates non-active leaves (see workspace.svelte.ts
    // ~line 280), so collapse requires the emptied pane to be non-active.
    mod.workspaceStore.openTab("lonely.md"); // left pane, active
    const left = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(left, "horizontal"); // right becomes active
    const right = mod.workspaceStore.activePaneId;
    mod.workspaceStore.openTab("keep.md");

    mod.workspaceStore.closeTabsByPath("lonely.md");

    expect(mod.workspaceStore.paneTree.type).toBe("leaf");
    if (mod.workspaceStore.paneTree.type === "leaf") {
      expect(mod.workspaceStore.paneTree.id).toBe(right);
      expect(asFile(mod.workspaceStore.paneTree.tabs[0]).path).toBe("keep.md");
    }
    expect(mod.workspaceStore.activePaneId).toBe(right);
  });

  it("does NOT collapse the active pane when it empties (current behavior)", () => {
    // Pins the asymmetry: if the active pane empties via closeTabsByPath, the
    // tree stays split because the collapse loop skips the active pane. This
    // may be a bug — pinning it so we notice if the fix changes the shape.
    mod.workspaceStore.openTab("keep.md"); // left pane
    const left = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(left, "horizontal"); // right active
    mod.workspaceStore.openTab("lonely.md");

    mod.workspaceStore.closeTabsByPath("lonely.md");

    // Tree is still split — right pane is empty but active, so no collapse.
    expect(mod.workspaceStore.paneTree.type).toBe("split");
  });

  it("updates activeTabId when the active tab is the one being closed", () => {
    mod.workspaceStore.openTab("a.md", { forceNew: true });
    mod.workspaceStore.openTab("shared.md", { forceNew: true }); // shared is active (last opened)
    mod.workspaceStore.closeTabsByPath("shared.md");
    const tree = mod.workspaceStore.paneTree;
    if (tree.type === "leaf") {
      // Fallback to last remaining tab
      expect(tree.tabs).toHaveLength(1);
      expect(tree.activeTabId).toBe(tree.tabs[0].id);
    }
  });
});

// ---------------------------------------------------------------------------
// closeTabsByPathPrefix
// ---------------------------------------------------------------------------

describe("workspaceStore — closeTabsByPathPrefix", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("closes tabs at and under the prefix, but not sibling directories", () => {
    mod.workspaceStore.openTab("notes/a.md", { forceNew: true });
    mod.workspaceStore.openTab("notes/sub/b.md", { forceNew: true });
    mod.workspaceStore.openTab("notes-other/c.md", { forceNew: true }); // similar prefix but different dir
    mod.workspaceStore.openTab("root.md", { forceNew: true });

    mod.workspaceStore.closeTabsByPathPrefix("notes");

    const tree = mod.workspaceStore.paneTree;
    if (tree.type === "leaf") {
      const paths = tree.tabs.map((t) => asFile(t).path).sort();
      expect(paths).toEqual(["notes-other/c.md", "root.md"]);
    }
  });

  it("closes an exact-match tab (file deletion as prefix)", () => {
    mod.workspaceStore.openTab("notes/a.md");
    mod.workspaceStore.closeTabsByPathPrefix("notes/a.md");
    const tree = mod.workspaceStore.paneTree;
    if (tree.type === "leaf") {
      expect(tree.tabs).toHaveLength(0);
    }
  });

  it("collapses a pane that becomes empty via prefix deletion", () => {
    mod.workspaceStore.openTab("keep.md");
    const left = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(left, "horizontal");
    mod.workspaceStore.openTab("notes/a.md");
    mod.workspaceStore.openTab("notes/b.md");

    mod.workspaceStore.closeTabsByPathPrefix("notes");

    expect(mod.workspaceStore.paneTree.type).toBe("leaf");
    if (mod.workspaceStore.paneTree.type === "leaf") {
      expect(mod.workspaceStore.paneTree.id).toBe(left);
      expect(asFile(mod.workspaceStore.paneTree.tabs[0]).path).toBe("keep.md");
    }
  });
});

// ---------------------------------------------------------------------------
// activeFilePath derived
// ---------------------------------------------------------------------------

describe("workspaceStore — activeFilePath derived", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("is null when no tabs are open", () => {
    expect(mod.workspaceStore.activeFilePath).toBeNull();
  });

  it("returns the active tab's path", () => {
    mod.workspaceStore.openTab("notes/a.md");
    expect(mod.workspaceStore.activeFilePath).toBe("notes/a.md");
  });

  it("updates when the active tab changes within a pane", () => {
    mod.workspaceStore.openTab("a.md");
    mod.workspaceStore.openTab("b.md");
    expect(mod.workspaceStore.activeFilePath).toBe("b.md");
  });

  it("updates when the active pane changes", () => {
    mod.workspaceStore.openTab("a.md");
    const left = mod.workspaceStore.activePaneId;
    mod.workspaceStore.splitPane(left, "horizontal");
    // Right pane is empty and active
    expect(mod.workspaceStore.activeFilePath).toBeNull();

    mod.workspaceStore.openTab("b.md");
    expect(mod.workspaceStore.activeFilePath).toBe("b.md");

    mod.workspaceStore.setActivePane(left);
    expect(mod.workspaceStore.activeFilePath).toBe("a.md");
  });

  it("is null when the active pane's active tab was closed", () => {
    mod.workspaceStore.openTab("a.md");
    const paneId = mod.workspaceStore.activePaneId;
    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    mod.workspaceStore.closeTab(paneId, tree.tabs[0].id);
    expect(mod.workspaceStore.activeFilePath).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// scheduleSave debounce
// ---------------------------------------------------------------------------

describe("workspaceStore — scheduleSave debounce", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("coalesces rapid mutations into a single persisted save", () => {
    mod.workspaceStore.openTab("a.md");
    mod.workspaceStore.openTab("b.md");
    mod.workspaceStore.openTab("c.md");
    expect(invokeMock).not.toHaveBeenCalled();

    vi.advanceTimersByTime(500);

    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith(
      "workspace_save",
      expect.objectContaining({
        state: expect.objectContaining({
          version: 3,
          paneTree: expect.any(Object),
        }),
      }),
    );
  });

  it("a new mutation resets the debounce timer", () => {
    mod.workspaceStore.openTab("a.md");
    vi.advanceTimersByTime(300); // not yet flushed
    expect(invokeMock).not.toHaveBeenCalled();

    mod.workspaceStore.openTab("b.md"); // resets timer
    vi.advanceTimersByTime(300); // 300ms since last change → still pending
    expect(invokeMock).not.toHaveBeenCalled();

    vi.advanceTimersByTime(200); // now 500ms since last change → fires
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("activateTab does NOT schedule a save", () => {
    mod.workspaceStore.openTab("a.md");
    mod.workspaceStore.openTab("b.md");
    vi.advanceTimersByTime(500); // flush the opens
    invokeMock.mockClear();

    const tree = mod.workspaceStore.paneTree;
    if (tree.type !== "leaf") throw new Error("expected leaf");
    mod.workspaceStore.activateTab(mod.workspaceStore.activePaneId, tree.tabs[0].id);

    vi.advanceTimersByTime(1000);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// load
// ---------------------------------------------------------------------------

describe("workspaceStore — load", () => {
  let mod: WorkspaceModule;

  beforeEach(async () => {
    mod = await freshStore();
  });

  it("restores persisted state when version matches", async () => {
    const savedTree = {
      type: "leaf" as const,
      id: "saved-leaf",
      tabs: [{ id: "t1", path: "notes/a.md", name: "a.md" }],
      activeTabId: "t1",
    };
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "workspace_load") {
        return {
          version: 1,
          activePanel: "backlinks",
          sidebarOpen: false,
          sidebarWidth: 220,
          activePaneId: "saved-leaf",
          paneTree: savedTree,
        };
      }
      return undefined;
    });

    await mod.workspaceStore.load();

    expect(mod.workspaceStore.activePanel).toBe("backlinks");
    expect(mod.workspaceStore.sidebarOpen).toBe(false);
    expect(mod.workspaceStore.sidebarWidth).toBe(220);
    expect(mod.workspaceStore.activePaneId).toBe("saved-leaf");
    expect(mod.workspaceStore.activeFilePath).toBe("notes/a.md");
  });

  it("ignores state with a mismatched version", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "workspace_load") {
        return {
          version: 999,
          activePanel: "backlinks",
          sidebarOpen: false,
          sidebarWidth: 300,
          activePaneId: "ghost",
          paneTree: { type: "leaf", id: "ghost", tabs: [], activeTabId: null },
        };
      }
      return undefined;
    });

    await mod.workspaceStore.load();

    // Defaults unchanged
    expect(mod.workspaceStore.activePanel).toBe("files");
    expect(mod.workspaceStore.sidebarOpen).toBe(true);
    expect(mod.workspaceStore.sidebarWidth).toBe(240);
  });

  it("falls back to firstLeafId if persisted activePaneId is not in the tree", async () => {
    const savedTree = {
      type: "leaf" as const,
      id: "real-leaf",
      tabs: [],
      activeTabId: null,
    };
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "workspace_load") {
        return {
          version: 1,
          activePanel: "files",
          sidebarOpen: true,
          sidebarWidth: 240,
          activePaneId: "stale-id",
          paneTree: savedTree,
        };
      }
      return undefined;
    });

    await mod.workspaceStore.load();

    expect(mod.workspaceStore.activePaneId).toBe("real-leaf");
  });

  it("clamps persisted sidebarWidth into the allowed range", async () => {
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === "workspace_load") {
        return {
          version: 1,
          activePanel: "files",
          sidebarOpen: true,
          sidebarWidth: 9999,
          activePaneId: "x",
          paneTree: { type: "leaf", id: "x", tabs: [], activeTabId: null },
        };
      }
      return undefined;
    });

    await mod.workspaceStore.load();

    expect(mod.workspaceStore.sidebarWidth).toBe(mod.SIDEBAR_MAX_WIDTH);
  });

  it("does not crash when invoke rejects", async () => {
    invokeMock.mockRejectedValue(new Error("missing file"));
    await expect(mod.workspaceStore.load()).resolves.toBeUndefined();
    // Defaults preserved
    expect(mod.workspaceStore.activePanel).toBe("files");
  });

  it("does not crash when invoke returns null", async () => {
    invokeMock.mockResolvedValue(null);
    await expect(mod.workspaceStore.load()).resolves.toBeUndefined();
    expect(mod.workspaceStore.activePanel).toBe("files");
  });
});
