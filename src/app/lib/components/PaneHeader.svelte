<script lang="ts">
    import { Sun, Moon, MoreHorizontal } from '@lucide/svelte';
    import type { PaneTab } from '$lib/stores/workspace.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import { editorStore } from '$lib/stores/editor.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import { pinnedStore } from '$lib/stores/pinned-notes.svelte';

    interface Props {
        tab: PaneTab | null;
    }

    let { tab }: Props = $props();

    const segments = $derived.by<{ folders: string[]; file: string } | null>(() => {
        if (!tab) return null;
        const vaultRoot = vaultStore.path ?? '';
        let rel = tab.path;
        if (vaultRoot && rel.startsWith(vaultRoot + '/')) {
            rel = rel.slice(vaultRoot.length + 1);
        }
        const parts = rel.split('/').filter(Boolean);
        if (parts.length === 0) return null;
        const file = parts[parts.length - 1].replace(/\.md$/i, '');
        return { folders: parts.slice(0, -1), file };
    });

    const statusChip = $derived.by(() => {
        if (!tab) return null;
        const vaultRoot = vaultStore.path ?? '';
        const rel = vaultRoot && tab.path.startsWith(vaultRoot + '/')
            ? tab.path.slice(vaultRoot.length + 1)
            : tab.path;
        if (pinnedStore.has(rel)) return 'Pinned';
        return 'Draft';
    });

    let _tick = $state(0);
    $effect(() => {
        const id = setInterval(() => { _tick++; }, 10_000);
        return () => clearInterval(id);
    });

    const savedLabel = $derived.by(() => {
        void _tick;
        if (editorStore.saveState === 'saving') return 'Saving…';
        if (editorStore.saveState === 'unsaved') return 'Unsaved';
        if (editorStore.saveState === 'error') return 'Save error';
        const ts = editorStore.lastSavedAt;
        if (ts === null) return 'Saved';
        const diffSec = Math.floor((Date.now() - ts) / 1000);
        if (diffSec < 5) return 'Saved just now';
        if (diffSec < 60) return `Saved ${diffSec}s ago`;
        const diffMin = Math.floor(diffSec / 60);
        if (diffMin < 60) return `Saved ${diffMin}m ago`;
        return `Saved ${Math.floor(diffMin / 60)}h ago`;
    });
</script>

<header
    class="flex h-10 shrink-0 select-none items-center gap-3 px-5"
    style="background-color: var(--color-surface);"
>
    <!-- Breadcrumb -->
    {#if segments}
        <nav class="flex min-w-0 flex-1 items-center gap-1.5 overflow-hidden whitespace-nowrap text-[12px] font-sans">
            {#each segments.folders as folder, i (i)}
                <span class="truncate" style="color: var(--color-text-muted);">{folder}</span>
                <span aria-hidden="true" style="color: var(--color-text-faint);">›</span>
            {/each}
            <span class="truncate" style="color: var(--color-text-secondary);">{segments.file}</span>
        </nav>
    {:else}
        <div class="flex-1"></div>
    {/if}

    <!-- Status chip -->
    {#if statusChip}
        <span
            class="font-sans"
            style="font-size: 11px; font-weight: 500; letter-spacing: 0.02em; color: var(--color-primary); background-color: color-mix(in srgb, var(--color-primary) 8%, transparent); padding: 2px 8px; border-radius: 20px;"
        >
            {statusChip}
        </span>
    {/if}

    <!-- Right cluster -->
    <div class="flex items-center gap-2">
        <span
            class="font-sans text-[11px]"
            style="color: var(--color-text-muted);"
        >
            {savedLabel}
        </span>

        <button
            type="button"
            class="flex h-6 w-6 items-center justify-center rounded-[4px] transition-colors"
            style="color: {workspaceStore.focusMode ? 'var(--color-primary)' : 'var(--color-text-muted)'};"
            onmouseenter={(e) => { e.currentTarget.style.backgroundColor = 'rgba(255,255,255,0.04)'; }}
            onmouseleave={(e) => { e.currentTarget.style.backgroundColor = 'transparent'; }}
            onclick={() => workspaceStore.toggleFocusMode()}
            aria-label="Toggle focus mode"
            aria-pressed={workspaceStore.focusMode}
            title="Focus mode"
        >
            {#if workspaceStore.focusMode}
                <Moon size={14} strokeWidth={1.75} />
            {:else}
                <Sun size={14} strokeWidth={1.75} />
            {/if}
        </button>

        <button
            type="button"
            class="flex h-6 w-6 items-center justify-center rounded-[4px] transition-colors"
            style="color: var(--color-text-muted);"
            onmouseenter={(e) => { e.currentTarget.style.backgroundColor = 'rgba(255,255,255,0.04)'; }}
            onmouseleave={(e) => { e.currentTarget.style.backgroundColor = 'transparent'; }}
            aria-label="More actions"
            title="More"
        >
            <MoreHorizontal size={14} strokeWidth={1.75} />
        </button>

        <button
            type="button"
            class="flex h-6 w-6 items-center justify-center rounded-full font-sans text-[10px] font-semibold"
            style="background: linear-gradient(135deg, #BDC2FF 0%, #8188D8 100%); color: #1a1760;"
            aria-label="User menu"
            title="Account"
        >
            J
        </button>
    </div>
</header>
