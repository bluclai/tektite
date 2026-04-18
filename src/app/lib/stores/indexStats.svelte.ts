/**
 * Index stats store.
 *
 * Tracks vault-wide aggregate counts (notes, links, unresolved links) and the
 * timestamp of the last index update. Populated via the `index:stats-changed`
 * push event and an initial `index_get_vault_stats` call on vault open.
 *
 * Also records a baseline snapshot captured at vault-open time so components
 * can show "N since opened" deltas.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Snake_case matches Rust's default serde serialisation.
interface IndexStatsPayload {
  note_count: number;
  link_count: number;
  unresolved_link_count: number;
  /** Unix timestamp in milliseconds. */
  indexed_at: number;
}

// ---------------------------------------------------------------------------
// Reactive state
// ---------------------------------------------------------------------------

let _noteCount = $state(0);
let _linkCount = $state(0);
let _unresolvedCount = $state(0);
let _lastIndexedAt = $state<number | null>(null);

// Baseline snapshot captured once when the vault is first opened.
let _baselineNoteCount = $state(0);
let _baselineLinkCount = $state(0);
let _baselineUnresolvedCount = $state(0);

let _unlistenFn: UnlistenFn | null = null;

function applyPayload(p: IndexStatsPayload) {
  _noteCount = p.note_count;
  _linkCount = p.link_count;
  _unresolvedCount = p.unresolved_link_count;
  _lastIndexedAt = p.indexed_at;
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const indexStatsStore = {
  get noteCount(): number {
    return _noteCount;
  },
  get linkCount(): number {
    return _linkCount;
  },
  get unresolvedCount(): number {
    return _unresolvedCount;
  },
  /** Unix ms of the last index settle, or null before the first update. */
  get lastIndexedAt(): number | null {
    return _lastIndexedAt;
  },

  /** Counts captured at vault-open time — used for "N since opened" tooltips. */
  get baselineNoteCount(): number {
    return _baselineNoteCount;
  },
  get baselineLinkCount(): number {
    return _baselineLinkCount;
  },
  get baselineUnresolvedCount(): number {
    return _baselineUnresolvedCount;
  },

  /**
   * Subscribe to index:stats-changed events and fetch initial stats.
   * Call once after vault open; idempotent (cancels any previous listener).
   * The first successful fetch is recorded as the baseline.
   */
  async start(): Promise<void> {
    if (_unlistenFn) {
      _unlistenFn();
      _unlistenFn = null;
    }

    _unlistenFn = await listen<IndexStatsPayload>("index:stats-changed", ({ payload }) => {
      applyPayload(payload);
    });

    // Fetch initial values — the push event may arrive before the listener is
    // registered if vault_open emits synchronously, so we always fetch once.
    try {
      const stats = await invoke<IndexStatsPayload>("index_get_vault_stats");
      applyPayload(stats);
      // Record the vault-open baseline once.
      _baselineNoteCount = stats.note_count;
      _baselineLinkCount = stats.link_count;
      _baselineUnresolvedCount = stats.unresolved_link_count;
    } catch {
      // Vault not ready yet — the push event will populate when it is.
    }
  },

  /** Tear down the event listener and reset all counts to zero. */
  stop(): void {
    if (_unlistenFn) {
      _unlistenFn();
      _unlistenFn = null;
    }
    _noteCount = 0;
    _linkCount = 0;
    _unresolvedCount = 0;
    _lastIndexedAt = null;
    _baselineNoteCount = 0;
    _baselineLinkCount = 0;
    _baselineUnresolvedCount = 0;
  },
};
