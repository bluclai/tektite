<script lang="ts">
	/**
	 * PaneLayout — recursive pane tree renderer.
	 *
	 * Renders either a LeafPane (via LeafPane.svelte) or a SplitPane containing
	 * two PaneLayout children with a draggable resize handle between them.
	 *
	 * The drag-resize handle uses pointer capture so the split can be resized
	 * smoothly without leaving the element. Sizes are stored as percentages and
	 * clamped to 15–85 per side. The store is updated on pointerup to avoid
	 * triggering a save on every pixel.
	 */

	import type { PaneLayout } from '$lib/stores/workspace.svelte';
	import { workspaceStore } from '$lib/stores/workspace.svelte';
	import LeafPaneView from '$lib/components/LeafPane.svelte';
	// Self-import for recursion (svelte:self is deprecated in Svelte 5)
	import Self from '$lib/components/PaneLayout.svelte';

	interface Props {
		layout: PaneLayout;
	}

	let { layout }: Props = $props();

	// ---------------------------------------------------------------------------
	// Drag-resize state (only used when layout.type === 'split')
	// ---------------------------------------------------------------------------

	let containerEl = $state<HTMLElement | null>(null);
	let dragging = $state(false);

	function startDrag(e: PointerEvent) {
		if (layout.type !== 'split') return;
		dragging = true;
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
	}

	function onDragMove(e: PointerEvent) {
		if (!dragging || !containerEl || layout.type !== 'split') return;
		const rect = containerEl.getBoundingClientRect();
		let pct: number;
		if (layout.direction === 'horizontal') {
			pct = ((e.clientX - rect.left) / rect.width) * 100;
		} else {
			pct = ((e.clientY - rect.top) / rect.height) * 100;
		}
		const a = Math.min(85, Math.max(15, pct));
		// Immediate update drives reactive re-render during drag (no save)
		workspaceStore.resizeSplitImmediate(layout.id, [a, 100 - a]);
	}

	function endDrag(e: PointerEvent) {
		if (!dragging || layout.type !== 'split') return;
		dragging = false;
		const rect = containerEl!.getBoundingClientRect();
		let pct: number;
		if (layout.direction === 'horizontal') {
			pct = ((e.clientX - rect.left) / rect.width) * 100;
		} else {
			pct = ((e.clientY - rect.top) / rect.height) * 100;
		}
		const a = Math.min(85, Math.max(15, pct));
		workspaceStore.commitSplitResize(layout.id, [a, 100 - a]);
	}
</script>

{#if layout.type === 'leaf'}
	<LeafPaneView pane={layout} />
{:else}
	<!--
		SplitPane: two children separated by a resize handle.
		`flex-col` for horizontal direction (side by side uses flex-row).
		The handle is 4px wide/tall with a 2px transparent hit-area extension
		via negative margin so it's easy to grab.
	-->
	<div
		bind:this={containerEl}
		class="flex h-full w-full overflow-hidden {layout.direction === 'horizontal'
			? 'flex-row'
			: 'flex-col'}"
	>
		<!-- Child A -->
		<div
			class="overflow-hidden"
			style="flex: 0 0 {layout.sizes[0]}%; {layout.direction === 'horizontal'
				? 'min-width'
				: 'min-height'}: 15%"
		>
			<Self layout={layout.a} />
		</div>

		<!-- Resize handle -->
		<!-- svelte-ignore a11y_interactive_supports_focus -->
		<div
			role="separator"
			aria-orientation={layout.direction === 'horizontal' ? 'vertical' : 'horizontal'}
			class="group relative z-10 shrink-0 transition-colors duration-150
				{layout.direction === 'horizontal' ? 'w-[4px] cursor-col-resize' : 'h-[4px] cursor-row-resize'}
				bg-outline-variant/20 hover:bg-primary/40
				{dragging ? 'bg-primary/50' : ''}"
			style="{layout.direction === 'horizontal' ? 'margin: 0 -1px' : 'margin: -1px 0'}; touch-action: none;"
			onpointerdown={startDrag}
			onpointermove={onDragMove}
			onpointerup={endDrag}
			onpointercancel={endDrag}
		></div>

		<!-- Child B -->
		<div
			class="flex-1 overflow-hidden"
			style="{layout.direction === 'horizontal' ? 'min-width' : 'min-height'}: 15%"
		>
			<Self layout={layout.b} />
		</div>
	</div>
{/if}
