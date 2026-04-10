<script lang="ts">
    import { getCurrentWindow } from '@tauri-apps/api/window';
    import { platformStore } from '$lib/stores/platform.svelte';

    interface Props {
        title?: string;
    }

    let { title = '' }: Props = $props();

    const win = getCurrentWindow();

    async function minimize() {
        await win.minimize();
    }

    async function toggleMaximize() {
        if (await win.isMaximized()) {
            await win.unmaximize();
        } else {
            await win.maximize();
        }
    }

    async function close() {
        await win.close();
    }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<header
    data-tauri-drag-region
    ondblclick={toggleMaximize}
    class="relative flex h-9 w-full shrink-0 select-none items-center bg-surface-container-low"
>
    <!-- Centred file title -->
    <div class="pointer-events-none absolute inset-0 z-0 flex items-center justify-center">
        <span class="max-w-[40%] truncate text-xs text-on-surface-variant opacity-60">{title}</span>
    </div>

    {#if platformStore.value === 'macos'}
        <!-- macOS traffic lights: top-left -->
        <div class="relative z-10 flex items-center gap-1.5 pl-3" role="presentation">
            <button
                onclick={close}
                aria-label="Close window"
                title="Close"
                class="group flex h-3 w-3 cursor-default items-center justify-center rounded-full bg-[#ff5f57]"
            >
                <svg class="opacity-0 transition-opacity duration-150 group-hover:opacity-100" width="6" height="6" viewBox="0 0 6 6" fill="none">
                    <line x1="1" y1="1" x2="5" y2="5" stroke="rgba(0,0,0,0.5)" stroke-width="1.5" stroke-linecap="round"/>
                    <line x1="5" y1="1" x2="1" y2="5" stroke="rgba(0,0,0,0.5)" stroke-width="1.5" stroke-linecap="round"/>
                </svg>
            </button>
            <button
                onclick={minimize}
                aria-label="Minimize window"
                title="Minimize"
                class="group flex h-3 w-3 cursor-default items-center justify-center rounded-full bg-[#febc2e]"
            >
                <svg class="opacity-0 transition-opacity duration-150 group-hover:opacity-100" width="6" height="2" viewBox="0 0 6 2" fill="none">
                    <line x1="0" y1="1" x2="6" y2="1" stroke="rgba(0,0,0,0.5)" stroke-width="1.5" stroke-linecap="round"/>
                </svg>
            </button>
            <button
                onclick={toggleMaximize}
                aria-label="Maximize window"
                title="Maximize"
                class="group flex h-3 w-3 cursor-default items-center justify-center rounded-full bg-[#28c840]"
            >
                <svg class="opacity-0 transition-opacity duration-150 group-hover:opacity-100" width="6" height="6" viewBox="0 0 6 6" fill="none">
                    <path d="M1 5V1H5" stroke="rgba(0,0,0,0.5)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                    <path d="M5 1V5H1" stroke="rgba(0,0,0,0.5)" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
                </svg>
            </button>
        </div>
    {:else}
        <!-- Linux/Windows: standard window controls top-right -->
        <div class="absolute inset-y-0 right-0 z-10 flex items-center" role="presentation">
            <button
                onclick={minimize}
                aria-label="Minimize window"
                title="Minimize"
                class="flex h-full w-[46px] cursor-default items-center justify-center text-on-surface-variant transition-colors duration-150 ease-out hover:bg-surface-container-high"
            >
                <svg width="10" height="1" viewBox="0 0 10 1" fill="none">
                    <line x1="0" y1="0.5" x2="10" y2="0.5" stroke="currentColor" stroke-width="1"/>
                </svg>
            </button>
            <button
                onclick={toggleMaximize}
                aria-label="Maximize window"
                title="Maximize"
                class="flex h-full w-[46px] cursor-default items-center justify-center text-on-surface-variant transition-colors duration-150 ease-out hover:bg-surface-container-high"
            >
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
                    <rect x="0.5" y="0.5" width="9" height="9" stroke="currentColor" stroke-width="1"/>
                </svg>
            </button>
            <button
                onclick={close}
                aria-label="Close window"
                title="Close"
                class="flex h-full w-[46px] cursor-default items-center justify-center text-on-surface-variant transition-colors duration-150 ease-out hover:bg-[#c42b1c] hover:text-white"
            >
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
                    <line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" stroke-width="1" stroke-linecap="round"/>
                    <line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" stroke-width="1" stroke-linecap="round"/>
                </svg>
            </button>
        </div>
    {/if}
</header>
