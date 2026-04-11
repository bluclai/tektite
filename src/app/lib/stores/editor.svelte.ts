/**
 * Editor store — tracks save/open status shown in the global status bar.
 */

export type SaveState = "saved" | "saving" | "unsaved" | "error";

let _saveState = $state<SaveState>("saved");
let _statusDetail = $state<string>("");
let _statusTarget = $state<string | null>(null);

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

  setSaveState(s: SaveState, options?: { detail?: string | null; target?: string | null }) {
    _saveState = s;
    if (options && "detail" in options) {
      _statusDetail = options.detail ?? "";
    }
    if (options && "target" in options) {
      _statusTarget = options.target ?? null;
    }
  },

  clearStatus() {
    _saveState = "saved";
    _statusDetail = "";
    _statusTarget = null;
  },
};
