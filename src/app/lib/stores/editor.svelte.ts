/**
 * Editor store — tracks save/open status shown in the global status bar.
 */

export type SaveState = "saved" | "saving" | "unsaved" | "error";

let _saveState = $state<SaveState>("saved");
let _statusDetail = $state<string>("");
let _statusTarget = $state<string | null>(null);
let _lastSavedAt = $state<number | null>(null);
let _wordCount = $state<number>(0);
let _line = $state<number>(1);
let _col = $state<number>(1);

export const editorStore = {
  get saveState(): SaveState {
    return _saveState;
  },

  get statusDetail(): string {
    return _statusDetail;
  },

  get statusTarget(): string | null {
    return _statusTarget;
  },

  get lastSavedAt(): number | null {
    return _lastSavedAt;
  },

  get wordCount(): number {
    return _wordCount;
  },

  get line(): number {
    return _line;
  },

  get col(): number {
    return _col;
  },

  get readingMinutes(): number {
    return Math.max(1, Math.ceil(_wordCount / 220));
  },

  setDocMetrics(words: number, line: number, col: number) {
    _wordCount = words;
    _line = line;
    _col = col;
  },

  setSaveState(s: SaveState, options?: { detail?: string | null; target?: string | null }) {
    _saveState = s;
    if (options && "detail" in options) {
      _statusDetail = options.detail ?? "";
    }
    if (options && "target" in options) {
      _statusTarget = options.target ?? null;
    }
    if (s === "saved") {
      _lastSavedAt = Date.now();
    }
  },

  clearStatus() {
    _saveState = "saved";
    _statusDetail = "";
    _statusTarget = null;
  },
};
