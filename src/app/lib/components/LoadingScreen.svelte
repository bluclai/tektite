<script lang="ts">
    // Intentionally minimal — this renders before any async init completes.
    // Keep it dependency-free so it paints immediately.
</script>

<div class="loading-root">
    <div class="wordmark">
        <span class="wordmark-tek">tek</span><span class="wordmark-tite">tite</span>
    </div>
    <div class="pulse-ring" aria-hidden="true"></div>
</div>

<style>
    .loading-root {
        position: fixed;
        inset: 0;
        background: #131314;
        display: flex;
        align-items: center;
        justify-content: center;
        flex-direction: column;
        gap: 32px;
    }

    /* Wordmark */
    .wordmark {
        font-family: "Inter Variable", "Inter", sans-serif;
        font-size: 1.75rem;
        font-weight: 300;
        letter-spacing: -0.04em;
        user-select: none;

        /* Fade-in after a short delay so it doesn't flash on fast loads */
        opacity: 0;
        animation: fade-in 400ms ease-out 120ms forwards;
    }

    .wordmark-tek {
        color: #e8e8ea;
    }

    .wordmark-tite {
        color: #bdc2ff;
    }

    /* Subtle ambient pulse — a single ring that expands and fades */
    .pulse-ring {
        width: 6px;
        height: 6px;
        border-radius: 50%;
        background: #bdc2ff;
        opacity: 0;
        animation:
            fade-in 400ms ease-out 200ms forwards,
            pulse 2s ease-out 600ms infinite;
    }

    @keyframes fade-in {
        to {
            opacity: 1;
        }
    }

    @keyframes pulse {
        0%   { box-shadow: 0 0 0 0 rgba(189, 194, 255, 0.35); }
        60%  { box-shadow: 0 0 0 12px rgba(189, 194, 255, 0); }
        100% { box-shadow: 0 0 0 0 rgba(189, 194, 255, 0); }
    }
</style>
