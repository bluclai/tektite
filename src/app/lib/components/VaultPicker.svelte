<script lang="ts">
    import { onMount } from 'svelte';
    import { open as openDialog } from '@tauri-apps/plugin-dialog';
    import { openVault, getRecentVaults, type VaultEntry } from '$lib/stores/vault.svelte';

    let recentVaults = $state<VaultEntry[]>([]);
    let isOpening = $state(false);

    onMount(async () => {
        recentVaults = await getRecentVaults();
    });

    async function pickFolder() {
        const selected = await openDialog({ directory: true, multiple: false });
        if (typeof selected === 'string' && selected) {
            isOpening = true;
            await openVault(selected);
        }
    }

    async function selectRecent(entry: VaultEntry) {
        isOpening = true;
        await openVault(entry.path);
    }
</script>

<div class="fixed inset-0 z-[100] flex items-center justify-center bg-surface">
    <div class="flex w-full max-w-[440px] flex-col items-center gap-6 px-8 py-8">
        <!-- Wordmark -->
        <div class="flex items-center justify-center">
            <span class="font-sans text-[2rem] font-light tracking-[-0.04em] text-on-surface">
                tektite
            </span>
        </div>

        <p class="text-center text-sm text-on-surface-variant">
            Choose a folder to open as your vault
        </p>

        <button
            class="btn btn-primary w-full px-4 py-2.5 text-sm disabled:opacity-50"
            onclick={pickFolder}
            disabled={isOpening}
        >
            Open folder
        </button>

        {#if recentVaults.length > 0}
            <div class="flex w-full flex-col gap-2">
                <p class="label-lg px-2 text-on-surface-variant opacity-60">Recent</p>
                <ul class="flex flex-col">
                    {#each recentVaults as entry (entry.path)}
                        <li>
                            <button
                                class="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-[6px] border-none bg-transparent px-3 py-2 text-left text-inherit transition-colors duration-200 ease-out hover:bg-surface-container-high disabled:cursor-not-allowed disabled:opacity-50"
                                onclick={() => selectRecent(entry)}
                                disabled={isOpening}
                            >
                                <span class="text-sm font-medium text-on-surface">{entry.name}</span>
                                <span class="max-w-full truncate text-xs text-on-surface-variant">{entry.path}</span>
                            </button>
                        </li>
                    {/each}
                </ul>
            </div>
        {/if}
    </div>
</div>
