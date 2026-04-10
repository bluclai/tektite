<script lang="ts">
    import { Files, Search, Network, Settings, Database } from 'lucide-svelte';
    import { workspaceStore, type Panel } from '$lib/stores/workspace.svelte';
    import * as Popover from '$lib/components/ui/popover/index';
    import VaultPopover from '$lib/components/VaultPopover.svelte';

    type PanelButton = {
        id: Panel;
        icon: typeof Files;
        label: string;
    };

    const topPanels: PanelButton[] = [
        { id: 'files', icon: Files, label: 'Files' },
        { id: 'search', icon: Search, label: 'Search' },
        { id: 'backlinks', icon: Network, label: 'Backlinks' },
    ];

    const bottomPanels: PanelButton[] = [
        { id: 'settings', icon: Settings, label: 'Settings' },
    ];

    let vaultPopoverOpen = $state(false);

    function handlePanelClick(panel: Panel) {
        if (workspaceStore.activePanel === panel) {
            workspaceStore.toggleSidebar();
        } else {
            workspaceStore.setActivePanel(panel);
            if (!workspaceStore.sidebarOpen) {
                workspaceStore.openSidebar();
            }
        }
    }
</script>

<nav
    class="flex h-full w-11 shrink-0 select-none flex-col bg-surface-container-low"
    aria-label="Activity bar"
>
    <!-- Top section -->
    <div class="flex flex-col items-center py-1">
        {#each topPanels as item (item.id)}
            {@const active = workspaceStore.activePanel === item.id}
            <button
                class="relative flex h-11 w-11 cursor-pointer items-center justify-center border-none bg-transparent text-on-surface-variant transition-opacity duration-150 ease-out {active ? 'opacity-100 text-on-surface' : 'opacity-60 hover:opacity-100'}"
                onclick={() => handlePanelClick(item.id)}
                aria-label={item.label}
                title={item.label}
                aria-pressed={active}
            >
                {#if active}
                    <span
                        class="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-r-[1px] bg-primary"
                        aria-hidden="true"
                    ></span>
                {/if}
                <item.icon size={16} />
            </button>
        {/each}
    </div>

    <!-- Bottom section -->
    <div class="mt-auto flex flex-col items-center py-1">
        {#each bottomPanels as item (item.id)}
            {@const active = workspaceStore.activePanel === item.id}
            <button
                class="relative flex h-11 w-11 cursor-pointer items-center justify-center border-none bg-transparent text-on-surface-variant transition-opacity duration-150 ease-out {active ? 'opacity-100 text-on-surface' : 'opacity-60 hover:opacity-100'}"
                onclick={() => handlePanelClick(item.id)}
                aria-label={item.label}
                title={item.label}
                aria-pressed={active}
            >
                {#if active}
                    <span
                        class="absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-r-[1px] bg-primary"
                        aria-hidden="true"
                    ></span>
                {/if}
                <item.icon size={16} />
            </button>
        {/each}

        <!-- Vault popover trigger -->
        <Popover.Root bind:open={vaultPopoverOpen}>
            <Popover.Trigger>
                {#snippet child({ props })}
                    <button
                        {...props}
                        class="relative flex h-11 w-11 cursor-pointer items-center justify-center border-none bg-transparent text-on-surface-variant transition-opacity duration-150 ease-out {vaultPopoverOpen ? 'opacity-100' : 'opacity-60 hover:opacity-100'}"
                        aria-label="Vault"
                        title="Vault"
                    >
                        <Database size={16} />
                    </button>
                {/snippet}
            </Popover.Trigger>
            <Popover.Content side="right" align="end" sideOffset={8} class="p-0 w-auto rounded-lg border-none shadow-2xl">
                <VaultPopover onclose={() => (vaultPopoverOpen = false)} />
            </Popover.Content>
        </Popover.Root>
    </div>
</nav>
