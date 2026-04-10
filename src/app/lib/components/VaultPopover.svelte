<script lang="ts">
    import { open as openDialog } from '@tauri-apps/plugin-dialog';
    import { openVault, getRecentVaults, vaultStore, type VaultEntry } from '$lib/stores/vault.svelte';

    interface Props {
        onclose?: () => void;
    }

    let { onclose }: Props = $props();

    let recentVaults = $state<VaultEntry[]>([]);

    $effect(() => {
        getRecentVaults().then((vaults) => {
            recentVaults = vaults;
        });
    });

    async function selectRecent(entry: VaultEntry) {
        await openVault(entry.path);
        onclose?.();
    }

    async function pickFolder() {
        const selected = await openDialog({ directory: true, multiple: false });
        if (typeof selected === 'string' && selected) {
            await openVault(selected);
            onclose?.();
        }
    }
</script>

<div class="flex w-[280px] flex-col gap-1 rounded-lg bg-surface-container-highest p-3">
    <p class="label-lg mb-1 px-2 py-1 text-on-surface-variant opacity-60">Vaults</p>

    {#if recentVaults.length > 0}
        <ul class="flex flex-col">
            {#each recentVaults as entry (entry.path)}
                <li>
                    <button
                        class="flex w-full cursor-pointer items-center justify-between rounded-[4px] border-none bg-transparent px-2 py-1.5 text-left text-on-surface transition-colors duration-150 ease-out hover:bg-surface-container-high {entry.path === vaultStore.path ? 'text-primary' : ''}"
                        onclick={() => selectRecent(entry)}
                    >
                        <span class="truncate text-sm">{entry.name}</span>
                        {#if entry.path === vaultStore.path}
                            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true" class="ml-2 shrink-0">
                                <path d="M2 6L5 9L10 3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                            </svg>
                        {/if}
                    </button>
                </li>
            {/each}
        </ul>
    {/if}

    <div class="my-1 h-px bg-outline-variant/30"></div>

    <button
        class="flex w-full cursor-pointer items-center gap-2 rounded-[4px] border-none bg-transparent px-2 py-1.5 text-left text-sm text-on-surface-variant transition-colors duration-150 ease-out hover:bg-surface-container-high hover:text-on-surface"
        onclick={pickFolder}
    >
        <svg width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true" class="shrink-0">
            <path d="M1 3.5C1 2.67 1.67 2 2.5 2H4.5L5.5 3H9.5C10.33 3 11 3.67 11 4.5V9C11 9.83 10.33 10 9.5 10H2.5C1.67 10 1 9.33 1 8.5V3.5Z" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        Open another folder
    </button>
</div>
