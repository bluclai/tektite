/**
 * Aura suggestion store — tracks the lifecycle of a single inline generative
 * continuation surfaced in the editor.
 *
 * Lifecycle:
 *   idle → loading → ready → (accept | dismiss) → idle
 *                 ↘ error  ────────────────────┘
 *
 * The store never dispatches edits directly; EditorPane calls `accept()` which
 * returns the accepted text so the editor can insert it at the original
 * cursor offset. This keeps CodeMirror as the single source of truth for
 * document mutation.
 */

import { invoke } from "@tauri-apps/api/core";

export type AuraState = "idle" | "loading" | "ready" | "error";

interface AnchorCoords {
  /** Viewport y below the cursor line. */
  top: number;
  /** Viewport x — matches left of .cm-content body column. */
  left: number;
  /** .cm-content body column width, for card sizing. */
  width: number;
}

let _state = $state<AuraState>("idle");
let _text = $state<string>("");
let _error = $state<string | null>(null);
let _path = $state<string | null>(null);
let _offset = $state<number>(0);
let _anchor = $state<AnchorCoords | null>(null);

/**
 * Incremented on every request so in-flight stale responses can be discarded
 * if the user dismisses + re-triggers before the previous one resolves.
 */
let requestId = 0;

export const auraStore = {
  get state(): AuraState {
    return _state;
  },
  get text(): string {
    return _text;
  },
  get error(): string | null {
    return _error;
  },
  get path(): string | null {
    return _path;
  },
  get offset(): number {
    return _offset;
  },
  get anchor(): AnchorCoords | null {
    return _anchor;
  },

  async request(path: string, cursorOffset: number, anchor: AnchorCoords): Promise<void> {
    const id = ++requestId;
    _state = "loading";
    _text = "";
    _error = null;
    _path = path;
    _offset = cursorOffset;
    _anchor = anchor;

    try {
      const text = await invoke<string>("aura_continue", {
        filePath: path,
        cursorOffset,
      });
      if (id !== requestId) return;
      _text = text;
      _state = "ready";
    } catch (error) {
      if (id !== requestId) return;
      _error = error instanceof Error ? error.message : String(error);
      _state = "error";
    }
  },

  /** Returns the accepted text and resets state. Caller is responsible for insertion. */
  accept(): { text: string; offset: number; path: string } | null {
    if (_state !== "ready" || _path === null) return null;
    const payload = { text: _text, offset: _offset, path: _path };
    reset();
    return payload;
  },

  dismiss(): void {
    reset();
  },
};

function reset() {
  requestId += 1;
  _state = "idle";
  _text = "";
  _error = null;
  _path = null;
  _offset = 0;
  _anchor = null;
}
