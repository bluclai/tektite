<script lang="ts">
    import { editorStore, type SaveState } from '$lib/stores/editor.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';

    // Map save state to display label
    const labels: Record<SaveState, string> = {
        saved: 'Saved',
        saving: 'Saving\u2026',
        unsaved: 'Unsaved',
        error: 'Save error',
    };

    const editorModeLabel = $derived(workspaceStore.previewMode ? 'Live Preview' : 'Source');
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
</script>

<footer
    class="flex h-6 shrink-0 select-none items-center border-t border-outline-variant/20 bg-surface-container-low px-3"
>
    <div class="ml-auto flex items-center gap-2 text-[0.6875rem] text-on-surface-variant opacity-60">
        <span>{editorModeLabel}</span>
        <span class="opacity-40">•</span>
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
