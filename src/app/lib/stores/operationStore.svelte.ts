/**
 * Operation store — transient system signals for the status bar middle zone.
 *
 * Listens to Tauri events emitted by features that are not yet shipped:
 *   - `embed:progress`  (tektite-embed Phase 3)
 *   - `agent:*`         (agent-native-workspace Phase 1)
 *
 * Listeners are registered eagerly so the status bar is ready the moment
 * those features start emitting. Until then, state stays at its defaults
 * and nothing renders.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ---------------------------------------------------------------------------
// Types — mirror the Rust event payloads (snake_case serialisation)
// ---------------------------------------------------------------------------

interface EmbedProgressPayload {
  done: number;
  total: number;
}

// Agent events carry various payloads; we only care about presence/absence.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyPayload = any;

// ---------------------------------------------------------------------------
// Reactive state
// ---------------------------------------------------------------------------

// Embedding progress (tektite-embed Phase 3)
let _embedDone = $state(0);
let _embedTotal = $state(0);

// Agent run (agent-native-workspace Phase 1)
let _agentRunning = $state(false);

let _unlistens: UnlistenFn[] = [];

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

export const operationStore = {
  /** Number of chunks embedded so far in the current backlog pass. */
  get embedDone(): number {
    return _embedDone;
  },
  /** Total chunks queued in the current backlog pass. */
  get embedTotal(): number {
    return _embedTotal;
  },
  /** True while embedding is in progress (done < total and total > 0). */
  get isEmbedding(): boolean {
    return _embedTotal > 0 && _embedDone < _embedTotal;
  },

  /** True while an agent run is active (between tool-call and complete/error). */
  get isAgentRunning(): boolean {
    return _agentRunning;
  },

  /**
   * Subscribe to all operation events.
   * Idempotent — cancels any existing listeners before re-subscribing.
   * Call once after vault open.
   */
  async start(): Promise<void> {
    _unlistens.forEach((fn) => fn());
    _unlistens = [];

    _unlistens = await Promise.all([
      // --- Embedding progress (tektite-embed Phase 3) ---
      listen<EmbedProgressPayload>("embed:progress", ({ payload }) => {
        _embedDone = payload.done;
        _embedTotal = payload.total;
      }),

      // --- Agent activity (agent-native-workspace Phase 1) ---
      // Any of these events signals an active run.
      listen<AnyPayload>("agent:tool-call", () => {
        _agentRunning = true;
      }),
      listen<AnyPayload>("agent:text", () => {
        _agentRunning = true;
      }),
      listen<AnyPayload>("agent:edit-proposal", () => {
        _agentRunning = true;
      }),
      // These events signal the run has ended.
      listen<AnyPayload>("agent:complete", () => {
        _agentRunning = false;
      }),
      listen<AnyPayload>("agent:error", () => {
        _agentRunning = false;
      }),
      listen<AnyPayload>("agent:cancel", () => {
        _agentRunning = false;
      }),
    ]);
  },

  /** Tear down all listeners and reset state. */
  stop(): void {
    _unlistens.forEach((fn) => fn());
    _unlistens = [];
    _embedDone = 0;
    _embedTotal = 0;
    _agentRunning = false;
  },
};
