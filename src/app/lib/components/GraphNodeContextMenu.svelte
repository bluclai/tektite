<script lang="ts">
	interface Props {
		x: number;
		y: number;
		title: string;
		onOpenInNewTab: () => void;
		onRevealInExplorer: () => void;
		onCopyPath: () => void;
		onLinkTo: () => void;
		onFocus: () => void;
		onClose: () => void;
	}

	let {
		x,
		y,
		title,
		onOpenInNewTab,
		onRevealInExplorer,
		onCopyPath,
		onLinkTo,
		onFocus,
		onClose,
	}: Props = $props();

	function act(fn: () => void) {
		return () => {
			fn();
			onClose();
		};
	}
</script>

<svelte:window onclick={onClose} onkeydown={(e) => e.key === 'Escape' && onClose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="absolute z-40 min-w-[180px] rounded-md border border-outline-variant/20 bg-surface-container/95 py-1 text-[11px] text-on-surface shadow-xl backdrop-blur"
	style="left: {x}px; top: {y}px;"
	onclick={(e) => e.stopPropagation()}
	role="menu"
	tabindex={-1}
>
	<div class="truncate px-3 py-1 text-[10px] opacity-50">{title}</div>
	<div class="my-1 h-px bg-outline-variant/15"></div>
	<button
		type="button"
		class="block w-full px-3 py-1 text-left hover:bg-surface-container-high"
		onclick={act(onOpenInNewTab)}
	>
		Open in new tab
	</button>
	<button
		type="button"
		class="block w-full px-3 py-1 text-left hover:bg-surface-container-high"
		onclick={act(onRevealInExplorer)}
	>
		Reveal in file explorer
	</button>
	<button
		type="button"
		class="block w-full px-3 py-1 text-left hover:bg-surface-container-high"
		onclick={act(onCopyPath)}
	>
		Copy path
	</button>
	<button
		type="button"
		class="block w-full px-3 py-1 text-left hover:bg-surface-container-high"
		onclick={act(onLinkTo)}
	>
		Link to…
	</button>
	<button
		type="button"
		class="block w-full px-3 py-1 text-left hover:bg-surface-container-high"
		onclick={act(onFocus)}
	>
		Focus on this node
	</button>
</div>
