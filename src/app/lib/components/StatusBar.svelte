<!--
    StatusBar — three-zone layout convention:
    ┌──────────────────────────────────────────────────────────────┐
    │  LEFT: vault stats          MIDDLE: transient ops   RIGHT: save │
    └──────────────────────────────────────────────────────────────┘

    Adding a new ambient signal:
      • Left zone   — permanent vault-wide counts (notes, links, unresolved)
      • Middle zone — transient operation state (embed progress, agent running, etc.)
                      Replaces "indexed N ago" while an operation is active.
                      Add a new {#if operationStore.isFoo} branch in order of priority.
      • Right zone  — editor save state (do not add to this zone)

    Compact mode (<800 px): abbreviated numbers with full details in title tooltip.
    No plugin system — add new signals directly to this file.
-->
<script lang="ts">
    import { editorStore, type SaveState } from '$lib/stores/editor.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import { indexStatsStore } from '$lib/stores/indexStats.svelte';
    import { operationStore } from '$lib/stores/operationStore.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';

    // ---------------------------------------------------------------------------
    // Save state — right zone
    // ---------------------------------------------------------------------------

    const labels: Record<SaveState, string> = {
        saved: 'Saved',
        saving: 'Saving\u2026',
        unsaved: 'Unsaved',
        error: 'Save error',
    };

    const statusLabel = $derived(
        vaultStore.openError && !editorStore.statusDetail ? 'Open error' : labels[editorStore.saveState],
    );
    const targetLabel = $derived(
        editorStore.statusTarget?.split('/').pop() ?? editorStore.statusTarget ?? null,
    );
    const titleText = $derived(
        [statusLabel, targetLabel, editorStore.statusDetail || vaultStore.openError]
            .filter(Boolean)
            .join(' — '),
    );

    // ---------------------------------------------------------------------------
    // Compact mode — window width tracking
    // ---------------------------------------------------------------------------

    let _windowWidth = $state(typeof window !== 'undefined' ? window.innerWidth : 1200);

    $effect(() => {
        const handler = () => { _windowWidth = window.innerWidth; };
        window.addEventListener('resize', handler);
        return () => window.removeEventListener('resize', handler);
    });

    const isCompact = $derived(_windowWidth < 800);

    // ---------------------------------------------------------------------------
    // Index stats — left zone
    // ---------------------------------------------------------------------------

    const fmt = new Intl.NumberFormat();

    /** Formats large numbers to compact form: 18740 → "18.7k", 1200000 → "1.2M". */
    function compactNum(n: number): string {
        if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
        if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
        return String(n);
    }

    /** Tick counter incremented every 10 s — drives relative time and stale check. */
    let _tick = $state(0);

    $effect(() => {
        const id = setInterval(() => { _tick++; }, 10_000);
        return () => clearInterval(id);
    });

    function relativeTime(ms: number | null): string {
        if (ms === null) return '';
        void _tick; // reactive dependency — re-evaluates every 10 s
        const diffSec = Math.floor((Date.now() - ms) / 1000);
        if (diffSec < 5) return 'just now';
        if (diffSec < 60) return `${diffSec}s ago`;
        const diffMin = Math.floor(diffSec / 60);
        if (diffMin < 60) return `${diffMin}m ago`;
        return `${Math.floor(diffMin / 60)}h ago`;
    }

    function signedDelta(n: number): string {
        return n > 0 ? `+${n}` : `${n}`;
    }

    const showStats = $derived(
        vaultStore.path !== null && indexStatsStore.lastIndexedAt !== null,
    );

    // Deltas vs. vault-open baseline
    const noteDelta = $derived(indexStatsStore.noteCount - indexStatsStore.baselineNoteCount);
    const linkDelta = $derived(indexStatsStore.linkCount - indexStatsStore.baselineLinkCount);
    const unresolvedDelta = $derived(
        indexStatsStore.unresolvedCount - indexStatsStore.baselineUnresolvedCount,
    );

    // Per-stat tooltips
    const notesTitle = $derived(
        noteDelta !== 0
            ? `${signedDelta(noteDelta)} since vault opened`
            : `${fmt.format(indexStatsStore.noteCount)} notes indexed`,
    );
    const linksTitle = $derived(
        linkDelta !== 0
            ? `${signedDelta(linkDelta)} since vault opened`
            : `${fmt.format(indexStatsStore.linkCount)} links indexed`,
    );
    const unresolvedTitle = $derived(
        unresolvedDelta !== 0
            ? `${signedDelta(unresolvedDelta)} since vault opened — click to inspect`
            : `${fmt.format(indexStatsStore.unresolvedCount)} unresolved wiki-links — click to inspect`,
    );
    const indexedTitle = $derived(
        indexStatsStore.lastIndexedAt !== null
            ? `Last indexed at ${new Date(indexStatsStore.lastIndexedAt).toLocaleTimeString()}`
            : '',
    );

    // Stale if index hasn't updated in > 60 s
    const isStale = $derived(
        _tick >= 0 &&
        indexStatsStore.lastIndexedAt !== null &&
        Date.now() - indexStatsStore.lastIndexedAt > 60_000,
    );

    const indexedLabel = $derived(relativeTime(indexStatsStore.lastIndexedAt));

    // Compact-mode tooltip — full details on the whole stats group
    const compactTitle = $derived(
        [
            `${fmt.format(indexStatsStore.noteCount)} notes`,
            `${fmt.format(indexStatsStore.linkCount)} links`,
            `${fmt.format(indexStatsStore.unresolvedCount)} unresolved`,
            indexedLabel ? `indexed ${indexedLabel}` : null,
        ].filter(Boolean).join(' · '),
    );

    // ---------------------------------------------------------------------------
    // Middle zone — transient operation indicators (in priority order)
    // ---------------------------------------------------------------------------

    /** Human-readable embed progress label, e.g. "142 / 400". */
    const embedLabel = $derived(
        operationStore.isEmbedding
            ? `${fmt.format(operationStore.embedDone)} / ${fmt.format(operationStore.embedTotal)}`
            : '',
    );

    // ---------------------------------------------------------------------------
    // Actions
    // ---------------------------------------------------------------------------

    function openUnresolved() {
        workspaceStore.openSidebar();
        workspaceStore.setActivePanel('unresolved');
    }

    // TODO (agent-native-workspace Phase 1): replace with workspaceStore.setActivePanel('agent')
    function openAgentPanel() { /* no-op until agent panel ships */ }
</script>

<footer
    class="flex h-6 shrink-0 select-none items-center border-t border-outline-variant/20 bg-surface-container-low px-3"
>
    <!-- LEFT + MIDDLE zones -->
    {#if showStats}
        {#if isCompact}
            <!-- ── Compact mode: abbreviated numbers, full details in tooltip ── -->
            <div
                class="flex items-center gap-1 text-[0.6875rem] text-on-surface-variant opacity-50"
                title={compactTitle}
            >
                <span>{compactNum(indexStatsStore.noteCount)}</span>
                <span class="opacity-40">·</span>
                <span>{compactNum(indexStatsStore.linkCount)}</span>
                <span class="opacity-40">·</span>
                <button
                    class="tabular-nums {indexStatsStore.unresolvedCount > 0 ? 'text-amber-400/70 opacity-100' : ''}"
                    onclick={openUnresolved}
                >
                    {compactNum(indexStatsStore.unresolvedCount)}{indexStatsStore.unresolvedCount > 0 ? '⚠' : ''}
                </button>
                <!-- Transient indicators still show in compact mode -->
                {#if operationStore.isAgentRunning}
                    <span class="opacity-40">·</span>
                    <span class="animate-pulse text-primary/70">●</span>
                {:else if operationStore.isEmbedding}
                    <span class="opacity-40">·</span>
                    <span>{compactNum(operationStore.embedDone)}/{compactNum(operationStore.embedTotal)}</span>
                {:else if isStale}
                    <span class="opacity-40">·</span>
                    <span class="text-amber-400/80 opacity-100">⚠</span>
                {/if}
            </div>

        {:else}
            <!-- ── Full mode ── -->
            <div class="flex items-center gap-1.5 text-[0.6875rem] text-on-surface-variant opacity-50">

                <!-- Left zone: permanent vault stats -->
                <span title={notesTitle}>
                    {fmt.format(indexStatsStore.noteCount)} notes
                </span>

                <span class="opacity-40">•</span>

                <span title={linksTitle}>
                    {fmt.format(indexStatsStore.linkCount)} links
                </span>

                <span class="opacity-40">•</span>

                <button
                    class="tabular-nums transition-opacity hover:opacity-100
                        {indexStatsStore.unresolvedCount > 0 ? 'text-amber-400/70 opacity-100' : ''}"
                    title={unresolvedTitle}
                    onclick={openUnresolved}
                >
                    {fmt.format(indexStatsStore.unresolvedCount)} unresolved
                </button>

                <!-- Middle zone: transient operation state (priority order) -->
                {#if operationStore.isAgentRunning}
                    <!-- Agent run indicator — shows while any agent:tool-call/text is active -->
                    <span class="opacity-40">•</span>
                    <!-- TODO (agent-native-workspace Phase 1): make this a button with onclick={openAgentPanel} -->
                    <button
                        class="flex items-center gap-1 transition-opacity hover:opacity-100"
                        title="An agent run is in progress"
                        onclick={openAgentPanel}
                    >
                        <span class="animate-pulse text-primary/70">●</span>
                        <span>agent running</span>
                    </button>

                {:else if operationStore.isEmbedding}
                    <!-- Embedding progress — shows during tektite-embed backlog processing -->
                    <span class="opacity-40">•</span>
                    <span title="Building semantic index — search will improve as this completes">
                        embedding {embedLabel}
                    </span>

                {:else if indexedLabel}
                    <!-- Fallback: index freshness (shown when no operation is active) -->
                    <span class="opacity-40">•</span>
                    <span
                        class="transition-colors {isStale ? 'text-amber-400/80 opacity-100' : ''}"
                        title={isStale
                            ? 'Index may be stale — watcher may not be running'
                            : indexedTitle}
                    >
                        {#if isStale}<span class="mr-0.5">⚠</span>{/if}indexed {indexedLabel}
                    </span>
                {/if}

            </div>
        {/if}
    {/if}

    <!-- RIGHT zone: editor save state — do not add signals here -->
    <div class="ml-auto flex items-center gap-2 text-[0.6875rem] text-on-surface-variant opacity-60">
        <span
            class="transition-colors duration-150
                {editorStore.saveState === 'error' || vaultStore.openError
                ? 'text-red-400 opacity-80'
                : 'text-on-surface-variant opacity-60'}"
            title={titleText}
        >
            {statusLabel}
        </span>
        {#if targetLabel}
            <span class="max-w-40 truncate" title={editorStore.statusTarget ?? undefined}>
                {targetLabel}
            </span>
        {/if}
        {#if editorStore.statusDetail}
            <span class="max-w-96 truncate" title={editorStore.statusDetail}>
                {editorStore.statusDetail}
            </span>
        {:else if vaultStore.openError}
            <span class="max-w-96 truncate text-red-400 opacity-80" title={vaultStore.openError}>
                {vaultStore.openError}
            </span>
        {/if}
    </div>
</footer>
