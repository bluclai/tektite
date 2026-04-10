<script lang="ts">
	import type { LeafPane } from '$lib/stores/workspace.svelte';
	import { workspaceStore } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';
	import TabBar from '$lib/components/TabBar.svelte';
	import EmptyPane from '$lib/components/EmptyPane.svelte';
	import EditorPane from '$lib/components/EditorPane.svelte';

	interface Props {
		pane: LeafPane;
	}

	let { pane }: Props = $props();

	const isActive = $derived(workspaceStore.activePaneId === pane.id);

	const activeTab = $derived(pane.tabs.find((t) => t.id === pane.activeTabId) ?? null);

	const absolutePath = $derived(
		activeTab && vaultStore.path ? `${vaultStore.path}/${activeTab.path}` : null,
	);

	function onSplitRight() {
		workspaceStore.splitPane(pane.id, 'horizontal');
	}

	function onSplitDown() {
		workspaceStore.splitPane(pane.id, 'vertical');
	}
</script>

<!--
	Clicking anywhere in the pane body makes it active.
	The tab bar shows a primary accent line when this pane is the active pane.
-->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="flex h-full flex-col overflow-hidden"
	onclick={() => workspaceStore.setActivePane(pane.id)}
>
	<TabBar
		tabs={pane.tabs}
		activeTabId={pane.activeTabId}
		paneId={pane.id}
		{isActive}
		{onSplitRight}
		{onSplitDown}
		onactivate={(id) => workspaceStore.activateTab(pane.id, id)}
		onclose={(id) => workspaceStore.closeTab(pane.id, id)}
	/>

	<div class="flex-1 overflow-hidden">
		{#if absolutePath === null}
			<EmptyPane />
		{:else}
			<!--
				Key on the tab ID so EditorPane is destroyed + recreated on tab switch
				giving each tab its own CM6 EditorView and independent undo history.
			-->
			{#key `${pane.activeTabId ?? 'none'}:${workspaceStore.previewMode ? 'preview' : 'source'}`}
				<EditorPane path={absolutePath} previewMode={workspaceStore.previewMode} />
			{/key}
		{/if}
	</div>
</div>
