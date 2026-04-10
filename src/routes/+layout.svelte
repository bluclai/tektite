<script lang="ts">
    import { onMount } from 'svelte';
    import { initPlatform } from '$lib/stores/platform.svelte';
    import { vaultStore, openVault, getRecentVaults } from '$lib/stores/vault.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import Shell from '$lib/components/Shell.svelte';
    import VaultPicker from '$lib/components/VaultPicker.svelte';
    import '../app.css';

    let { children } = $props();

    let ready = $state(false);

    onMount(async () => {
        // Run in parallel — platform detection and workspace restore are independent
        await Promise.all([initPlatform(), workspaceStore.load()]);

        // Auto-open the most recently used vault. Silently falls through to
        // VaultPicker if none exists or the path is no longer accessible.
        const recents = await getRecentVaults().catch(() => []);
        if (recents.length > 0) {
            await openVault(recents[0].path).catch(() => {});
        }

        ready = true;
        document.getElementById('app-loading')?.remove();
    });
</script>

<div class="app-root">
    {#if !ready}
        <!-- loading screen is the static #app-loading div in app.html -->
    {:else if vaultStore.path === null}
        <VaultPicker />
    {:else}
        <Shell>
            {@render children()}
        </Shell>
    {/if}
</div>

<style>
    :global(html, body) {
        height: 100%;
        overflow: hidden;
    }

    .app-root {
        display: flex;
        flex-direction: column;
        height: 100vh;
        background-color: var(--color-surface);
        overflow: hidden;
    }
</style>
