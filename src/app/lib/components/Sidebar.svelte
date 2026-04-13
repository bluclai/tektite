<script lang="ts">
    import { workspaceStore, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH } from '$lib/stores/workspace.svelte';
    import FileExplorer from '$lib/components/FileExplorer.svelte';
    import SearchPanel from '$lib/components/SearchPanel.svelte';
    import BacklinksPanel from '$lib/components/BacklinksPanel.svelte';
    import UnresolvedLinksPanel from '$lib/components/UnresolvedLinksPanel.svelte';

    const panelLabels: Record<string, string> = {
        files: 'Files',
        search: 'Search',
        backlinks: 'Backlinks',
        unresolved: 'Unresolved Links',
        settings: 'Settings',
    };

    // Sidebar element — used to write --sidebar-width directly during drag
    let sidebarEl = $state<HTMLElement | null>(null);

    // Drag state (not reactive — purely imperative for 60fps)
    let dragging = false;
    let dragStartX = 0;
    let dragStartWidth = 0;

    function clamp(value: number, min: number, max: number) {
        return Math.min(max, Math.max(min, value));
    }

    function onHandleMousedown(e: MouseEvent) {
        e.preventDefault();
        dragging = true;
        dragStartX = e.clientX;
        dragStartWidth = workspaceStore.sidebarWidth;

        // Disable transition during drag for 60fps feel
        sidebarEl?.style.setProperty('transition', 'none');

        window.addEventListener('mousemove', onMousemove);
        window.addEventListener('mouseup', onMouseup);
    }

    function onMousemove(e: MouseEvent) {
        if (!dragging) return;
        const delta = e.clientX - dragStartX;
        const newWidth = clamp(dragStartWidth + delta, SIDEBAR_MIN_WIDTH, SIDEBAR_MAX_WIDTH);
        // Direct DOM mutation — no Svelte reactivity during drag
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

        // Re-enable transition after drag ends
        sidebarEl?.style.removeProperty('transition');
    }
</script>

<!--
    The sidebar uses a CSS custom property (--sidebar-width) for its width so the
    drag handler can update it via direct DOM mutation for 60fps performance.
    The ::after pseudo-element on the resize handle also needs a <style> block.
    Everything else uses Tailwind.
-->
<aside
    bind:this={sidebarEl}
    class="sidebar relative flex shrink-0 flex-row overflow-hidden transition-[width] duration-150 ease-out"
    class:sidebar--closed={!workspaceStore.sidebarOpen}
    style="--sidebar-width: {workspaceStore.sidebarWidth}px"
>
    <!-- Inner panel — fixed at stored width so content doesn't reflow on close -->
    <div class="sidebar-inner flex h-full shrink-0 flex-col overflow-hidden bg-surface-container-low">
        <!-- Panel header -->
        <div class="flex h-9 shrink-0 items-center px-4">
            <span class="text-xs font-medium tracking-wide text-on-surface-variant opacity-60 uppercase">
                {panelLabels[workspaceStore.activePanel] ?? workspaceStore.activePanel}
            </span>
        </div>

        <!-- Panel content -->
        <div class="flex-1 overflow-hidden">
            {#if workspaceStore.activePanel === 'files'}
                <FileExplorer />
            {:else if workspaceStore.activePanel === 'search'}
                <SearchPanel />
            {:else if workspaceStore.activePanel === 'backlinks'}
                <BacklinksPanel />
            {:else if workspaceStore.activePanel === 'unresolved'}
                <UnresolvedLinksPanel />
            {:else}
                <!-- Placeholder for Settings (future phases) -->
                <div class="flex h-full items-center justify-center">
                    <p class="text-xs text-on-surface-variant opacity-30">Coming soon</p>
                </div>
            {/if}
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
    /* Width driven by CSS custom property for direct DOM mutation during drag */
    .sidebar {
        width: var(--sidebar-width, 240px);
    }

    .sidebar--closed {
        width: 0 !important;
    }

    /* Inner content stays at full stored width so it doesn't reflow on close */
    .sidebar-inner {
        width: var(--sidebar-width, 240px);
        min-width: var(--sidebar-width, 240px);
        border-right: 1px solid color-mix(in srgb, var(--color-outline-variant) 20%, transparent);
    }

    /* 4px visible indicator centred in the 12px hit zone */
    .resize-handle::after {
        content: '';
        position: absolute;
        top: 0;
        right: 0;
        width: 4px;
        height: 100%;
        background-color: color-mix(in srgb, var(--color-outline-variant) 20%, transparent);
        transition: background-color 150ms ease-out;
    }

    .resize-handle:hover::after,
    .resize-handle:active::after {
        background-color: var(--color-primary);
    }
</style>
