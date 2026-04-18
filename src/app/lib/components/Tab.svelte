<script lang="ts">
	import type { PaneTab } from '$lib/stores/workspace.svelte';
	import {
		ContextMenu,
		ContextMenuContent,
		ContextMenuSeparator,
		ContextMenuTrigger,
		ContextMenuItem,
	} from '$lib/components/ui/context-menu';

	interface Props {
		tab: PaneTab;
		active: boolean;
		onclick: () => void;
		onclose: () => void;
		onSplitRight: () => void;
		onSplitDown: () => void;
		onCloseOthers: () => void;
		onCloseRight: () => void;
	}

	let { tab, active, onclick, onclose, onSplitRight, onSplitDown, onCloseOthers, onCloseRight }: Props =
		$props();
</script>

<!--
	Min-width 80px / max-width 200px enforced in TabBar via .tab-slot flex sizing.
-->
<ContextMenu>
	<ContextMenuTrigger>
		<div
			role="tab"
			aria-selected={active}
			tabindex={active ? 0 : -1}
			class="group relative flex w-full min-w-0 cursor-pointer select-none items-center gap-1.5 px-3 transition-colors duration-200 ease-out
			{active
			? 'bg-surface text-on-surface'
			: 'bg-surface-container-low text-on-surface-variant hover:bg-surface-container hover:text-on-surface'}"
			onclick={onclick}
			onkeydown={(e) => e.key === 'Enter' && onclick()}
		>
			<!-- Active tab top indicator -->
			{#if active}
				<span class="absolute inset-x-0 top-0 h-px bg-primary" aria-hidden="true"></span>
			{/if}

			<!-- Filename -->
			<span class="min-w-0 flex-1 truncate text-xs">{tab.name}</span>

			<!-- Close button -->
			<button
				class="flex h-4 w-4 shrink-0 items-center justify-center rounded-sm text-on-surface-variant transition-all duration-100 ease-out hover:bg-surface-container-high hover:text-on-surface
				{active ? 'opacity-60 hover:opacity-100' : 'opacity-0 group-hover:opacity-60 group-hover:hover:opacity-100'}"
				onclick={(e) => { e.stopPropagation(); onclose(); }}
				aria-label="Close {tab.name}"
				tabindex={-1}
			>
				<svg width="8" height="8" viewBox="0 0 8 8" fill="none" aria-hidden="true">
					<line x1="1" y1="1" x2="7" y2="7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
					<line x1="7" y1="1" x2="1" y2="7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
				</svg>
			</button>
		</div>
	</ContextMenuTrigger>

	<ContextMenuContent>
		<ContextMenuItem onselect={onclose}>
			Close
		</ContextMenuItem>
		<ContextMenuItem onselect={onCloseOthers}>
			Close others
		</ContextMenuItem>
		<ContextMenuItem onselect={onCloseRight}>
			Close to the right
		</ContextMenuItem>
		<ContextMenuSeparator />
		<ContextMenuItem onselect={onSplitRight}>
			<!-- Vertical divider icon -->
			<svg width="13" height="13" viewBox="0 0 13 13" fill="none" aria-hidden="true">
				<rect x="1" y="2" width="4.5" height="9" rx="1" stroke="currentColor" stroke-width="1.2" />
				<rect x="7.5" y="2" width="4.5" height="9" rx="1" stroke="currentColor" stroke-width="1.2" />
			</svg>
			Split right
		</ContextMenuItem>
		<ContextMenuItem onselect={onSplitDown}>
			<!-- Horizontal divider icon -->
			<svg width="13" height="13" viewBox="0 0 13 13" fill="none" aria-hidden="true">
				<rect x="2" y="1" width="9" height="4.5" rx="1" stroke="currentColor" stroke-width="1.2" />
				<rect x="2" y="7.5" width="9" height="4.5" rx="1" stroke="currentColor" stroke-width="1.2" />
			</svg>
			Split down
		</ContextMenuItem>
	</ContextMenuContent>
</ContextMenu>

<style>
	:global(.cn-context-menu-trigger[data-slot="context-menu-trigger"]) {
		display: flex;
		width: 100%;
	}
</style>