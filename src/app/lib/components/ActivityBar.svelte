<script lang="ts">
    import {
        Files,
        Search,
        Network,
        Unlink,
        Share2,
        Settings,
        RefreshCw,
    } from 'lucide-svelte';
    import { workspaceStore, type Panel } from '$lib/stores/workspace.svelte';
    import { indexStatsStore } from '$lib/stores/indexStats.svelte';
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
        { id: 'graph', icon: Share2, label: 'Graph' },
        { id: 'backlinks', icon: Network, label: 'Backlinks' },
        { id: 'unresolved', icon: Unlink, label: 'Unresolved Links' },
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

    function handleAuraClick() {
        // Wired in Phase 6.
    }

    const settingsActive = $derived(workspaceStore.activePanel === 'settings');
</script>

<nav
    class="relative flex h-full w-14 shrink-0 select-none flex-col items-center bg-surface-deepest py-2"
    aria-label="Activity bar"
>
    <!-- Brand mark -->
    <div class="mb-2 flex h-9 w-9 items-center justify-center">
        <span
            class="block h-[22px] w-[22px] rounded-[6px]"
            style="background: linear-gradient(135deg, #BDC2FF 0%, #8188D8 100%); box-shadow: 0 0 16px rgba(189, 194, 255, 0.22);"
            aria-hidden="true"
        ></span>
    </div>

    <!-- Top panels -->
    <div class="flex flex-col items-center gap-1">
        {#each topPanels as item (item.id)}
            {@const active = workspaceStore.activePanel === item.id}
            {@const showBadge =
                item.id === 'backlinks' &&
                indexStatsStore.unresolvedCount > 0}
            <button
                class="relative flex h-9 w-9 items-center justify-center rounded-[8px] border-none bg-transparent transition-colors duration-200 ease-out {active
                    ? 'text-primary'
                    : 'text-text-muted hover:text-text-secondary'}"
                onclick={() => handlePanelClick(item.id)}
                aria-label={item.label}
                title={item.label}
                aria-pressed={active}
                style={active
                    ? 'background: linear-gradient(180deg, rgba(189,194,255,0.14) 0%, rgba(189,194,255,0.06) 100%); box-shadow: inset 0 0 0 1px rgba(189,194,255,0.18);'
                    : ''}
            >
                {#if active}
                    <span
                        class="pointer-events-none absolute top-1/2 h-5 w-[2px] -translate-y-1/2 rounded-full"
                        style="left: -16px; background-color: var(--color-primary); box-shadow: 0 0 8px rgba(189,194,255,0.6);"
                        aria-hidden="true"
                    ></span>
                {/if}
                <item.icon size={16} strokeWidth={1.75} />
                {#if showBadge}
                    <span
                        class="absolute -top-0.5 -right-0.5 flex h-4 min-w-[16px] items-center justify-center rounded-full px-1 text-[9px] font-semibold"
                        style="background-color: var(--color-primary); color: var(--color-on-primary);"
                    >
                        {indexStatsStore.unresolvedCount > 99
                            ? '99+'
                            : indexStatsStore.unresolvedCount}
                    </span>
                {/if}
            </button>
        {/each}
    </div>

    <!-- Divider -->
    <span
        class="my-[10px] block h-px w-5"
        style="background-color: rgba(255, 255, 255, 0.06);"
        aria-hidden="true"
    ></span>

    <!-- Aura slot -->
    <button
        class="flex h-9 w-9 cursor-pointer items-center justify-center rounded-[8px] border-none transition-all duration-200 ease-out hover:brightness-110"
        onclick={handleAuraClick}
        aria-label="Aura"
        title="Aura"
        style="background: linear-gradient(180deg, rgba(206,189,255,0.06) 0%, rgba(206,189,255,0.02) 100%); box-shadow: inset 0 0 0 1px rgba(206,189,255,0.12), 0 0 12px rgba(206,189,255,0.08);"
    >
        <span
            class="block h-[10px] w-[10px] rounded-[3px]"
            style="background: linear-gradient(135deg, #CEBDFF 0%, #8A7FD8 100%);"
            aria-hidden="true"
        ></span>
    </button>

    <!-- Bottom group -->
    <div class="mt-auto flex flex-col items-center gap-1">
        <!-- Sync -->
        <button
            class="relative flex h-9 w-9 cursor-pointer items-center justify-center rounded-[8px] border-none bg-transparent text-text-muted transition-colors duration-200 ease-out hover:text-text-secondary"
            aria-label="Sync"
            title="Synced"
        >
            <RefreshCw size={16} strokeWidth={1.75} />
            <span
                class="absolute top-1.5 right-1.5 block h-[6px] w-[6px] rounded-full"
                style="background-color: #7AD396; box-shadow: 0 0 6px rgba(122,211,150,0.6);"
                aria-hidden="true"
            ></span>
        </button>

        <!-- Settings -->
        <button
            class="relative flex h-9 w-9 cursor-pointer items-center justify-center rounded-[8px] border-none bg-transparent transition-colors duration-200 ease-out {settingsActive
                ? 'text-primary'
                : 'text-text-muted hover:text-text-secondary'}"
            onclick={() => handlePanelClick('settings')}
            aria-label="Settings"
            title="Settings"
            aria-pressed={settingsActive}
            style={settingsActive
                ? 'background: linear-gradient(180deg, rgba(189,194,255,0.14) 0%, rgba(189,194,255,0.06) 100%); box-shadow: inset 0 0 0 1px rgba(189,194,255,0.18);'
                : ''}
        >
            {#if settingsActive}
                <span
                    class="pointer-events-none absolute top-1/2 h-5 w-[2px] -translate-y-1/2 rounded-full"
                    style="left: -16px; background-color: var(--color-primary); box-shadow: 0 0 8px rgba(189,194,255,0.6);"
                    aria-hidden="true"
                ></span>
            {/if}
            <Settings size={16} strokeWidth={1.75} />
        </button>

        <!-- Avatar / Vault switcher -->
        <Popover.Root bind:open={vaultPopoverOpen}>
            <Popover.Trigger>
                {#snippet child({ props })}
                    <button
                        {...props}
                        class="flex h-9 w-9 cursor-pointer items-center justify-center rounded-full border-none font-sans text-[11px] font-semibold tracking-wide transition-all duration-200 ease-out"
                        aria-label="Vault"
                        title="Vault"
                        style="background: linear-gradient(180deg, #2a2a2e 0%, #1a1a1c 100%); color: var(--color-text-secondary); box-shadow: inset 0 0 0 1px rgba(255,255,255,0.06);"
                    >
                        J
                    </button>
                {/snippet}
            </Popover.Trigger>
            <Popover.Content
                side="right"
                align="end"
                sideOffset={8}
                class="p-0 w-auto rounded-lg border-none shadow-2xl"
            >
                <VaultPopover onclose={() => (vaultPopoverOpen = false)} />
            </Popover.Content>
        </Popover.Root>
    </div>
</nav>
