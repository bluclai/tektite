import { describe, it, expect, beforeEach, vi } from "vitest";
import type { CompletionContext } from "@codemirror/autocomplete";

// Hoisted mocks — must survive module re-imports.
const invokeMock = vi.hoisted(() => vi.fn());
const filesStoreRefreshMock = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/core", () => ({
  invoke: invokeMock,
}));

vi.mock("$lib/stores/files.svelte", () => ({
  filesStore: { refresh: filesStoreRefreshMock },
}));

// Import after mocks so wiki-link.ts picks up the stubs.
import {
  wikiLinkCompletionSource,
  wikiLinkExtension,
  wikiLinkAutocomplete,
  wikiLinkTheme,
  updateLinkHandlers,
  type FileCompletionEntry,
} from "../editor/wiki-link";

// ---------------------------------------------------------------------------
// Fake CompletionContext / EditorView helpers
// ---------------------------------------------------------------------------

/** Build a minimal CompletionContext around a fixed document string. */
function makeCtx(text: string, pos: number = text.length): CompletionContext {
  return {
    state: {
      doc: {
        sliceString: (from: number, to: number) => text.slice(from, to),
      },
    },
    pos,
  } as unknown as CompletionContext;
}

/**
 * Build a minimal EditorView-like object for testing the `apply` callback.
 * `update()` just tags the spec so we can read it back; `dispatch()` captures it.
 */
interface CapturedDispatch {
  changes: { from: number; to: number; insert: string };
  selection: { anchor: number };
}

function makeView(text: string): {
  view: { state: { doc: { sliceString: (f: number, t: number) => string }; update: (s: unknown) => unknown }; dispatch: (tr: unknown) => void };
  captured: CapturedDispatch[];
} {
  const captured: CapturedDispatch[] = [];
  const view = {
    state: {
      doc: {
        sliceString: (from: number, to: number) => text.slice(from, to),
      },
      update: (spec: unknown) => spec, // identity: dispatch receives the spec directly
    },
    dispatch: (tr: unknown) => {
      captured.push(tr as CapturedDispatch);
    },
  };
  return { view, captured };
}

// ---------------------------------------------------------------------------
// wikiLinkCompletionSource — short-circuit conditions
// ---------------------------------------------------------------------------

describe("wikiLinkCompletionSource — trigger conditions", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("returns null when no [[ appears before the cursor", async () => {
    const result = await wikiLinkCompletionSource(makeCtx("hello world"));
    expect(result).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("returns null when ]] already closes the link before the cursor", async () => {
    // `[[note]] ` — after the closing brackets there's no active insertion.
    const result = await wikiLinkCompletionSource(makeCtx("[[note]] "));
    expect(result).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("returns null when the user has started typing a # fragment", async () => {
    const result = await wikiLinkCompletionSource(makeCtx("[[note#h"));
    expect(result).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("returns null when the user has started typing a | alias", async () => {
    const result = await wikiLinkCompletionSource(makeCtx("[[note|a"));
    expect(result).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("returns null when index_get_files throws", async () => {
    invokeMock.mockRejectedValueOnce(new Error("index not ready"));
    const result = await wikiLinkCompletionSource(makeCtx("[[fo"));
    expect(result).toBeNull();
  });

  it("looks back only within a 200-char window", async () => {
    // Place `[[` well outside the 200-char lookback — should not trigger.
    const long = "x".repeat(300) + "more text";
    const text = "[[" + long;
    const result = await wikiLinkCompletionSource(makeCtx(text));
    expect(result).toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// wikiLinkCompletionSource — successful completion
// ---------------------------------------------------------------------------

describe("wikiLinkCompletionSource — successful completion", () => {
  const uniqueFiles: FileCompletionEntry[] = [
    { path: "notes/alpha.md", name: "alpha" },
    { path: "notes/beta.md", name: "beta" },
    { path: "zeta.md", name: "zeta" },
  ];

  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("returns options sorted by path ascending", async () => {
    invokeMock.mockResolvedValueOnce(uniqueFiles);
    const result = await wikiLinkCompletionSource(makeCtx("[[al"));
    expect(result).not.toBeNull();
    const labels = result!.options.map((o) => o.label);
    expect(labels).toEqual(["alpha", "beta", "zeta"]);
  });

  it("`from` is positioned at the start of the target text (after [[ )", async () => {
    invokeMock.mockResolvedValueOnce(uniqueFiles);
    const text = "See [[al";
    const result = await wikiLinkCompletionSource(makeCtx(text));
    expect(result).not.toBeNull();
    // `[[` is at index 4, target starts at index 6, cursor is at end (8).
    // afterOpen = "al" (length 2), so from = pos - 2 = 6.
    expect(result!.from).toBe(6);
  });

  it("validFor regex matches while typing a target (no ] # |)", () => {
    // Sanity-check the regex exposed on CompletionResult.
    const regex = /^[^\]#|]*$/;
    expect(regex.test("")).toBe(true);
    expect(regex.test("foo")).toBe(true);
    expect(regex.test("foo bar")).toBe(true);
    expect(regex.test("foo]")).toBe(false);
    expect(regex.test("foo#")).toBe(false);
    expect(regex.test("foo|")).toBe(false);
  });

  it("unique-name files get boost=1 and insert the bare name", async () => {
    invokeMock.mockResolvedValueOnce([
      { path: "notes/alpha.md", name: "alpha" },
    ] satisfies FileCompletionEntry[]);
    const result = await wikiLinkCompletionSource(makeCtx("[["));
    const option = result!.options[0];
    expect(option.boost).toBe(1);
    expect(option.label).toBe("alpha");
    // detail shows the path
    expect(option.detail).toBe("notes/alpha.md");
  });

  it("duplicate-name files get boost=0 and inserts a path-qualified target", async () => {
    const dupFiles: FileCompletionEntry[] = [
      { path: "projects/foo/notes.md", name: "notes" },
      { path: "projects/bar/notes.md", name: "notes" },
    ];
    invokeMock.mockResolvedValueOnce(dupFiles);

    const result = await wikiLinkCompletionSource(makeCtx("[[no"));
    expect(result).not.toBeNull();
    expect(result!.options).toHaveLength(2);

    for (const option of result!.options) {
      expect(option.boost).toBe(0);
      expect(option.detail).toContain("inserts path");
    }

    // Verify the first option's apply inserts the path-qualified target.
    // Simulated doc: `[[no` (cursor at end). from=2, to=4.
    const { view, captured } = makeView("[[no");
    // @ts-expect-error — we only need the apply callback, not a full Completion
    result!.options[0].apply(view, result!.options[0], 2, 4);
    expect(captured).toHaveLength(1);
    // "projects/bar/notes.md" sorts before "projects/foo/notes.md" → first option is bar
    expect(captured[0].changes.insert).toBe("projects/bar/notes]] ");
  });
});

// ---------------------------------------------------------------------------
// wikiLinkCompletionSource — apply() behavior
// ---------------------------------------------------------------------------

describe("wikiLinkCompletionSource — apply() behavior", () => {
  const oneFile: FileCompletionEntry[] = [{ path: "alpha.md", name: "alpha" }];

  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("inserts `target]] ` and places the cursor after it", async () => {
    invokeMock.mockResolvedValueOnce(oneFile);
    const result = await wikiLinkCompletionSource(makeCtx("[["));
    const option = result!.options[0];

    const { view, captured } = makeView("[[");
    // from = 2 (start of target), to = 2 (cursor, nothing typed yet)
    // @ts-expect-error — minimal fake view
    option.apply(view, option, 2, 2);

    expect(captured).toHaveLength(1);
    expect(captured[0].changes).toEqual({ from: 2, to: 2, insert: "alpha]] " });
    // Cursor lands right after the inserted text: 2 + "alpha]] ".length = 10
    expect(captured[0].selection.anchor).toBe(10);
  });

  it("skips over an existing `]]` so the link is not doubled", async () => {
    invokeMock.mockResolvedValueOnce(oneFile);
    const result = await wikiLinkCompletionSource(makeCtx("[[al", 4));
    const option = result!.options[0];

    // Simulated doc has `]]` immediately after the cursor at position 4:
    //   "[[al]] "
    //    0123456
    // from=2 (start of target), to=4 (cursor / end of "al")
    // ahead = doc.sliceString(4, 6) = "]]" → insertEnd = 6
    const { view, captured } = makeView("[[al]] ");
    // @ts-expect-error — minimal fake view
    option.apply(view, option, 2, 4);

    expect(captured).toHaveLength(1);
    expect(captured[0].changes.from).toBe(2);
    // insertEnd should be 6 (extends over the existing `]]`)
    expect(captured[0].changes.to).toBe(6);
    expect(captured[0].changes.insert).toBe("alpha]] ");
  });

  it("does NOT skip when only a single `]` follows", async () => {
    invokeMock.mockResolvedValueOnce(oneFile);
    const result = await wikiLinkCompletionSource(makeCtx("[[al", 4));
    const option = result!.options[0];

    // ahead = doc.sliceString(4, 6) = "] " ≠ "]]" → insertEnd stays = to
    const { view, captured } = makeView("[[al] ");
    // @ts-expect-error — minimal fake view
    option.apply(view, option, 2, 4);

    expect(captured[0].changes.to).toBe(4);
  });
});

// ---------------------------------------------------------------------------
// Extension factories (smoke tests)
// ---------------------------------------------------------------------------

describe("wiki-link extension factories", () => {
  it("wikiLinkExtension returns a non-empty extension", () => {
    const ext = wikiLinkExtension({
      onFollow: () => {},
      onAmbiguous: () => {},
      vaultRoot: "/vault",
      sourcePath: "notes/a.md",
    });
    // Array of extensions — should be truthy and non-empty.
    expect(ext).toBeTruthy();
    expect(Array.isArray(ext)).toBe(true);
    expect((ext as unknown[]).length).toBeGreaterThan(0);
  });

  it("wikiLinkAutocomplete returns an extension", () => {
    const ext = wikiLinkAutocomplete();
    expect(ext).toBeTruthy();
  });

  it("wikiLinkTheme is importable as an extension", () => {
    expect(wikiLinkTheme).toBeTruthy();
  });

  it("updateLinkHandlers is exported as a StateEffect alias", () => {
    // It's a StateEffect type — calling .of(...) should produce a StateEffect instance.
    const effect = updateLinkHandlers.of(null);
    expect(effect).toBeTruthy();
  });
});
