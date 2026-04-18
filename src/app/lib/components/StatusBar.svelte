<!--
    StatusBar — editorial rewrite (Phase 5).

    Left:  {words} words · {reading} min read · Line {L}, Col {C}
    Mid:   transient operation state (embedding, agent running) — when present
    Right: Markdown · UTF-8 · Focus (Focus toggles .editor--focus)

    Mid-dot separators in --color-text-faint, no dividers.
-->
<script lang="ts">
    import { editorStore } from '$lib/stores/editor.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import { operationStore } from '$lib/stores/operationStore.svelte';

    const fmt = new Intl.NumberFormat();

    const hasActiveFile = $derived(
        vaultStore.path !== null && workspaceStore.activeFilePath !== null,
    );

    const saveLabel = $derived.by(() => {
        if (editorStore.saveState === 'saving') return 'Saving…';
        if (editorStore.saveState === 'unsaved') return 'Unsaved';
        if (editorStore.saveState === 'error') return 'Save error';
        return null;
    });
</script>

<footer
    class="flex h-7 shrink-0 select-none items-center px-4 font-sans text-[11px]"
    style="background-color: var(--color-surface); color: var(--color-text-muted);"
>
    <!-- LEFT: doc metrics -->
    {#if hasActiveFile}
        <div class="flex items-center gap-1.5">
            <span>{fmt.format(editorStore.wordCount)} words</span>
            <span style="color: var(--color-text-faint);">·</span>
            <span>{editorStore.readingMinutes} min read</span>
            <span style="color: var(--color-text-faint);">·</span>
            <span class="tabular-nums">Line {editorStore.line}, Col {editorStore.col}</span>
        </div>
    {/if}

    <!-- MID: transient operation state -->
    {#if operationStore.isAgentRunning}
        <div class="ml-3 flex items-center gap-1.5">
            <span style="color: var(--color-text-faint);">·</span>
            <span class="flex items-center gap-1">
                <span class="animate-pulse" style="color: var(--color-primary);">●</span>
                <span>agent running</span>
            </span>
        </div>
    {:else if operationStore.isEmbedding}
        <div class="ml-3 flex items-center gap-1.5">
            <span style="color: var(--color-text-faint);">·</span>
            <span title="Building semantic index">
                embedding {fmt.format(operationStore.embedDone)} / {fmt.format(operationStore.embedTotal)}
            </span>
        </div>
    {/if}

    <!-- RIGHT: format + encoding + focus -->
    <div class="ml-auto flex items-center gap-1.5">
        {#if saveLabel}
            <span
                style="color: {editorStore.saveState === 'error' ? 'var(--color-destructive)' : 'var(--color-text-secondary)'};"
                title={editorStore.statusDetail || undefined}
            >
                {saveLabel}
            </span>
            <span style="color: var(--color-text-faint);">·</span>
        {/if}
        <span>Markdown</span>
        <span style="color: var(--color-text-faint);">·</span>
        <span>UTF-8</span>
        <span style="color: var(--color-text-faint);">·</span>
        <button
            type="button"
            class="transition-colors"
            style="color: {workspaceStore.focusMode ? 'var(--color-primary)' : 'var(--color-text-muted)'};"
            onclick={() => workspaceStore.toggleFocusMode()}
            aria-pressed={workspaceStore.focusMode}
            title="Toggle focus mode"
        >
            Focus
        </button>
    </div>
</footer>
