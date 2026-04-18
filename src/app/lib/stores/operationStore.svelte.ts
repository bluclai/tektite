/**
 * Operation store — transient system signals for the status bar middle zone.
 *
 * Currently tracks agent activity (agent-native-workspace Phase 1). Embed
 * backlog progress moved to `embedStatusStore` when semantic search grew its
 * own UI surfaces (palette `?` mode + Related Notes panel).
 *
 * Listeners are registered eagerly so the status bar is ready the moment the
 * agent starts emitting. Until then, state stays at its defaults and nothing
 * renders.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Agent events carry various payloads; we only care about presence/absence.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyPayload = any;

let _agentRunning = $state(false);

let _unlistens: UnlistenFn[] = [];

export const operationStore = {
  /** True while an agent run is active (between tool-call and complete/error). */
  get isAgentRunning(): boolean {
    return _agentRunning;
  },

  /**
   * Subscribe to agent activity events.
   * Idempotent — cancels any existing listeners before re-subscribing.
   * Call once after vault open.
   */
  async start(): Promise<void> {
    _unlistens.forEach((fn) => fn());
    _unlistens = [];

    _unlistens = await Promise.all([
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
    _agentRunning = false;
  },
};
