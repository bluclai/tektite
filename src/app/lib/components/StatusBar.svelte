<script lang="ts">
    import { editorStore, type SaveState } from '$lib/stores/editor.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';

    // Map save state to display label
    const labels: Record<SaveState, string> = {
        saved: 'Saved',
        saving: 'Saving\u2026',
        unsaved: 'Unsaved',
        error: 'Save error',
    };

    const editorModeLabel = $derived(workspaceStore.previewMode ? 'Live Preview' : 'Source');
</script>

<footer
    class="flex h-6 shrink-0 select-none items-center border-t border-outline-variant/20 bg-surface-container-low px-3"
>
    <div class="ml-auto flex items-center gap-2 text-[0.6875rem] text-on-surface-variant opacity-60">
        <span>{editorModeLabel}</span>
        <span class="opacity-40">•</span>
        <span
            class="transition-colors duration-150
                {editorStore.saveState === 'error'
                ? 'text-red-400 opacity-80'
                : 'text-on-surface-variant opacity-60'}"
        >
            {labels[editorStore.saveState]}
        </span>
    </div>
</footer>
