import { describe, it, expect } from "vitest";

import {
  nameFromPath,
  makeTab,
  makeLeaf,
  leafOpenTab,
  leafCloseTab,
  leafActivateTab,
  mapLeaf,
  firstLeafId,
  allLeaves,
  splitLayout,
  removePane,
  resizeSplitInTree,
  renamePathValue,
  renamePathsInTree,
  type PaneLayout,
} from "../stores/workspace-tree";

// ---------------------------------------------------------------------------
// nameFromPath
// ---------------------------------------------------------------------------

describe("nameFromPath", () => {
  it("extracts filename from unix path", () => {
    expect(nameFromPath("notes/daily/journal.md")).toBe("journal.md");
  });

  it("returns the string itself when no slash", () => {
    expect(nameFromPath("readme.md")).toBe("readme.md");
  });

  it("extracts filename from Windows-style path", () => {
    // nameFromPath uses Math.max(lastIndexOf("/"), lastIndexOf("\\")) so both separators are checked simultaneously
    expect(nameFromPath(String.raw`folder\file.md`)).toBe("file.md");
  });

  it("handles mixed separators — picks the rightmost / or \\", () => {
    // "a/b\c.md" — the backslash is after the slash, so basename is "c.md"
    expect(nameFromPath(String.raw`a/b\c.md`)).toBe("c.md");
  });

  it("returns empty string for trailing slash", () => {
    expect(nameFromPath("foo/")).toBe("");
  });

  it("returns the string itself for empty string", () => {
    expect(nameFromPath("")).toBe("");
  });

  it("handles deeply nested paths", () => {
    expect(nameFromPath("a/b/c/d/e.md")).toBe("e.md");
  });
});

// ---------------------------------------------------------------------------
// makeTab
// ---------------------------------------------------------------------------

describe("makeTab", () => {
  it("creates a tab with a generated id and correct path/name", () => {
    const tab = makeTab("notes/daily.md");
    expect(tab.path).toBe("notes/daily.md");
    expect(tab.name).toBe("daily.md");
    expect(tab.id).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// makeLeaf
// ---------------------------------------------------------------------------

describe("makeLeaf", () => {
  it("creates an empty leaf with no tabs", () => {
    const leaf = makeLeaf();
    expect(leaf.type).toBe("leaf");
    expect(leaf.tabs).toEqual([]);
    expect(leaf.activeTabId).toBeNull();
    expect(leaf.id).toBeTruthy();
  });
});

// ---------------------------------------------------------------------------
// leafOpenTab
// ---------------------------------------------------------------------------

describe("leafOpenTab", () => {
  it("adds a tab to an empty leaf", () => {
    const leaf = makeLeaf();
    const result = leafOpenTab(leaf, "a.md");
    expect(result.tabs).toHaveLength(1);
    expect(result.tabs[0].path).toBe("a.md");
    expect(result.activeTabId).toBe(result.tabs[0].id);
  });

  it("does not duplicate an existing tab — just activates it", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withA2 = leafOpenTab(withA, "a.md");
    expect(withA2.tabs).toHaveLength(1);
    expect(withA2.activeTabId).toBe(withA.tabs[0].id);
  });

  it("adds a second tab and makes it active", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withB = leafOpenTab(withA, "b.md");
    expect(withB.tabs).toHaveLength(2);
    expect(withB.activeTabId).toBe(withB.tabs[1].id);
  });

  it("preserves structural sharing when activating an existing tab", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withB = leafOpenTab(withA, "b.md");
    const activated = leafOpenTab(withB, "a.md");
    // Same tabs array reference since we only changed activeTabId
    expect(activated.tabs).toBe(withB.tabs);
    expect(activated.activeTabId).toBe(withA.tabs[0].id);
  });
});

// ---------------------------------------------------------------------------
// leafCloseTab
// ---------------------------------------------------------------------------

describe("leafCloseTab", () => {
  it("closes the only tab — leaf becomes empty", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const result = leafCloseTab(withA, withA.tabs[0].id);
    expect(result.tabs).toHaveLength(0);
    expect(result.activeTabId).toBeNull();
  });

  it("closing last (active) tab — falls back to previous sibling", () => {
    // Tabs: [a, b, c] with c active. Close c → b active.
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withB = leafOpenTab(withA, "b.md");
    const withC = leafOpenTab(withB, "c.md");
    const result = leafCloseTab(withC, withC.tabs[2].id);
    expect(result.tabs).toHaveLength(2);
    expect(result.activeTabId).toBe(withB.tabs[1].id);
  });

  it("closing middle active tab — activates next sibling at same index", () => {
    // Tabs: [a, b, c] with b active. Close b → c active (same index position).
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withB = leafOpenTab(withA, "b.md");
    const withC = leafOpenTab(withB, "c.md");
    // Activate b (index 1)
    const activated = leafActivateTab(withC, withB.tabs[1].id);
    const result = leafCloseTab(activated, withB.tabs[1].id);
    expect(result.tabs).toHaveLength(2);
    // idx was 1; tabs[1] is now c (previously at index 2)
    expect(result.activeTabId).toBe(withC.tabs[2].id);
  });

  it("closing first active tab — activates next sibling", () => {
    // Tabs: [a, b] with a active. Close a → b active.
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withB = leafOpenTab(withA, "b.md");
    const activated = leafActivateTab(withB, withA.tabs[0].id);
    const result = leafCloseTab(activated, withA.tabs[0].id);
    expect(result.tabs).toHaveLength(1);
    expect(result.activeTabId).toBe(withB.tabs[1].id);
  });

  it("closing non-active tab preserves current active tab", () => {
    // Tabs: [a, b, c] with c active. Close b → c still active.
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withB = leafOpenTab(withA, "b.md");
    const withC = leafOpenTab(withB, "c.md");
    // c is active; closing b should not change active tab
    const result = leafCloseTab(withC, withB.tabs[1].id);
    expect(result.tabs).toHaveLength(2);
    expect(result.activeTabId).toBe(withC.tabs[2].id);
  });

  it("closing non-existent tabId returns same leaf", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const result = leafCloseTab(withA, "nonexistent");
    expect(result).toBe(withA);
  });
});

// ---------------------------------------------------------------------------
// leafActivateTab
// ---------------------------------------------------------------------------

describe("leafActivateTab", () => {
  it("sets the activeTabId", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const withB = leafOpenTab(withA, "b.md");
    const result = leafActivateTab(withB, withA.tabs[0].id);
    expect(result.activeTabId).toBe(withA.tabs[0].id);
  });

  it("sets activeTabId even for a non-existent tab id (no validation)", () => {
    // leafActivateTab is a pure setter — it doesn't check if the id exists.
    // Pinning this behavior so a future validation step would show up as a breaking change.
    const leaf = makeLeaf();
    const result = leafActivateTab(leaf, "ghost-id");
    expect(result.activeTabId).toBe("ghost-id");
  });
});

// ---------------------------------------------------------------------------
// mapLeaf
// ---------------------------------------------------------------------------

describe("mapLeaf", () => {
  it("applies updater to the matching leaf", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const result = mapLeaf(withA, withA.id, (p) => leafOpenTab(p, "b.md"));
    expect(result.type === "leaf" && result.tabs).toHaveLength(2);
  });

  it("returns same reference for non-matching leaf", () => {
    const other = makeLeaf();
    const result = mapLeaf(other, "nonexistent", (p) => leafOpenTab(p, "b.md"));
    expect(result).toBe(other);
  });

  it("traverses into split panes", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "a.md");
    const [split] = splitLayout(withA, withA.id, "horizontal")!;
    const result = mapLeaf(split, leaf.id, (p) => leafOpenTab(p, "b.md"));
    // Left leaf should now have 2 tabs
    expect(allLeaves(result)[0].tabs).toHaveLength(2);
  });
});

// ---------------------------------------------------------------------------
// firstLeafId
// ---------------------------------------------------------------------------

describe("firstLeafId", () => {
  it("returns the leaf's own id for a single leaf", () => {
    const leaf = makeLeaf();
    expect(firstLeafId(leaf)).toBe(leaf.id);
  });

  it("returns the leftmost leaf in a split", () => {
    const leaf = makeLeaf();
    const [split] = splitLayout(leaf, leaf.id, "horizontal")!;
    expect(firstLeafId(split)).toBe(leaf.id);
  });
});

// ---------------------------------------------------------------------------
// allLeaves
// ---------------------------------------------------------------------------

describe("allLeaves", () => {
  it("returns a single-element array for a leaf", () => {
    const leaf = makeLeaf();
    expect(allLeaves(leaf)).toHaveLength(1);
  });

  it("returns both leaves for a split", () => {
    const leaf = makeLeaf();
    const [split, newLeafId] = splitLayout(leaf, leaf.id, "horizontal")!;
    const leaves = allLeaves(split);
    expect(leaves).toHaveLength(2);
    expect(leaves.map((l) => l.id)).toContain(leaf.id);
    expect(leaves.map((l) => l.id)).toContain(newLeafId);
  });

  it("returns leaves in left-to-right order for nested splits", () => {
    const leaf = makeLeaf();
    const [split1] = splitLayout(leaf, leaf.id, "horizontal")!;
    const rightLeaf = allLeaves(split1)[1];
    const [split2] = splitLayout(split1 as PaneLayout, rightLeaf.id, "vertical")!;
    const leaves = allLeaves(split2);
    expect(leaves).toHaveLength(3);
    // Leftmost should be original leaf
    expect(leaves[0].id).toBe(leaf.id);
  });
});

// ---------------------------------------------------------------------------
// splitLayout
// ---------------------------------------------------------------------------

describe("splitLayout", () => {
  it("splits a leaf into a split with a new empty leaf", () => {
    const leaf = makeLeaf();
    const result = splitLayout(leaf, leaf.id, "horizontal");
    expect(result).not.toBeNull();
    const [newTree, newLeafId] = result!;
    expect(newTree.type).toBe("split");
    if (newTree.type === "split") {
      expect(newTree.direction).toBe("horizontal");
      expect(newTree.a).toBe(leaf);
      expect(newTree.b.id).toBe(newLeafId);
      expect(newTree.sizes).toEqual([50, 50]);
    }
  });

  it("returns null for non-existent targetId", () => {
    const leaf = makeLeaf();
    expect(splitLayout(leaf, "nonexistent", "horizontal")).toBeNull();
  });

  it("splits the right child in a split tree", () => {
    const leaf = makeLeaf();
    const [split1] = splitLayout(leaf, leaf.id, "horizontal")!;
    const rightLeaf = allLeaves(split1)[1];
    const result = splitLayout(split1, rightLeaf.id, "vertical");
    expect(result).not.toBeNull();
    const [split2] = result!;
    expect(allLeaves(split2)).toHaveLength(3);
  });
});

// ---------------------------------------------------------------------------
// removePane
// ---------------------------------------------------------------------------

describe("removePane", () => {
  it("removes the only leaf and returns null", () => {
    const leaf = makeLeaf();
    expect(removePane(leaf, leaf.id)).toBeNull();
  });

  it("returns the leaf itself if its id doesn't match", () => {
    const leaf = makeLeaf();
    expect(removePane(leaf, "nonexistent")).toBe(leaf);
  });

  it("collapses split when one child is removed", () => {
    const leaf = makeLeaf();
    const [split, newLeafId] = splitLayout(leaf, leaf.id, "horizontal")!;
    // Remove the new leaf → should collapse to original leaf
    const result = removePane(split, newLeafId);
    expect(result).toBe(leaf);
  });

  it("collapses to the other child when the first is removed", () => {
    const leaf = makeLeaf();
    const [split, newLeafId] = splitLayout(leaf, leaf.id, "horizontal")!;
    // Remove the original leaf → should collapse to new leaf
    const result = removePane(split, leaf.id);
    expect(result?.type).toBe("leaf");
    expect(result!.id).toBe(newLeafId);
  });

  it("is a silent no-op when called with a SplitPane id", () => {
    // removePane only matches leaf nodes; passing a split id produces a shallow copy
    // but leaves the tree structurally unchanged (no panes removed).
    const leaf = makeLeaf();
    const [split] = splitLayout(leaf, leaf.id, "horizontal")!;
    const result = removePane(split, split.id);
    // Not reference-identical (spread in non-null branch), but structurally unchanged
    expect(result).not.toBeNull();
    expect(result!.type).toBe("split");
    const leaves = allLeaves(result!);
    expect(leaves).toHaveLength(2);
    expect(leaves.map((l) => l.id)).toContain(leaf.id);
  });
});

// ---------------------------------------------------------------------------
// resizeSplitInTree
// ---------------------------------------------------------------------------

describe("resizeSplitInTree", () => {
  it("updates sizes on the matching split by id", () => {
    const leaf = makeLeaf();
    const [split] = splitLayout(leaf, leaf.id, "horizontal")!;
    const result = resizeSplitInTree(split, split.id, [70, 30]);
    expect(result.type).toBe("split");
    if (result.type === "split") {
      expect(result.sizes).toEqual([70, 30]);
    }
  });

  it("returns same reference for a leaf", () => {
    const leaf = makeLeaf();
    expect(resizeSplitInTree(leaf, "any", [60, 40])).toBe(leaf);
  });

  it("preserves structural sharing when no change", () => {
    const leaf = makeLeaf();
    const [split] = splitLayout(leaf, leaf.id, "horizontal")!;
    const result = resizeSplitInTree(split, "nonexistent", [60, 40]);
    expect(result).toBe(split);
  });

  it("resizes nested splits", () => {
    const leaf = makeLeaf();
    const [split1] = splitLayout(leaf, leaf.id, "horizontal")!;
    const rightLeaf = allLeaves(split1)[1];
    const [split2] = splitLayout(split1, rightLeaf.id, "vertical")!;
    // Resize the outer split
    const result = resizeSplitInTree(split2, split1.id, [70, 30]);
    if (result.type === "split") {
      expect(result.sizes).toEqual([70, 30]);
    }
  });
});

// ---------------------------------------------------------------------------
// renamePathValue
// ---------------------------------------------------------------------------

describe("renamePathValue", () => {
  it("renames exact path match", () => {
    expect(renamePathValue("notes/a.md", "notes/a.md", "notes/b.md")).toBe("notes/b.md");
  });

  it("renames prefix for child paths", () => {
    expect(renamePathValue("notes/a/child.md", "notes/a", "notes/b")).toBe("notes/b/child.md");
  });

  it("does not rename unrelated paths", () => {
    expect(renamePathValue("other/c.md", "notes/a.md", "notes/b.md")).toBe("other/c.md");
  });

  it("does not partial-match directory names (e.g. notes/a2 vs notes/a)", () => {
    expect(renamePathValue("notes/a2/file.md", "notes/a", "notes/b")).toBe("notes/a2/file.md");
  });

  it("does not match when oldPath has trailing slash — produces double slash", () => {
    // oldPath "notes/a/" creates prefix "notes/a//" which never matches a normal path.
    // Pinning current behavior: this is a silent no-op.
    expect(renamePathValue("notes/a/child.md", "notes/a/", "notes/b")).toBe("notes/a/child.md");
  });
});

// ---------------------------------------------------------------------------
// renamePathsInTree
// ---------------------------------------------------------------------------

describe("renamePathsInTree", () => {
  it("renames tab paths in a leaf", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "notes/a.md");
    const result = renamePathsInTree(withA, "notes/a.md", "notes/b.md");
    if (result.type === "leaf") {
      expect(result.tabs[0].path).toBe("notes/b.md");
      expect(result.tabs[0].name).toBe("b.md");
    }
  });

  it("rename preserves activeTabId (tab is still the same object with updated path)", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "notes/a.md");
    const withB = leafOpenTab(withA, "other/doc.md");
    // a is active
    const activated = leafActivateTab(withB, withA.tabs[0].id);
    const result = renamePathsInTree(activated, "notes/a.md", "notes/renamed.md");
    if (result.type === "leaf") {
      // activeTabId should survive the rename
      expect(result.activeTabId).toBe(withA.tabs[0].id);
      expect(result.tabs[0].path).toBe("notes/renamed.md");
    }
  });

  it("renames directory prefix for tabs inside that folder", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "notes/daily/journal.md");
    const result = renamePathsInTree(withA, "notes/daily", "notes/archive");
    if (result.type === "leaf") {
      expect(result.tabs[0].path).toBe("notes/archive/journal.md");
      expect(result.tabs[0].name).toBe("journal.md");
    }
  });

  it("preserves structural sharing when no tabs match", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "notes/a.md");
    const result = renamePathsInTree(withA, "other/x.md", "other/y.md");
    expect(result).toBe(withA);
  });

  it("mixed-match leaf: unchanged tabs are preserved by reference", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "notes/a.md");
    const withB = leafOpenTab(withA, "other/doc.md");
    const result = renamePathsInTree(withB, "notes/a.md", "notes/renamed.md");
    if (result.type === "leaf") {
      // b.md tab should be the exact same object reference (not cloned)
      expect(result.tabs[1]).toBe(withB.tabs[1]);
      // a.md tab was replaced (new reference)
      expect(result.tabs[0]).not.toBe(withB.tabs[0]);
    }
  });

  it("renames paths in a split tree", () => {
    const leaf = makeLeaf();
    const withA = leafOpenTab(leaf, "notes/a.md");
    const [split, newLeafId] = splitLayout(withA, withA.id, "horizontal")!;
    // Open same file in right leaf
    const updated = mapLeaf(split, newLeafId, (p) => leafOpenTab(p, "notes/a.md"));
    const result = renamePathsInTree(updated, "notes/a.md", "notes/b.md");
    const leaves = allLeaves(result);
    for (const l of leaves) {
      for (const t of l.tabs) {
        expect(t.path).toBe("notes/b.md");
      }
    }
  });
});
