<script lang="ts">
	import type { PaneTab } from '$lib/stores/workspace.svelte';
	import Tab from '$lib/components/Tab.svelte';
	import TabOverflow from '$lib/components/TabOverflow.svelte';

	interface Props {
		tabs: PaneTab[];
		activeTabId: string | null;
		paneId: string;
		/** Whether this pane is the globally active pane */
		isActive: boolean;
		onactivate: (tabId: string) => void;
		onclose: (tabId: string) => void;
		onSplitRight: () => void;
		onSplitDown: () => void;
		onCloseOthers: (tabId: string) => void;
		onCloseRight: (tabId: string) => void;
	}

	let { tabs, activeTabId, paneId, isActive, onactivate, onclose, onSplitRight, onSplitDown, onCloseOthers, onCloseRight }: Props =
		$props();

	let stripEl = $state<HTMLElement | null>(null);
	let overflowing = $state(false);

	$effect(() => {
		if (!stripEl) return;
		const ro = new ResizeObserver(() => checkOverflow());
		ro.observe(stripEl);
		return () => ro.disconnect();
	});

	$effect(() => {
		void tabs.length;
		checkOverflow();
	});

	function checkOverflow() {
		if (!stripEl) return;
		overflowing = stripEl.scrollWidth > stripEl.clientWidth;
	}
</script>

<!--
	Tab bar: 36px tall. Active pane gets a subtle primary accent on the bottom edge.
-->
<div
	class="relative flex h-9 shrink-0 overflow-hidden bg-surface-container-low transition-colors duration-200 ease-out"
>
	<!-- Active pane bottom accent -->
	{#if isActive}
		<span class="absolute inset-x-0 bottom-0 h-px bg-primary/40" aria-hidden="true"></span>
	{/if}

	<!-- Scrollable tab strip -->
	<div
		bind:this={stripEl}
		class="flex min-w-0 flex-1 overflow-hidden"
		role="tablist"
		aria-label="Open files"
	>
		{#each tabs as tab (tab.id)}
			<div class="tab-slot flex shrink">
				<Tab
					{tab}
					active={tab.id === activeTabId}
					onclick={() => onactivate(tab.id)}
					onclose={() => onclose(tab.id)}
					{onSplitRight}
					{onSplitDown}
					onCloseOthers={() => onCloseOthers(tab.id)}
					onCloseRight={() => onCloseRight(tab.id)}
				/>
			</div>
		{/each}
	</div>

	{#if overflowing}
		<TabOverflow {tabs} {activeTabId} {onactivate} />
	{/if}
</div>

<style>
	.tab-slot {
		min-width: 80px;
		max-width: 200px;
		flex: 1 1 200px;
	}
</style>
