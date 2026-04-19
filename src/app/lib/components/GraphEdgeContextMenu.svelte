<script lang="ts">
	interface Props {
		x: number;
		y: number;
		sourceTitle: string;
		targetTitle: string;
		onLinkThese: () => void;
		onClose: () => void;
	}

	let { x, y, sourceTitle, targetTitle, onLinkThese, onClose }: Props = $props();
</script>

<svelte:window onclick={onClose} onkeydown={(e) => e.key === 'Escape' && onClose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="absolute z-40 min-w-[200px] rounded-md border border-outline-variant/20 bg-surface-container/95 py-1 text-[11px] text-on-surface shadow-xl backdrop-blur"
	style="left: {x}px; top: {y}px;"
	onclick={(e) => e.stopPropagation()}
	role="menu"
	tabindex={-1}
>
	<div class="truncate px-3 py-1 text-[10px] opacity-50">
		{sourceTitle} ↔ {targetTitle}
	</div>
	<div class="my-1 h-px bg-outline-variant/15"></div>
	<button
		type="button"
		class="block w-full px-3 py-1 text-left hover:bg-surface-container-high"
		onclick={() => {
			onLinkThese();
			onClose();
		}}
	>
		Link these
	</button>
</div>
