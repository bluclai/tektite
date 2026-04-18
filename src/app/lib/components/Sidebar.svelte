<script lang="ts">
    import { ChevronDown, Search, Settings } from 'lucide-svelte';
    import { workspaceStore, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH } from '$lib/stores/workspace.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import { indexStatsStore } from '$lib/stores/indexStats.svelte';
    import FileExplorer from '$lib/components/FileExplorer.svelte';
    import SearchPanel from '$lib/components/SearchPanel.svelte';
    import BacklinksPanel from '$lib/components/BacklinksPanel.svelte';
    import RelatedNotesPanel from '$lib/components/RelatedNotesPanel.svelte';
    import UnresolvedLinksPanel from '$lib/components/UnresolvedLinksPanel.svelte';
    import GraphPanel from '$lib/components/GraphPanel.svelte';
    import VaultPopover from '$lib/components/VaultPopover.svelte';
    import * as Popover from '$lib/components/ui/popover/index';

    interface Props {
        onopenPalette?: () => void;
    }

    let { onopenPalette }: Props = $props();

    // Sidebar element — used to write --sidebar-width directly during drag
    let sidebarEl = $state<HTMLElement | null>(null);

    // Drag state (not reactive — purely imperative for 60fps)
    let dragging = false;
    let dragStartX = 0;
    let dragStartWidth = 0;

    let vaultPopoverOpen = $state(false);

    function clamp(value: number, min: number, max: number) {
        return Math.min(max, Math.max(min, value));
    }

    function onHandleMousedown(e: MouseEvent) {
        e.preventDefault();
        dragging = true;
        dragStartX = e.clientX;
        dragStartWidth = workspaceStore.sidebarWidth;

        sidebarEl?.style.setProperty('transition', 'none');

        window.addEventListener('mousemove', onMousemove);
        window.addEventListener('mouseup', onMouseup);
    }

    function onMousemove(e: MouseEvent) {
        if (!dragging) return;
        const delta = e.clientX - dragStartX;
        const newWidth = clamp(dragStartWidth + delta, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        sidebarEl?.style.setProperty('--sidebar-width', `${newWidth}px`);
        workspaceStore.setSidebarWidthImmediate(newWidth);
    }

    function onMouseup(e: MouseEvent) {
        if (!dragging) return;
        dragging = false;
        window.removeEventListener('mousemove', onMousemove);
        window.removeEventListener('mouseup', onMouseup);

        const delta = e.clientX - dragStartX;
        const newWidth = clamp(dragStartWidth + delta, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        workspaceStore.commitSidebarWidth(newWidth);

        sidebarEl?.style.removeProperty('transition');
    }

    function openSettings() {
        workspaceStore.setActivePanel('settings');
        if (!workspaceStore.sidebarOpen) workspaceStore.openSidebar();
    }
</script>

<aside
    bind:this={sidebarEl}
    class="sidebar relative flex shrink-0 flex-row overflow-hidden transition-[width] duration-200 ease-out"
    class:sidebar--closed={!workspaceStore.sidebarOpen}
    style="--sidebar-width: {workspaceStore.sidebarWidth}px"
>
    <div class="sidebar-inner flex h-full shrink-0 flex-col overflow-hidden bg-surface-abyss">
        <!-- Workspace header -->
        <Popover.Root bind:open={vaultPopoverOpen}>
            <Popover.Trigger>
                {#snippet child({ props })}
                    <button
                        {...props}
                        class="group flex h-[54px] shrink-0 cursor-pointer items-center gap-2.5 border-none bg-transparent px-4 text-left transition-colors duration-200 ease-out hover:bg-[rgba(255,255,255,0.02)]"
                        aria-label="Switch vault"
                    >
                        <span
                            class="block h-[22px] w-[22px] shrink-0 rounded-[6px]"
                            style="background: linear-gradient(135deg, #BDC2FF 0%, #8188D8 100%); box-shadow: 0 0 12px rgba(189,194,255,0.18);"
                            aria-hidden="true"
                        ></span>
                        <div class="flex min-w-0 flex-1 flex-col leading-tight">
                            <span class="truncate font-sans text-[13px] font-semibold text-text-primary">
                                {vaultStore.name || 'Vault'}
                            </span>
                            <span class="truncate font-sans text-[11px] text-text-muted">
                                Personal · {indexStatsStore.noteCount} notes
                            </span>
                        </div>
                        <ChevronDown
                            size={14}
                            strokeWidth={1.75}
                            class="shrink-0 text-text-ghost transition-colors duration-200 group-hover:text-text-muted"
                            aria-hidden="true"
                        />
                    </button>
                {/snippet}
            </Popover.Trigger>
            <Popover.Content
                side="bottom"
                align="start"
                sideOffset={6}
                class="p-0 w-auto rounded-lg border-none shadow-2xl"
            >
                <VaultPopover onclose={() => (vaultPopoverOpen = false)} />
            </Popover.Content>
        </Popover.Root>

        <!-- Quick search -->
        <div class="px-3 pb-2">
            <button
                class="flex h-[34px] w-full cursor-pointer items-center gap-2 rounded-[8px] border-none px-2.5 text-left transition-colors duration-200 ease-out hover:brightness-110"
                style="background-color: #131316;"
                onclick={() => onopenPalette?.()}
                aria-label="Quick search"
            >
                <Search size={13} strokeWidth={1.75} class="shrink-0 text-text-muted" aria-hidden="true" />
                <span class="flex-1 truncate font-sans text-[12px] text-text-muted">Quick search</span>
                <span class="kbd">⌘K</span>
            </button>
        </div>

        <!-- Panel content -->
        <div class="flex-1 overflow-hidden">
            {#if workspaceStore.activePanel === 'files'}
                <FileExplorer />
            {:else if workspaceStore.activePanel === 'search'}
                <SearchPanel />
            {:else if workspaceStore.activePanel === 'backlinks'}
                <BacklinksPanel />
            {:else if workspaceStore.activePanel === 'related'}
                <RelatedNotesPanel />
            {:else if workspaceStore.activePanel === 'unresolved'}
                <UnresolvedLinksPanel />
            {:else if workspaceStore.activePanel === 'graph'}
                <GraphPanel />
            {:else}
                <div class="flex h-full items-center justify-center">
                    <p class="font-sans text-[11px] text-text-ghost">Coming soon</p>
                </div>
            {/if}
        </div>

        <!-- Footer -->
        <div class="flex h-9 shrink-0 items-center gap-2 px-4">
            <span
                class="block h-[6px] w-[6px] rounded-full"
                style="background-color: #7AD396; box-shadow: 0 0 6px rgba(122,211,150,0.5);"
                aria-hidden="true"
            ></span>
            <span class="flex-1 truncate font-sans text-[11px] text-text-muted">
                Synced · just now
            </span>
            <button
                class="flex h-6 w-6 cursor-pointer items-center justify-center rounded border-none bg-transparent text-text-ghost transition-colors duration-200 ease-out hover:text-text-secondary"
                onclick={openSettings}
                aria-label="Settings"
                title="Settings"
            >
                <Settings size={13} strokeWidth={1.75} />
            </button>
        </div>
    </div>

    <!-- Resize handle -->
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
        class="resize-handle absolute right-0 top-0 z-10 h-full w-3 cursor-col-resize bg-transparent"
        onmousedown={onHandleMousedown}
        role="separator"
        aria-label="Resize sidebar"
        aria-orientation="vertical"
    ></div>
</aside>

<style>
    .sidebar {
        width: var(--sidebar-width, 240px);
    }

    .sidebar--closed {
        width: 0 !important;
    }

    .sidebar-inner {
        width: var(--sidebar-width, 240px);
        min-width: var(--sidebar-width, 240px);
    }

    .resize-handle::after {
        content: '';
        position: absolute;
        top: 0;
        right: 0;
        width: 2px;
        height: 100%;
        background-color: transparent;
        transition: background-color 150ms ease-out;
    }

    .resize-handle:hover::after,
    .resize-handle:active::after {
        background-color: color-mix(in srgb, var(--color-primary) 50%, transparent);
    }
</style>
