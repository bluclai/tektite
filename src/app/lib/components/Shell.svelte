<script lang="ts">
    import { type Snippet } from "svelte";
    import Titlebar from "$lib/components/Titlebar.svelte";
    import ActivityBar from "$lib/components/ActivityBar.svelte";
    import Sidebar from "$lib/components/Sidebar.svelte";
    import StatusBar from "$lib/components/StatusBar.svelte";
    import CommandPalette from "$lib/components/CommandPalette.svelte";
    import { workspaceStore } from "$lib/stores/workspace.svelte";

    interface Props {
        children?: Snippet;
    }

    let { children }: Props = $props();
    let commandPaletteOpen = $state(false);

    const titlebarTitle = $derived.by(() => {
        const p = workspaceStore.activeFilePath;
        if (!p) return '';
        return p.split('/').pop()?.replace(/\.md$/i, '') ?? '';
    });

    function onKeydown(e: KeyboardEvent) {
        // Ctrl+K / Cmd+K — open/close command palette
        if (e.key === "k" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            commandPaletteOpen = !commandPaletteOpen;
            return;
        }
        // Ctrl+B / Cmd+B — toggle sidebar
        if (e.key === "b" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            workspaceStore.toggleSidebar();
            return;
        }
        // Ctrl+\ / Cmd+\ — split active pane vertically (side by side)
        if (e.key === "\\" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault();
            workspaceStore.splitPane(workspaceStore.activePaneId, "horizontal");
            return;
        }


    }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="flex h-full flex-col overflow-hidden">
    <Titlebar title={titlebarTitle} />
    <div class="flex flex-1 overflow-hidden">
        <ActivityBar />
        <Sidebar />
        <div class="flex-1 overflow-hidden bg-surface">
            {@render children?.()}
        </div>
    </div>
    <StatusBar />
    <CommandPalette bind:open={commandPaletteOpen} />
</div>
