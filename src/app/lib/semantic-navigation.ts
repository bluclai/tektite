import { editorNavigationStore } from "$lib/stores/editor-navigation.svelte";
import { vaultStore } from "$lib/stores/vault.svelte";
import { workspaceStore } from "$lib/stores/workspace.svelte";

export interface SemanticHitLike {
  file_path: string;
  heading_text?: string | null;
  heading_level?: number | null;
}

/**
 * Open a semantic-search hit in the active pane, scrolling to the chunk's
 * heading when one is known. Callers are responsible for any surrounding UI
 * cleanup (closing the palette, clearing focus, etc.).
 */
export function openSemanticHit(hit: SemanticHitLike): void {
  const vaultRoot = vaultStore.path;
  const absPath =
    vaultRoot && !hit.file_path.startsWith(vaultRoot)
      ? `${vaultRoot}/${hit.file_path}`
      : hit.file_path;

  if (hit.heading_text && hit.heading_level) {
    editorNavigationStore.requestHeading(hit.file_path, hit.heading_text, hit.heading_level);
  }

  workspaceStore.openTab(absPath);
}
