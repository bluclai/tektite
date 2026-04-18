<!--
    AuraSuggestion — inline generative continuation card.

    Fixed-positioned over the CM6 scroll surface at the cursor anchor computed
    at request time. State drives the visual treatment:
      loading → pulsing glow + placeholder text
      ready   → static card with suggestion + Tab / Esc hints
      error   → terse destructive message + Esc hint

    All interaction (Tab accept / Esc dismiss) is handled upstream in
    EditorPane via CM6 keybindings — this component is purely presentational.
-->
<script lang="ts">
    import { auraStore } from '$lib/stores/aura.svelte';

    const visible = $derived(auraStore.state !== 'idle');
    const anchor = $derived(auraStore.anchor);

    const cardStyle = $derived.by(() => {
        if (!anchor) return 'display: none;';
        return `top: ${anchor.top}px; left: ${anchor.left}px; width: ${anchor.width}px;`;
    });
</script>

{#if visible && anchor}
    <div
        class="aura-card"
        class:aura-loading={auraStore.state === 'loading'}
        style={cardStyle}
        role="status"
        aria-live="polite"
    >
        <div class="aura-header">
            <span class="aura-icon"></span>
            <span class="eyebrow aura-label">Continue with Aura</span>
            <span class="aura-kbd-group">
                {#if auraStore.state === 'ready'}
                    <span class="kbd aura-kbd">Tab</span>
                    <span class="aura-kbd-sep">accept</span>
                    <span class="kbd aura-kbd">Esc</span>
                    <span class="aura-kbd-sep">dismiss</span>
                {:else if auraStore.state === 'loading'}
                    <span class="aura-kbd-sep">generating…</span>
                    <span class="kbd aura-kbd">Esc</span>
                {:else if auraStore.state === 'error'}
                    <span class="kbd aura-kbd">Esc</span>
                    <span class="aura-kbd-sep">dismiss</span>
                {/if}
            </span>
        </div>

        <div class="aura-body">
            {#if auraStore.state === 'loading'}
                <span class="aura-placeholder">…</span>
            {:else if auraStore.state === 'ready'}
                {auraStore.text}
            {:else if auraStore.state === 'error'}
                <span class="aura-error">{auraStore.error ?? 'Aura unavailable'}</span>
            {/if}
        </div>
    </div>
{/if}

<style>
    .aura-card {
        position: fixed;
        z-index: 30;
        display: flex;
        flex-direction: column;
        gap: 10px;
        padding: 14px 18px 16px;
        border-radius: 12px;
        background:
            linear-gradient(
                180deg,
                rgba(206, 189, 255, 0.05) 0%,
                rgba(206, 189, 255, 0.02) 100%
            );
        box-shadow:
            inset 0 0 0 1px rgba(206, 189, 255, 0.14),
            0 0 24px rgba(206, 189, 255, 0.05);
        pointer-events: none;
    }

    .aura-card.aura-loading {
        animation: aura-pulse 2s ease-in-out infinite;
    }

    .aura-header {
        display: flex;
        align-items: center;
        gap: 8px;
    }

    .aura-icon {
        width: 16px;
        height: 16px;
        border-radius: 4px;
        background: linear-gradient(135deg, #cebdff 0%, #8878c8 100%);
        box-shadow: 0 0 8px rgba(206, 189, 255, 0.25);
        flex-shrink: 0;
    }

    .aura-label {
        color: var(--color-tertiary);
        flex-shrink: 0;
    }

    .aura-kbd-group {
        margin-left: auto;
        display: inline-flex;
        align-items: center;
        gap: 6px;
    }

    .aura-kbd {
        background-color: rgba(206, 189, 255, 0.08);
        color: var(--color-tertiary);
    }

    .aura-kbd-sep {
        font-family: var(--font-sans);
        font-size: 10.5px;
        color: var(--color-text-ghost);
    }

    .aura-body {
        font-family: var(--font-prose);
        font-size: 17px;
        line-height: 28px;
        font-style: italic;
        color: #b8b5d4;
    }

    .aura-placeholder {
        color: var(--color-text-ghost);
    }

    .aura-error {
        color: var(--color-destructive, #ff6b6b);
        font-style: normal;
        font-family: var(--font-sans);
        font-size: 13px;
    }
</style>
