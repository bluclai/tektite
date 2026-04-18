/**
 * Embed status store — semantic-index health and backlog progress.
 *
 * Listens to two Tauri events emitted by the backend:
 *   - `embed:progress { done, total }` — fires after every completed
 *     embedding job while the backlog drains.
 *   - `embed:unavailable`              — fires once at vault open when the
 *     ONNX model can't be loaded (missing resource dir, corrupt model,
 *     factory error). Semantic search is permanently disabled until the
 *     next vault open.
 *
 * Surfaced by StatusBar ("Indexing 47/312", "Semantic search unavailable")
 * and the command palette's `?` mode (progress banner + unavailable state).
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

interface EmbedProgressPayload {
  done: number;
  total: number;
}

let _done = $state(0);
let _total = $state(0);
let _available = $state(true);
let _unlistens: UnlistenFn[] = [];

export const embedStatusStore = {
  /** Chunks embedded so far in the current backlog pass. */
  get done(): number {
    return _done;
  },
  /** Total chunks queued in the current backlog pass. */
  get total(): number {
    return _total;
  },
  /**
   * False once `embed:unavailable` fires. The backend does not emit a
   * matching recovery event — semantic search stays disabled until the
   * next vault open.
   */
  get available(): boolean {
    return _available;
  },
  /** True while the backlog is draining (0 < done < total). */
  get inProgress(): boolean {
    return _total > 0 && _done < _total;
  },

  /**
   * Subscribe to embed:* events. Idempotent — cancels any existing
   * listeners before re-subscribing. Call once after vault open.
   */
  async start(): Promise<void> {
    _unlistens.forEach((fn) => fn());
    _unlistens = [];
    _done = 0;
    _total = 0;
    _available = true;

    _unlistens = await Promise.all([
      listen<EmbedProgressPayload>("embed:progress", ({ payload }) => {
        _done = payload.done;
        _total = payload.total;
      }),
      listen<void>("embed:unavailable", () => {
        _available = false;
      }),
    ]);
  },

  /** Tear down listeners and reset state. */
  stop(): void {
    _unlistens.forEach((fn) => fn());
    _unlistens = [];
    _done = 0;
    _total = 0;
    _available = true;
  },
};
