<script lang="ts">
	/**
	 * GraphSettingsDrawer — top-right anchored drawer for the whole-vault graph.
	 *
	 * Non-modal: the canvas stays interactive behind the drawer. Sections are
	 * collapsible; their open/closed state is owned by the parent so it can be
	 * persisted alongside the rest of the graph view state.
	 */
	import { ChevronDown, ChevronRight, X } from 'lucide-svelte';

	export type ColorMode = 'tag' | 'folder' | 'single';
	export type ReduceMotionMode = 'auto' | 'on' | 'off';
	export type PerformanceMode = 'auto' | 'high' | 'low';

	export interface GraphSettings {
		chargeStrength: number;
		linkDistance: number;
		centerStrength: number;

		k: number;
		minSimilarity: number;
		showSemanticEdges: boolean;

		colorBy: ColorMode;
		showOrphans: boolean;
		labelsAtFitAll: boolean;
		reduceMotion: ReduceMotionMode;

		tags: string[];
		folder: string;
		recencyDays: number;

		performance: PerformanceMode;
	}

	interface Props {
		settings: GraphSettings;
		availableTags: string[];
		openSections: string[];
		onSettingsChange: (partial: Partial<GraphSettings>) => void;
		onSectionToggle: (section: string) => void;
		onClose: () => void;
		onFocusActive: () => void;
		onResetLayout: () => void;
		onFindNodes: () => void;
	}

	let {
		settings,
		availableTags,
		openSections,
		onSettingsChange,
		onSectionToggle,
		onClose,
		onFocusActive,
		onResetLayout,
		onFindNodes,
	}: Props = $props();

	function isOpen(id: string): boolean {
		return openSections.includes(id);
	}

	function toggleTag(tag: string) {
		const next = settings.tags.includes(tag)
			? settings.tags.filter((t) => t !== tag)
			: [...settings.tags, tag];
		onSettingsChange({ tags: next });
	}

	function clearFilters() {
		onSettingsChange({ tags: [], folder: '', recencyDays: 0 });
	}

	const activeFilterCount = $derived(
		(settings.tags.length > 0 ? 1 : 0) +
			(settings.folder.trim().length > 0 ? 1 : 0) +
			(settings.recencyDays > 0 ? 1 : 0),
	);
</script>

<aside
	class="pointer-events-auto absolute right-0 top-0 z-20 flex h-full w-[min(320px,40%)] flex-col overflow-hidden border-l border-outline-variant/20 bg-surface-container/95 text-[11px] text-on-surface-variant shadow-lg backdrop-blur"
	aria-label="Graph settings"
>
	<header class="flex items-center justify-between border-b border-outline-variant/15 px-3 py-2">
		<span class="font-medium text-on-surface">Graph settings</span>
		<button
			type="button"
			class="rounded p-1 text-on-surface-variant opacity-70 hover:opacity-100"
			onclick={onClose}
			aria-label="Close settings"
		>
			<X size={14} />
		</button>
	</header>

	<div class="flex-1 overflow-y-auto">
		<!-- Forces -->
		{@render sectionHeader('forces', 'Forces')}
		{#if isOpen('forces')}
			<div class="flex flex-col gap-2 px-3 pb-3">
				{@render slider(
					'Charge',
					-400,
					-50,
					1,
					settings.chargeStrength,
					(v) => onSettingsChange({ chargeStrength: v }),
					settings.chargeStrength.toFixed(0),
				)}
				{@render slider(
					'Link distance',
					20,
					150,
					1,
					settings.linkDistance,
					(v) => onSettingsChange({ linkDistance: v }),
					settings.linkDistance.toFixed(0),
				)}
				{@render slider(
					'Center gravity',
					0,
					0.2,
					0.005,
					settings.centerStrength,
					(v) => onSettingsChange({ centerStrength: v }),
					settings.centerStrength.toFixed(3),
				)}
			</div>
		{/if}

		<!-- Edges -->
		{@render sectionHeader('edges', 'Edges')}
		{#if isOpen('edges')}
			<div class="flex flex-col gap-2 px-3 pb-3">
				{@render slider(
					'K (neighbours)',
					1,
					10,
					1,
					settings.k,
					(v) => onSettingsChange({ k: Math.round(v) }),
					String(settings.k),
				)}
				{@render slider(
					'Min similarity',
					0,
					1,
					0.01,
					settings.minSimilarity,
					(v) => onSettingsChange({ minSimilarity: v }),
					settings.minSimilarity.toFixed(2),
				)}
				<label class="flex items-center gap-2">
					<input
						type="checkbox"
						class="accent-primary"
						checked={settings.showSemanticEdges}
						onchange={(e) =>
							onSettingsChange({ showSemanticEdges: (e.target as HTMLInputElement).checked })}
					/>
					<span>Show semantic edges</span>
				</label>
			</div>
		{/if}

		<!-- Display -->
		{@render sectionHeader('display', 'Display')}
		{#if isOpen('display')}
			<div class="flex flex-col gap-2 px-3 pb-3">
				<label class="flex items-center justify-between gap-2">
					<span>Color by</span>
					<select
						class="rounded border border-outline-variant/20 bg-transparent px-1.5 py-0.5 text-[11px] text-on-surface"
						value={settings.colorBy}
						onchange={(e) =>
							onSettingsChange({ colorBy: (e.target as HTMLSelectElement).value as ColorMode })}
					>
						<option value="tag">Tag</option>
						<option value="folder">Folder</option>
						<option value="single">Single</option>
					</select>
				</label>
				<label class="flex items-center gap-2">
					<input
						type="checkbox"
						class="accent-primary"
						checked={settings.showOrphans}
						onchange={(e) =>
							onSettingsChange({ showOrphans: (e.target as HTMLInputElement).checked })}
					/>
					<span>Show orphans</span>
				</label>
				<label class="flex items-center gap-2">
					<input
						type="checkbox"
						class="accent-primary"
						checked={settings.labelsAtFitAll}
						onchange={(e) =>
							onSettingsChange({ labelsAtFitAll: (e.target as HTMLInputElement).checked })}
					/>
					<span>Labels at fit-all</span>
				</label>
				<label class="flex items-center justify-between gap-2">
					<span>Reduce motion</span>
					<select
						class="rounded border border-outline-variant/20 bg-transparent px-1.5 py-0.5 text-[11px] text-on-surface"
						value={settings.reduceMotion}
						onchange={(e) =>
							onSettingsChange({
								reduceMotion: (e.target as HTMLSelectElement).value as ReduceMotionMode,
							})}
					>
						<option value="auto">Auto</option>
						<option value="on">On</option>
						<option value="off">Off</option>
					</select>
				</label>
			</div>
		{/if}

		<!-- Filters -->
		{@render sectionHeader('filters', 'Filters', activeFilterCount)}
		{#if isOpen('filters')}
			<div class="flex flex-col gap-2 px-3 pb-3">
				<div class="flex flex-col gap-1">
					<div class="flex items-center justify-between">
						<span class="font-medium opacity-70">Tags</span>
						{#if settings.tags.length > 0}
							<button
								type="button"
								class="text-[10px] opacity-50 hover:opacity-90"
								onclick={() => onSettingsChange({ tags: [] })}
							>
								clear
							</button>
						{/if}
					</div>
					{#if availableTags.length === 0}
						<div class="opacity-40">No tags in vault</div>
					{:else}
						<div class="flex flex-wrap gap-1">
							{#each availableTags as tag (tag)}
								{@const active = settings.tags.includes(tag)}
								<button
									type="button"
									class="rounded-full border px-1.5 py-0.5 text-[10px] transition-opacity {active
										? 'border-primary/40 bg-primary/15 text-primary'
										: 'border-outline-variant/20 opacity-60 hover:opacity-100'}"
									onclick={() => toggleTag(tag)}
								>
									#{tag}
								</button>
							{/each}
						</div>
					{/if}
				</div>

				<label class="flex items-center gap-2">
					<span class="font-medium opacity-70">Folder</span>
					<input
						type="text"
						placeholder="e.g. journal/"
						value={settings.folder}
						oninput={(e) =>
							onSettingsChange({ folder: (e.target as HTMLInputElement).value })}
						class="flex-1 rounded border border-outline-variant/20 bg-transparent px-1.5 py-0.5 text-[11px] text-on-surface placeholder:opacity-40 focus:border-primary/40 focus:outline-none"
					/>
				</label>

				<label class="flex items-center gap-2">
					<span class="font-medium opacity-70">Recency</span>
					<input
						type="range"
						min="0"
						max="365"
						step="1"
						value={settings.recencyDays}
						oninput={(e) =>
							onSettingsChange({
								recencyDays: Number((e.target as HTMLInputElement).value),
							})}
						class="flex-1 accent-primary"
					/>
					<span class="w-14 text-right text-[10px] opacity-60">
						{settings.recencyDays === 0 ? 'all time' : `≤ ${settings.recencyDays}d`}
					</span>
				</label>

				{#if activeFilterCount > 0}
					<div class="flex justify-end">
						<button
							type="button"
							class="text-[10px] opacity-60 hover:opacity-100"
							onclick={clearFilters}
						>
							Clear all filters
						</button>
					</div>
				{/if}
			</div>
		{/if}

		<!-- Performance -->
		{@render sectionHeader('performance', 'Performance')}
		{#if isOpen('performance')}
			<div class="flex flex-col gap-2 px-3 pb-3">
				<label class="flex items-center justify-between gap-2">
					<span>Mode</span>
					<select
						class="rounded border border-outline-variant/20 bg-transparent px-1.5 py-0.5 text-[11px] text-on-surface"
						value={settings.performance}
						onchange={(e) =>
							onSettingsChange({
								performance: (e.target as HTMLSelectElement).value as PerformanceMode,
							})}
					>
						<option value="auto">Auto</option>
						<option value="high">High quality</option>
						<option value="low">Low impact</option>
					</select>
				</label>
			</div>
		{/if}

		<!-- Actions -->
		{@render sectionHeader('actions', 'Actions')}
		{#if isOpen('actions')}
			<div class="flex flex-col gap-1 px-3 pb-3">
				<button
					type="button"
					class="rounded border border-outline-variant/20 px-2 py-1 text-left hover:bg-surface-container-high"
					onclick={onFocusActive}
				>
					Focus active note
				</button>
				<button
					type="button"
					class="rounded border border-outline-variant/20 px-2 py-1 text-left hover:bg-surface-container-high"
					onclick={onResetLayout}
				>
					Reset layout
				</button>
				<button
					type="button"
					class="rounded border border-outline-variant/20 px-2 py-1 text-left opacity-50 hover:bg-surface-container-high"
					onclick={onFindNodes}
					title="Available in Phase 5"
				>
					Find nodes…
				</button>
			</div>
		{/if}
	</div>
</aside>

{#snippet sectionHeader(id: string, label: string, badge?: number)}
	<button
		type="button"
		class="flex w-full items-center justify-between px-3 py-1.5 text-left font-medium text-on-surface hover:bg-surface-container-high"
		onclick={() => onSectionToggle(id)}
		aria-expanded={isOpen(id)}
	>
		<span class="flex items-center gap-1.5">
			{#if isOpen(id)}
				<ChevronDown size={12} />
			{:else}
				<ChevronRight size={12} />
			{/if}
			<span>{label}</span>
		</span>
		{#if badge && badge > 0}
			<span class="rounded-full bg-primary/20 px-1.5 text-[10px] text-primary">{badge}</span>
		{/if}
	</button>
{/snippet}

{#snippet slider(
	label: string,
	min: number,
	max: number,
	step: number,
	value: number,
	onChange: (v: number) => void,
	display: string,
)}
	<label class="flex flex-col gap-0.5">
		<span class="flex items-center justify-between">
			<span class="opacity-70">{label}</span>
			<span class="text-[10px] opacity-60">{display}</span>
		</span>
		<input
			type="range"
			{min}
			{max}
			{step}
			{value}
			oninput={(e) => onChange(Number((e.target as HTMLInputElement).value))}
			class="accent-primary"
		/>
	</label>
{/snippet}
