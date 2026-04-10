/**
 * Editor store — tracks per-tab save state.
 *
 * The status bar observes `editorStore.saveState` to show
 * "Saving…" / "Saved" / "Error" feedback.
 */

export type SaveState = "saved" | "saving" | "unsaved" | "error";

let _saveState = $state<SaveState>("saved");

export const editorStore = {
  get saveState(): SaveState {
    return _saveState;
  },
  setSaveState(s: SaveState) {
    _saveState = s;
  },
};
