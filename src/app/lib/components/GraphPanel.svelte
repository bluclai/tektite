<script lang="ts">
	/**
	 * GraphPanel — interactive local-neighborhood graph centered on the active note.
	 *
	 * Link edges come from `graph_get_neighborhood`; semantic similarity edges
	 * (rendered as dotted lines to unlinked-but-similar notes) come from
	 * `graph_get_semantic_edges`. A d3-force simulation lays both out; SVG
	 * rendering supports click-to-navigate, zoom, pan, hover tooltip, depth
	 * toggle (1 / 2), and a "Show similar notes" toggle persisted to
	 * localStorage.
	 */
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { onMount, untrack } from 'svelte';
	import { Share2, SlidersHorizontal } from 'lucide-svelte';
	import {
		forceCenter,
		forceCollide,
		forceLink,
		forceManyBody,
		forceSimulation,
		type Simulation,
		type SimulationLinkDatum,
		type SimulationNodeDatum,
	} from 'd3-force';
	import { workspaceStore, allLeaves } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';

	// ---------------------------------------------------------------------------
	// Wire-format types (mirror tektite_index::graph on the Rust side)
	// ---------------------------------------------------------------------------

	interface GraphNodeDTO {
		id: string;
		path: string;
		title: string;
		tags: string[];
		modified: number;
		link_count: number;
	}

	interface GraphEdgeDTO {
		source: string;
		target: string;
		kind: string;
		score?: number;
	}

	interface GraphDataDTO {
		nodes: GraphNodeDTO[];
		edges: GraphEdgeDTO[];
	}

	// ---------------------------------------------------------------------------
	// Simulation types — d3-force mutates nodes in place adding x/y/vx/vy/fx/fy
	// ---------------------------------------------------------------------------

	interface SimNode extends SimulationNodeDatum {
		id: string;
		path: string;
		title: string;
		tags: string[];
		modified: number;
		linkCount: number;
		isCenter: boolean;
		isSemanticOnly: boolean;
	}

	interface SimEdge extends SimulationLinkDatum<SimNode> {
		kind: string;
		score?: number;
	}

	// ---------------------------------------------------------------------------
	// Active-file derivation (mirror BacklinksPanel)
	// ---------------------------------------------------------------------------

	let activeFilePath = $derived.by(() => {
		const leaves = allLeaves(workspaceStore.paneTree);
		const activeLeaf = leaves.find((l) => l.id === workspaceStore.activePaneId);
		if (!activeLeaf || !activeLeaf.activeTabId) return null;
		const tab = activeLeaf.tabs.find((t) => t.id === activeLeaf.activeTabId);
		if (!tab) return null;

		const vaultRoot = vaultStore.path;
		if (vaultRoot && tab.path.startsWith(vaultRoot + '/')) {
			return tab.path.slice(vaultRoot.length + 1);
		}
		return tab.path;
	});

	// ---------------------------------------------------------------------------
	// Reactive state
	// ---------------------------------------------------------------------------

	const SHOW_SEMANTIC_KEY = 'tektite.graph.showSemantic';
	const FILTERS_KEY = 'tektite.graph.filters';
	const FILTER_PANEL_OPEN_KEY = 'tektite.graph.filterPanelOpen';

	interface GraphFiltersState {
		tags: string[];
		folder: string;
		/** 0 means "no recency filter". */
		recencyDays: number;
	}

	function readShowSemantic(): boolean {
		try {
			const v = localStorage.getItem(SHOW_SEMANTIC_KEY);
			return v === null ? true : v === '1';
		} catch {
			return true;
		}
	}

	function readFilters(): GraphFiltersState {
		const fallback: GraphFiltersState = { tags: [], folder: '', recencyDays: 0 };
		try {
			const raw = localStorage.getItem(FILTERS_KEY);
			if (!raw) return fallback;
			const parsed = JSON.parse(raw);
			return {
				tags: Array.isArray(parsed.tags) ? parsed.tags.filter((t: unknown) => typeof t === 'string') : [],
				folder: typeof parsed.folder === 'string' ? parsed.folder : '',
				recencyDays: Number.isFinite(parsed.recencyDays) ? Math.max(0, Math.min(365, parsed.recencyDays)) : 0,
			};
		} catch {
			return fallback;
		}
	}

	function readFilterPanelOpen(): boolean {
		try {
			return localStorage.getItem(FILTER_PANEL_OPEN_KEY) === '1';
		} catch {
			return false;
		}
	}

	let depth = $state<1 | 2>(1);
	let showSemantic = $state<boolean>(readShowSemantic());
	let filters = $state<GraphFiltersState>(readFilters());
	let filterPanelOpen = $state<boolean>(readFilterPanelOpen());
	let availableTags = $state<string[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);
	let graph = $state<GraphDataDTO | null>(null);
	let width = $state(280);
	let height = $state(400);
	let viewTransform = $state({ x: 0, y: 0, k: 1 });
	let hoveredNode = $state<SimNode | null>(null);
	let hoveredEdge = $state<SimEdge | null>(null);

	// Simulation state — imperative; positions tracked via $state so Svelte re-renders.
	let simNodes = $state<SimNode[]>([]);
	let simEdges = $state<SimEdge[]>([]);
	let simulation: Simulation<SimNode, SimEdge> | null = null;
	let containerEl = $state<HTMLDivElement | null>(null);
	let panning = $state(false);

	// Debounce timer for active-file changes
	let fetchTimer: ReturnType<typeof setTimeout> | null = null;

	// ---------------------------------------------------------------------------
	// Fetch + simulation rebuild
	// ---------------------------------------------------------------------------

	function edgeKey(a: string, b: string): string {
		// Undirected key — wiki-links are directional but for "is this pair
		// already connected?" checks we treat both orderings as the same.
		return a < b ? `${a}\u0000${b}` : `${b}\u0000${a}`;
	}

	function buildFilterPayload(f: GraphFiltersState): {
		tags: string[] | null;
		folder: string | null;
		modified_after: number | null;
	} | null {
		const hasTag = f.tags.length > 0;
		const hasFolder = f.folder.trim().length > 0;
		const hasRecency = f.recencyDays > 0;
		if (!hasTag && !hasFolder && !hasRecency) return null;
		return {
			tags: hasTag ? f.tags : null,
			folder: hasFolder ? f.folder.trim() : null,
			modified_after: hasRecency
				? Math.floor(Date.now() / 1000) - f.recencyDays * 86400
				: null,
		};
	}

	async function fetchGraph(
		centerPath: string,
		d: 1 | 2,
		withSemantic: boolean,
		f: GraphFiltersState,
	) {
		loading = true;
		error = null;
		try {
			const linkReq = invoke<GraphDataDTO>('graph_get_neighborhood', {
				centerPath,
				depth: d,
				filters: buildFilterPayload(f),
			});
			const semanticReq: Promise<GraphDataDTO> = withSemantic
				? invoke<GraphDataDTO>('graph_get_semantic_edges', {
						centerPath,
						limit: 8,
					})
				: Promise.resolve({ nodes: [], edges: [] });

			const [linkData, semData] = await Promise.all([linkReq, semanticReq]);

			const linkEdgeKeys = new Set(
				linkData.edges.map((e) => edgeKey(e.source, e.target)),
			);
			const linkNodeIds = new Set(linkData.nodes.map((n) => n.id));

			const nodes: GraphNodeDTO[] = [...linkData.nodes];
			for (const n of semData.nodes) {
				if (!linkNodeIds.has(n.id)) {
					nodes.push(n);
					linkNodeIds.add(n.id);
				}
			}

			const edges: GraphEdgeDTO[] = [...linkData.edges];
			for (const e of semData.edges) {
				if (linkEdgeKeys.has(edgeKey(e.source, e.target))) continue;
				if (!linkNodeIds.has(e.source) || !linkNodeIds.has(e.target)) continue;
				edges.push(e);
			}

			graph = { nodes, edges };
			const semanticOnly = new Set(nodes.filter((n) => !linkData.nodes.some((l) => l.id === n.id)).map((n) => n.id));
			rebuildSimulation(graph, centerPath, semanticOnly);
		} catch (e) {
			error = String(e);
			graph = null;
			simNodes = [];
			simEdges = [];
		} finally {
			loading = false;
		}
	}

	function rebuildSimulation(
		data: GraphDataDTO,
		centerPath: string,
		semanticOnly: Set<string>,
	) {
		simulation?.stop();

		const w = width;
		const h = height;
		const cx = w / 2;
		const cy = h / 2;

		const nodes: SimNode[] = data.nodes.map((n) => {
			const isCenter = n.path === centerPath;
			return {
				id: n.id,
				path: n.path,
				title: n.title,
				tags: n.tags,
				modified: n.modified,
				linkCount: n.link_count,
				isCenter,
				isSemanticOnly: semanticOnly.has(n.id),
				// Seed center at middle, others randomly around it
				x: isCenter ? cx : cx + (Math.random() - 0.5) * 80,
				y: isCenter ? cy : cy + (Math.random() - 0.5) * 80,
				fx: isCenter ? cx : null,
				fy: isCenter ? cy : null,
			};
		});

		const nodeById = new Map(nodes.map((n) => [n.id, n]));
		const edges: SimEdge[] = [];
		for (const e of data.edges) {
			const source = nodeById.get(e.source);
			const target = nodeById.get(e.target);
			if (source && target)
				edges.push({ source, target, kind: e.kind, score: e.score });
		}

		const sim = forceSimulation<SimNode, SimEdge>(nodes)
			.force(
				'link',
				forceLink<SimNode, SimEdge>(edges)
					.id((d) => d.id)
					.distance((e) => (e.kind === 'semantic' ? 90 : 60))
					// Semantic edges pull weakly so they don't distort the link
					// skeleton — they are hints, not structure.
					.strength((e) => (e.kind === 'semantic' ? 0.25 : 0.7)),
			)
			.force('charge', forceManyBody<SimNode>().strength(-180))
			.force('center', forceCenter(cx, cy).strength(0.05))
			.force('collide', forceCollide<SimNode>().radius((d) => nodeRadius(d) + 4))
			.alphaDecay(0.05);

		// Settle offscreen before the first paint so the layout isn't jittery.
		sim.stop();
		for (let i = 0; i < 100; i++) sim.tick();

		simNodes = nodes;
		simEdges = edges;

		sim.on('tick', () => {
			// Copy into new arrays so Svelte's proxy notices the mutation.
			simNodes = [...nodes];
			simEdges = [...edges];
		});
		sim.alpha(0.4).restart();
		simulation = sim;
	}

	function nodeRadius(n: SimNode): number {
		if (n.isCenter) return 8;
		// Map link_count (0..) into 3.5..7
		const c = Math.max(0, Math.min(40, n.linkCount));
		return 3.5 + (c / 40) * 3.5;
	}

	function nodeLabel(n: SimNode): string {
		const label = n.title || n.path.split('/').pop() || n.path;
		return label.length > 22 ? label.slice(0, 21) + '…' : label;
	}

	// ---------------------------------------------------------------------------
	// Container size (ResizeObserver) and active-file → fetch
	// ---------------------------------------------------------------------------

	onMount(() => {
		const ro = new ResizeObserver((entries) => {
			for (const entry of entries) {
				const rect = entry.contentRect;
				width = Math.max(80, rect.width);
				height = Math.max(120, rect.height);
			}
		});
		if (containerEl) ro.observe(containerEl);

		const unlistenIndex = listen('index:stats-changed', () => {
			const path = untrack(() => activeFilePath);
			if (path && path.endsWith('.md')) {
				void fetchGraph(
					path,
					untrack(() => depth),
					untrack(() => showSemantic),
					untrack(() => filters),
				);
			}
			void refreshAvailableTags();
		});

		void refreshAvailableTags();

		return () => {
			ro.disconnect();
			simulation?.stop();
			simulation = null;
			if (fetchTimer !== null) clearTimeout(fetchTimer);
			void unlistenIndex.then((fn) => fn());
		};
	});

	// Re-fetch when inputs change (300ms debounce on file swaps).
	$effect(() => {
		const path = activeFilePath;
		const d = depth;
		const withSemantic = showSemantic;
		const f = filters;
		if (fetchTimer !== null) clearTimeout(fetchTimer);
		if (!path || !path.endsWith('.md')) {
			graph = null;
			simNodes = [];
			simEdges = [];
			simulation?.stop();
			simulation = null;
			return;
		}
		fetchTimer = setTimeout(() => {
			void fetchGraph(path, d, withSemantic, f);
		}, 300);
	});

	async function refreshAvailableTags() {
		try {
			availableTags = await invoke<string[]>('index_list_all_tags');
		} catch {
			availableTags = [];
		}
	}

	function persistFilters() {
		try {
			localStorage.setItem(FILTERS_KEY, JSON.stringify(filters));
		} catch {
			// Ignore — session state still updates.
		}
	}

	function toggleFilterTag(tag: string) {
		const next = filters.tags.includes(tag)
			? filters.tags.filter((t) => t !== tag)
			: [...filters.tags, tag];
		filters = { ...filters, tags: next };
		persistFilters();
	}

	function setFolderFilter(e: Event) {
		const next = (e.currentTarget as HTMLInputElement).value;
		filters = { ...filters, folder: next };
		persistFilters();
	}

	function setRecencyDays(e: Event) {
		const raw = parseInt((e.currentTarget as HTMLInputElement).value, 10);
		const next = Number.isFinite(raw) ? Math.max(0, Math.min(365, raw)) : 0;
		filters = { ...filters, recencyDays: next };
		persistFilters();
	}

	function clearFilters() {
		filters = { tags: [], folder: '', recencyDays: 0 };
		persistFilters();
	}

	function toggleFilterPanel() {
		filterPanelOpen = !filterPanelOpen;
		try {
			localStorage.setItem(FILTER_PANEL_OPEN_KEY, filterPanelOpen ? '1' : '0');
		} catch {
			// Ignore.
		}
	}

	let activeFilterCount = $derived(
		(filters.tags.length > 0 ? 1 : 0)
			+ (filters.folder.trim().length > 0 ? 1 : 0)
			+ (filters.recencyDays > 0 ? 1 : 0),
	);

	function toggleSemantic(e: Event) {
		const next = (e.currentTarget as HTMLInputElement).checked;
		showSemantic = next;
		try {
			localStorage.setItem(SHOW_SEMANTIC_KEY, next ? '1' : '0');
		} catch {
			// Ignore — storage may be disabled; state still updates for the session.
		}
	}

	function edgeTooltipPos(e: SimEdge): { left: number; top: number } {
		const sx = (edgeEndpoint(e.source, 'x') + edgeEndpoint(e.target, 'x')) / 2;
		const sy = (edgeEndpoint(e.source, 'y') + edgeEndpoint(e.target, 'y')) / 2;
		return {
			left: sx * viewTransform.k + viewTransform.x + 8,
			top: sy * viewTransform.k + viewTransform.y + 8,
		};
	}

	// ---------------------------------------------------------------------------
	// Interaction
	// ---------------------------------------------------------------------------

	function handleNodeClick(n: SimNode) {
		const vaultRoot = vaultStore.path;
		const absPath =
			vaultRoot && !n.path.startsWith(vaultRoot) ? `${vaultRoot}/${n.path}` : n.path;
		workspaceStore.openTab(absPath);
	}

	// Zoom — wheel scales around cursor
	function onWheel(e: WheelEvent) {
		e.preventDefault();
		const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
		const rect = (e.currentTarget as SVGElement).getBoundingClientRect();
		const mx = e.clientX - rect.left;
		const my = e.clientY - rect.top;
		// Scale around cursor: new_translate = mouse - (mouse - old_translate) * factor
		const k = Math.min(4, Math.max(0.25, viewTransform.k * factor));
		const scale = k / viewTransform.k;
		viewTransform = {
			x: mx - (mx - viewTransform.x) * scale,
			y: my - (my - viewTransform.y) * scale,
			k,
		};
	}

	// Pan — drag background
	let panStart = { x: 0, y: 0, tx: 0, ty: 0 };

	function onBackgroundPointerDown(e: PointerEvent) {
		if (e.button !== 0) return;
		panning = true;
		panStart = { x: e.clientX, y: e.clientY, tx: viewTransform.x, ty: viewTransform.y };
		(e.currentTarget as Element).setPointerCapture(e.pointerId);
	}

	function onBackgroundPointerMove(e: PointerEvent) {
		if (!panning) return;
		viewTransform = {
			...viewTransform,
			x: panStart.tx + (e.clientX - panStart.x),
			y: panStart.ty + (e.clientY - panStart.y),
		};
	}

	function onBackgroundPointerUp(e: PointerEvent) {
		if (!panning) return;
		panning = false;
		(e.currentTarget as Element).releasePointerCapture(e.pointerId);
	}

	function resetView() {
		viewTransform = { x: 0, y: 0, k: 1 };
	}

	// Edge endpoint extraction — d3-force swaps string IDs for node refs after init,
	// but the type is `string | number | SimNode` since we use SimulationLinkDatum.
	function edgeEndpoint(end: SimEdge['source'], axis: 'x' | 'y'): number {
		if (typeof end === 'string' || typeof end === 'number') return 0;
		return end[axis] ?? 0;
	}

	function tooltipPos(n: SimNode): { left: number; top: number } {
		const sx = (n.x ?? 0) * viewTransform.k + viewTransform.x;
		const sy = (n.y ?? 0) * viewTransform.k + viewTransform.y;
		return { left: sx + 10, top: sy + 10 };
	}

	// ---------------------------------------------------------------------------
	// Color-by-folder — stable hue per folder (FNV-1a hash of the folder path).
	// ---------------------------------------------------------------------------

	function folderOf(p: string): string {
		const i = p.lastIndexOf('/');
		return i === -1 ? '' : p.slice(0, i);
	}

	function hashHue(s: string): number {
		// FNV-1a — tiny, deterministic, good-enough spread for color assignment.
		let h = 0x811c9dc5;
		for (let i = 0; i < s.length; i++) {
			h ^= s.charCodeAt(i);
			h = Math.imul(h, 0x01000193);
		}
		return ((h >>> 0) % 360);
	}

	function nodeColor(n: SimNode): string {
		if (n.isCenter) return 'var(--color-primary)';
		const folder = folderOf(n.path);
		if (!folder) return 'color-mix(in srgb, var(--color-on-surface-variant) 60%, transparent)';
		return `hsl(${hashHue(folder)} 55% 60%)`;
	}

	// ---------------------------------------------------------------------------
	// Edge context menu — "Link these" on semantic edges
	// ---------------------------------------------------------------------------

	let edgeMenu = $state<{ x: number; y: number; edge: SimEdge } | null>(null);
	let toast = $state<string | null>(null);
	let toastTimer: ReturnType<typeof setTimeout> | null = null;

	function showToast(msg: string) {
		toast = msg;
		if (toastTimer !== null) clearTimeout(toastTimer);
		toastTimer = setTimeout(() => (toast = null), 2000);
	}

	function onEdgeContextMenu(e: MouseEvent, edge: SimEdge) {
		if (edge.kind !== 'semantic') return;
		e.preventDefault();
		const rect = containerEl?.getBoundingClientRect();
		const left = rect ? e.clientX - rect.left : e.clientX;
		const top = rect ? e.clientY - rect.top : e.clientY;
		edgeMenu = { x: left, y: top, edge };
	}

	function closeEdgeMenu() {
		edgeMenu = null;
	}

	async function linkTheseEdge(edge: SimEdge) {
		closeEdgeMenu();
		if (typeof edge.source === 'string' || typeof edge.source === 'number') return;
		if (typeof edge.target === 'string' || typeof edge.target === 'number') return;
		const sourcePath = edge.source.path;
		const targetPath = edge.target.path;
		try {
			await invoke('graph_append_wiki_link', { sourcePath, targetPath });
			showToast(`Linked → ${edge.target.title || targetPath}`);
		} catch (err) {
			showToast(`Failed: ${String(err)}`);
		}
	}
</script>

<div class="flex h-full flex-col overflow-hidden">
	<!-- Toolbar: depth toggle + reset view -->
	<div class="flex shrink-0 items-center gap-2 border-b border-outline-variant/10 px-3 py-1.5">
		<div class="flex items-center gap-1 text-xs text-on-surface-variant opacity-60">
			<span>Depth</span>
			<button
				type="button"
				class="rounded px-1.5 py-0.5 text-xs transition-opacity {depth === 1
					? 'bg-primary/20 text-primary opacity-100'
					: 'opacity-50 hover:opacity-80'}"
				onclick={() => (depth = 1)}
			>
				1
			</button>
			<button
				type="button"
				class="rounded px-1.5 py-0.5 text-xs transition-opacity {depth === 2
					? 'bg-primary/20 text-primary opacity-100'
					: 'opacity-50 hover:opacity-80'}"
				onclick={() => (depth = 2)}
			>
				2
			</button>
		</div>
		<div class="flex-1"></div>
		<label
			class="flex cursor-pointer items-center gap-1 text-xs text-on-surface-variant opacity-60 hover:opacity-90"
			title="Show dotted edges to semantically similar notes"
		>
			<input
				type="checkbox"
				class="h-3 w-3 accent-primary"
				checked={showSemantic}
				onchange={toggleSemantic}
			/>
			<span>Similar</span>
		</label>
		<button
			type="button"
			class="relative flex items-center gap-1 rounded px-1.5 py-0.5 text-xs transition-opacity {filterPanelOpen || activeFilterCount > 0
				? 'bg-primary/10 text-primary opacity-100'
				: 'text-on-surface-variant opacity-50 hover:opacity-90'}"
			onclick={toggleFilterPanel}
			title="Filters"
			aria-expanded={filterPanelOpen}
		>
			<SlidersHorizontal size={12} strokeWidth={1.6} />
			{#if activeFilterCount > 0}
				<span class="text-[10px]">({activeFilterCount})</span>
			{/if}
		</button>
		<button
			type="button"
			class="rounded px-2 py-0.5 text-xs text-on-surface-variant opacity-50 transition-opacity hover:opacity-90"
			onclick={resetView}
			title="Reset view"
		>
			Reset
		</button>
	</div>

	<!-- Filter panel -->
	{#if filterPanelOpen}
		<div class="flex shrink-0 flex-col gap-2 border-b border-outline-variant/10 bg-surface-container-low/40 px-3 py-2 text-xs">
			<!-- Tags -->
			<div class="flex flex-col gap-1">
				<div class="flex items-center justify-between">
					<span class="font-medium text-on-surface-variant opacity-70">Tags</span>
					{#if filters.tags.length > 0}
						<button
							type="button"
							class="text-[10px] text-on-surface-variant opacity-50 hover:opacity-90"
							onclick={() => {
								filters = { ...filters, tags: [] };
								persistFilters();
							}}
						>
							clear
						</button>
					{/if}
				</div>
				{#if availableTags.length === 0}
					<div class="text-on-surface-variant opacity-40">No tags in vault</div>
				{:else}
					<div class="flex flex-wrap gap-1">
						{#each availableTags as tag (tag)}
							{@const active = filters.tags.includes(tag)}
							<button
								type="button"
								class="rounded-full border px-1.5 py-0.5 text-[10px] transition-opacity {active
									? 'border-primary/40 bg-primary/15 text-primary'
									: 'border-outline-variant/20 text-on-surface-variant opacity-60 hover:opacity-100'}"
								onclick={() => toggleFilterTag(tag)}
							>
								#{tag}
							</button>
						{/each}
					</div>
				{/if}
			</div>

			<!-- Folder prefix -->
			<div class="flex items-center gap-2">
				<span class="font-medium text-on-surface-variant opacity-70">Folder</span>
				<input
					type="text"
					placeholder="e.g. journal/"
					value={filters.folder}
					oninput={setFolderFilter}
					class="flex-1 rounded border border-outline-variant/20 bg-transparent px-1.5 py-0.5 text-[11px] text-on-surface placeholder:text-on-surface-variant placeholder:opacity-40 focus:border-primary/40 focus:outline-none"
				/>
			</div>

			<!-- Recency -->
			<div class="flex items-center gap-2">
				<span class="font-medium text-on-surface-variant opacity-70">Recency</span>
				<input
					type="range"
					min="0"
					max="180"
					step="1"
					value={filters.recencyDays}
					oninput={setRecencyDays}
					class="flex-1 accent-primary"
				/>
				<span class="w-14 text-right text-[10px] text-on-surface-variant opacity-60">
					{filters.recencyDays === 0 ? 'all time' : `≤ ${filters.recencyDays}d`}
				</span>
			</div>

			{#if activeFilterCount > 0}
				<div class="flex justify-end">
					<button
						type="button"
						class="text-[10px] text-on-surface-variant opacity-60 hover:opacity-100"
						onclick={clearFilters}
					>
						Clear all filters
					</button>
				</div>
			{/if}
		</div>
	{/if}

	<!-- Canvas -->
	<div class="relative flex-1 overflow-hidden" bind:this={containerEl}>
		{#if !activeFilePath || !activeFilePath.endsWith('.md')}
			<div class="flex h-full items-center justify-center p-6">
				<div class="flex flex-col items-center gap-2 text-center">
					<Share2 size={20} strokeWidth={1.2} class="text-on-surface-variant opacity-30" />
					<p class="text-xs text-on-surface-variant opacity-30">Open a note to see its graph</p>
				</div>
			</div>
		{:else if loading && simNodes.length === 0}
			<div class="flex h-full items-center justify-center p-6">
				<p class="text-xs text-on-surface-variant opacity-40">Building graph…</p>
			</div>
		{:else if error}
			<div class="flex h-full items-center justify-center p-6">
				<p class="text-xs text-red-400 opacity-70">{error}</p>
			</div>
		{:else if graph && graph.edges.length === 0 && graph.nodes.length <= 1}
			<div class="flex h-full items-center justify-center p-6">
				<div class="flex flex-col items-center gap-2 text-center">
					<Share2 size={20} strokeWidth={1.2} class="text-on-surface-variant opacity-20" />
					<p class="text-xs text-on-surface-variant opacity-30">No links from this note</p>
					<p class="text-xs text-on-surface-variant opacity-20">Add a [[wiki-link]] to see connections</p>
				</div>
			</div>
		{:else}
			<svg
				class="absolute inset-0 h-full w-full"
				viewBox="0 0 {width} {height}"
				onwheel={onWheel}
				role="presentation"
			>
				<!-- Background capture rect for panning -->
				<rect
					x="0"
					y="0"
					width={width}
					height={height}
					fill="transparent"
					role="presentation"
					style="cursor: {panning ? 'grabbing' : 'grab'};"
					onpointerdown={onBackgroundPointerDown}
					onpointermove={onBackgroundPointerMove}
					onpointerup={onBackgroundPointerUp}
					onpointercancel={onBackgroundPointerUp}
				/>

				<g
					transform="translate({viewTransform.x}, {viewTransform.y}) scale({viewTransform.k})"
				>
					<!-- Edges -->
					{#each simEdges as edge, i (i)}
						<line
							x1={edgeEndpoint(edge.source, 'x')}
							y1={edgeEndpoint(edge.source, 'y')}
							x2={edgeEndpoint(edge.target, 'x')}
							y2={edgeEndpoint(edge.target, 'y')}
							class="graph-edge"
							class:graph-edge--semantic={edge.kind === 'semantic'}
						/>
					{/each}
					<!-- Wider transparent hit-lines for semantic edges so they're hoverable / right-clickable -->
					{#each simEdges as edge, i (`hit-${i}`)}
						{#if edge.kind === 'semantic'}
							<line
								x1={edgeEndpoint(edge.source, 'x')}
								y1={edgeEndpoint(edge.source, 'y')}
								x2={edgeEndpoint(edge.target, 'x')}
								y2={edgeEndpoint(edge.target, 'y')}
								class="graph-edge-hit"
								onpointerenter={() => (hoveredEdge = edge)}
								onpointerleave={() => hoveredEdge === edge && (hoveredEdge = null)}
								oncontextmenu={(e) => onEdgeContextMenu(e, edge)}
								role="presentation"
							/>
						{/if}
					{/each}

					<!-- Nodes -->
					{#each simNodes as node (node.id)}
						{@const r = nodeRadius(node)}
						<g
							transform="translate({node.x ?? 0}, {node.y ?? 0})"
							class="graph-node"
							class:graph-node--center={node.isCenter}
							class:graph-node--semantic={node.isSemanticOnly}
							onclick={() => handleNodeClick(node)}
							onkeydown={(e) => {
								if (e.key === 'Enter' || e.key === ' ') {
									e.preventDefault();
									handleNodeClick(node);
								}
							}}
							onpointerenter={() => (hoveredNode = node)}
							onpointerleave={() => hoveredNode === node && (hoveredNode = null)}
							role="button"
							tabindex="0"
							aria-label={node.title}
						>
							<circle r={r} style="fill: {nodeColor(node)};" />
							<text
								y={r + 9}
								text-anchor="middle"
								class="graph-label"
								font-size={Math.max(7, 10 / viewTransform.k)}
							>
								{nodeLabel(node)}
							</text>
						</g>
					{/each}
				</g>
			</svg>

			{#if hoveredNode}
				{@const pos = tooltipPos(hoveredNode)}
				<div
					class="pointer-events-none absolute z-10 max-w-[220px] rounded border border-outline-variant/30 bg-surface-container px-2 py-1 text-xs shadow-lg"
					style="left: {pos.left}px; top: {pos.top}px;"
				>
					<div class="font-medium text-on-surface">{hoveredNode.title}</div>
					<div class="truncate text-on-surface-variant opacity-60">{hoveredNode.path}</div>
					{#if hoveredNode.tags.length > 0}
						<div class="mt-0.5 text-on-surface-variant opacity-50">
							#{hoveredNode.tags.join(' #')}
						</div>
					{/if}
				</div>
			{/if}

			{#if hoveredEdge && hoveredEdge.kind === 'semantic'}
				{@const epos = edgeTooltipPos(hoveredEdge)}
				<div
					class="pointer-events-none absolute z-10 rounded border border-outline-variant/30 bg-surface-container px-2 py-1 text-xs shadow-lg"
					style="left: {epos.left}px; top: {epos.top}px;"
				>
					<div class="text-on-surface">Similar content</div>
					{#if hoveredEdge.score !== undefined}
						<div class="text-on-surface-variant opacity-60">
							score {hoveredEdge.score.toFixed(2)}
						</div>
					{/if}
				</div>
			{/if}

			{#if showSemantic && simEdges.some((e) => e.kind === 'semantic')}
				<div class="pointer-events-none absolute bottom-2 right-2 z-10 flex flex-col gap-1 rounded border border-outline-variant/20 bg-surface-container/80 px-2 py-1 text-[10px] text-on-surface-variant opacity-70">
					<div class="flex items-center gap-1.5">
						<svg width="16" height="4"><line x1="0" y1="2" x2="16" y2="2" class="graph-edge" /></svg>
						<span>link</span>
					</div>
					<div class="flex items-center gap-1.5">
						<svg width="16" height="4"><line x1="0" y1="2" x2="16" y2="2" class="graph-edge graph-edge--semantic" /></svg>
						<span>similar</span>
					</div>
				</div>
			{/if}

			{#if edgeMenu}
				<!-- Invisible backdrop swallows the next click so the menu dismisses cleanly. -->
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<!-- svelte-ignore a11y_click_events_have_key_events -->
				<div
					class="absolute inset-0 z-20"
					onclick={closeEdgeMenu}
					oncontextmenu={(e) => {
						e.preventDefault();
						closeEdgeMenu();
					}}
				></div>
				<div
					class="absolute z-30 min-w-[140px] rounded border border-outline-variant/30 bg-surface-container py-1 text-xs shadow-lg"
					style="left: {edgeMenu.x}px; top: {edgeMenu.y}px;"
				>
					<button
						type="button"
						class="flex w-full items-center justify-between gap-2 px-2 py-1 text-left text-on-surface hover:bg-primary/10"
						onclick={() => linkTheseEdge(edgeMenu!.edge)}
					>
						<span>Link these</span>
						<span class="text-[10px] text-on-surface-variant opacity-50">[[…]]</span>
					</button>
				</div>
			{/if}

			{#if toast}
				<div class="pointer-events-none absolute bottom-2 left-1/2 z-40 -translate-x-1/2 rounded border border-outline-variant/30 bg-surface-container px-2 py-1 text-xs text-on-surface shadow-lg">
					{toast}
				</div>
			{/if}
		{/if}
	</div>
</div>

<style>
	:global(.graph-edge) {
		stroke: color-mix(in srgb, var(--color-outline-variant) 50%, transparent);
		stroke-width: 1;
		pointer-events: none;
	}

	:global(.graph-edge--semantic) {
		stroke: color-mix(in srgb, var(--color-primary) 40%, transparent);
		stroke-dasharray: 3 3;
	}

	:global(.graph-edge-hit) {
		stroke: transparent;
		stroke-width: 8;
		cursor: help;
	}

	:global(.graph-node) {
		cursor: pointer;
	}

	:global(.graph-node circle) {
		stroke: var(--color-surface-container-low);
		stroke-width: 1.5;
		transition: fill 150ms ease-out;
	}

	:global(.graph-node:hover circle) {
		fill: var(--color-primary) !important;
	}

	:global(.graph-node--center circle) {
		stroke: var(--color-surface-container-low);
		stroke-width: 2;
	}

	:global(.graph-node--semantic circle) {
		stroke: color-mix(in srgb, var(--color-primary) 60%, transparent);
		stroke-dasharray: 2 2;
		stroke-width: 1.5;
		fill-opacity: 0.4;
	}

	:global(.graph-label) {
		fill: var(--color-on-surface-variant);
		opacity: 0.7;
		pointer-events: none;
		user-select: none;
		font-family: var(--font-sans, system-ui);
	}

	:global(.graph-node:hover .graph-label) {
		fill: var(--color-on-surface);
		opacity: 1;
	}
</style>
