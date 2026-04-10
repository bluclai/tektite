<script lang="ts">
    import type { PaneTab } from '$lib/stores/workspace.svelte';

    interface Props {
        tabs: PaneTab[];
        activeTabId: string | null;
        onactivate: (tabId: string) => void;
    }

    let { tabs, activeTabId, onactivate }: Props = $props();

    let open = $state(false);
    let buttonEl = $state<HTMLButtonElement | null>(null);

    function toggle() {
        open = !open;
    }

    function handleSelect(tabId: string) {
        onactivate(tabId);
        open = false;
    }

    function onWindowClick(e: MouseEvent) {
        if (open && buttonEl && !buttonEl.contains(e.target as Node)) {
            open = false;
        }
    }
</script>

<svelte:window onclick={onWindowClick} />

<div class="relative flex shrink-0 items-center">
    <button
        bind:this={buttonEl}
        class="flex h-full items-center gap-1 border-l border-outline-variant/15 bg-surface-container-low px-2 text-on-surface-variant transition-colors duration-150 ease-out hover:bg-surface-container hover:text-on-surface"
        onclick={toggle}
        aria-label="Show all tabs"
        aria-expanded={open}
        aria-haspopup="listbox"
    >
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
            <line x1="1" y1="3" x2="9" y2="3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            <line x1="1" y1="7" x2="9" y2="7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
        </svg>
        <span class="text-[0.625rem] tabular-nums">{tabs.length}</span>
    </button>

    {#if open}
        <div
            class="absolute right-0 top-full z-50 mt-0.5 min-w-[180px] max-w-[280px] overflow-hidden rounded-[6px] bg-surface-container-highest py-1 shadow-[0_8px_32px_rgba(0,0,0,0.24)]"
            role="listbox"
            aria-label="All tabs"
        >
            {#each tabs as tab (tab.id)}
                <button
                    role="option"
                    aria-selected={tab.id === activeTabId}
                    class="flex w-full cursor-pointer items-center gap-2 px-3 py-1.5 text-left text-xs transition-colors duration-100 ease-out hover:bg-surface-container-high
                        {tab.id === activeTabId ? 'text-primary' : 'text-on-surface'}"
                    onclick={() => handleSelect(tab.id)}
                >
                    {#if tab.id === activeTabId}
                        <svg width="8" height="8" viewBox="0 0 8 8" fill="none" class="shrink-0" aria-hidden="true">
                            <circle cx="4" cy="4" r="2.5" fill="currentColor"/>
                        </svg>
                    {:else}
                        <span class="w-2 shrink-0"></span>
                    {/if}
                    <span class="min-w-0 truncate">{tab.name}</span>
                </button>
            {/each}
        </div>
    {/if}
</div>
