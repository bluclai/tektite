<script lang="ts">
	/**
	 * GraphView — whole-vault force-directed graph rendered on Canvas 2D.
	 *
	 * Phase 4 adds a settings drawer, per-vault persistence of positions /
	 * viewport / settings, live filters, and the Focus Active + Reset Layout
	 * viewport animations. The simulation forces are re-tuned from the
	 * drawer in realtime; K / min-similarity / filter changes re-issue the
	 * mutual-kNN query, superseding any in-flight request by `request_id`.
	 */
	import { invoke } from '@tauri-apps/api/core';
	import { listen, type UnlistenFn } from '@tauri-apps/api/event';
	import { onMount, untrack } from 'svelte';
	import { SlidersHorizontal } from 'lucide-svelte';
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
	import { quadtree, type Quadtree } from 'd3-quadtree';
	import { workspaceStore } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';
	import GraphSettingsDrawer, {
		type GraphSettings,
		type ColorMode,
	} from './GraphSettingsDrawer.svelte';
	import GraphFindOverlay from './GraphFindOverlay.svelte';
	import GraphNodeContextMenu from './GraphNodeContextMenu.svelte';
	import GraphEdgeContextMenu from './GraphEdgeContextMenu.svelte';

	interface Props {
		/** True iff this graph tab is the visible tab of its pane. Drives the RAF pause. */
		paneVisible: boolean;
	}

	let { paneVisible }: Props = $props();

	// ---------------------------------------------------------------------------
	// Wire format
	// ---------------------------------------------------------------------------

	interface GraphNodeDTO {
		id: string;
		path: string;
		title: string;
		tags: string[];
		modified: number;
		link_count: number;
		has_embedding?: boolean;
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

	interface GraphKnnResponse {
		edges: GraphEdgeDTO[];
	}

	interface GraphFiltersPayload {
		tags: string[] | null;
		folder: string | null;
		modified_after: number | null;
	}

	// ---------------------------------------------------------------------------
	// Simulation types
	// ---------------------------------------------------------------------------

	interface SimNode extends SimulationNodeDatum {
		id: string;
		path: string;
		title: string;
		tags: string[];
		wikiDegree: number;
		semDegree: number;
	}

	interface SimEdge extends SimulationLinkDatum<SimNode> {
		kind: string;
	}

	/** Semantic edges are render-only — they do not participate in the force sim. */
	interface SemEdge {
		source: SimNode;
		target: SimNode;
		score: number;
	}

	// ---------------------------------------------------------------------------
	// Persistence shape
	// ---------------------------------------------------------------------------

	interface PersistedGraphState {
		version: number;
		positions: Record<string, [number, number]>;
		viewport: { x: number; y: number; k: number };
		settings: GraphSettings;
		drawerOpen: boolean;
		openSections: string[];
	}

	const STATE_VERSION = 1;

	const DEFAULT_SETTINGS: GraphSettings = {
		chargeStrength: -180,
		linkDistance: 60,
		centerStrength: 0.05,
		k: 4,
		minSimilarity: 0.55,
		showSemanticEdges: true,
		colorBy: 'tag',
		showOrphans: true,
		labelsAtFitAll: false,
		reduceMotion: 'auto',
		tags: [],
		folder: '',
		recencyDays: 0,
		performance: 'auto',
	};

	// ---------------------------------------------------------------------------
	// DOM + reactive state
	// ---------------------------------------------------------------------------

	let containerEl = $state<HTMLDivElement | null>(null);
	let canvasEl = $state<HTMLCanvasElement | null>(null);

	let width = $state(800);
	let height = $state(600);
	let dpr = $state(1);

	let loading = $state(true);
	let errorMsg = $state<string | null>(null);

	// `$state.raw` — reassignment triggers reactivity (template reads
	// `simNodes.length`), but the inner SimNode objects are not deep-proxied,
	// so d3-force's per-tick x/y mutation stays fast.
	let simNodes = $state.raw<SimNode[]>([]);
	let simEdges: SimEdge[] = []; // wiki links — fed to forceLink
	let semanticEdges: SemEdge[] = []; // render-only dashed layer
	let simulation: Simulation<SimNode, SimEdge> | null = null;
	let qt: Quadtree<SimNode> | null = null;

	let nodesById: Map<string, SimNode> = new Map();

	// Viewport transform (screen = world * k + translate)
	let tx = 0;
	let ty = 0;
	let zk = 1;

	let hoveredId = $state<string | null>(null);
	const activeFilePath = $derived(workspaceStore.activeFilePath);

	let rafHandle: number | null = null;
	let documentVisible = $state(true);

	// ---------------------------------------------------------------------------
	// Settings + drawer
	// ---------------------------------------------------------------------------

	let settings = $state<GraphSettings>({ ...DEFAULT_SETTINGS });
	let availableTags = $state<string[]>([]);
	let drawerOpen = $state(false);
	let openSections = $state<string[]>(['forces', 'edges', 'display']);

	// Snapshot of filters used for the most recent wiki + knn fetches — used
	// to skip redundant refetches when `settings` updates don't touch filters.
	let filtersFetchedFor: string = '';

	// ---------------------------------------------------------------------------
	// Semantic-edge / kNN state
	// ---------------------------------------------------------------------------

	let knnInProgress = $state(false);
	let activeRequestId: string | null = null;
	let semanticFadeStart: number | null = null;
	let lastKnnAt = 0;
	let lastKnnNoteCount = 0;

	// ---------------------------------------------------------------------------
	// Viewport animation (Focus Active / Reset view)
	// ---------------------------------------------------------------------------

	let viewportAnim:
		| null
		| {
				startTx: number;
				startTy: number;
				startZk: number;
				endTx: number;
				endTy: number;
				endZk: number;
				startTime: number;
				duration: number;
		  } = null;

	// ---------------------------------------------------------------------------
	// Persistence
	// ---------------------------------------------------------------------------

	let persistLoaded = false; // becomes true after a successful load (may have null)
	let saveTimer: ReturnType<typeof setInterval> | null = null;

	// ---------------------------------------------------------------------------
	// Find overlay
	// ---------------------------------------------------------------------------

	let findOpen = $state(false);
	let findQuery = $state('');
	let matchIndex = $state(0);

	interface ParsedFind {
		text: string;
		tags: string[];
		pathPrefixes: string[];
		empty: boolean;
	}

	function parseFindQuery(q: string): ParsedFind {
		const tags: string[] = [];
		const pathPrefixes: string[] = [];
		const leftover: string[] = [];
		for (const tok of q.trim().split(/\s+/)) {
			if (!tok) continue;
			if (tok.startsWith('tag:')) tags.push(tok.slice(4).toLowerCase());
			else if (tok.startsWith('path:')) pathPrefixes.push(tok.slice(5).toLowerCase());
			else leftover.push(tok);
		}
		const text = leftover.join(' ').toLowerCase();
		return {
			text,
			tags,
			pathPrefixes,
			empty: text === '' && tags.length === 0 && pathPrefixes.length === 0,
		};
	}

	function matchesQuery(n: SimNode, q: ParsedFind): boolean {
		if (q.tags.length) {
			const lower = n.tags.map((t) => t.toLowerCase());
			if (!q.tags.every((t) => lower.includes(t))) return false;
		}
		if (q.pathPrefixes.length) {
			const p = n.path.toLowerCase();
			if (!q.pathPrefixes.every((pp) => p.includes(pp))) return false;
		}
		if (q.text) {
			const hay = `${n.title} ${n.path}`.toLowerCase();
			if (!hay.includes(q.text)) return false;
		}
		return true;
	}

	const parsedFind = $derived(parseFindQuery(findQuery));
	const matchedIds = $derived.by<Set<string> | null>(() => {
		if (!findOpen || parsedFind.empty) return null;
		const s = new Set<string>();
		for (const n of simNodes) if (matchesQuery(n, parsedFind)) s.add(n.id);
		return s;
	});
	const matchedNodes = $derived.by<SimNode[]>(() => {
		if (!matchedIds) return [];
		return simNodes.filter((n) => matchedIds.has(n.id));
	});

	// ---------------------------------------------------------------------------
	// Soft degradation tiers
	// ---------------------------------------------------------------------------

	/** 0 = full quality, 1 = collision off, 2 = settle-then-freeze. */
	function degradationTier(): 0 | 1 | 2 {
		if (settings.performance === 'high') return 0;
		if (settings.performance === 'low') return 2;
		const n = simNodes.length;
		let tier: 0 | 1 | 2 = n <= 1500 ? 0 : n <= 5000 ? 1 : 2;
		if (
			typeof navigator !== 'undefined' &&
			(navigator.hardwareConcurrency ?? 8) < 4 &&
			tier < 2
		) {
			tier = (tier + 1) as 0 | 1 | 2;
		}
		return tier;
	}

	function driftAlphaTarget(): number {
		if (reducedMotionActive()) return 0;
		return degradationTier() === 2 ? 0 : 0.005;
	}

	// ---------------------------------------------------------------------------
	// Simulation pause (Space key)
	// ---------------------------------------------------------------------------

	let simPaused = $state(false);

	// ---------------------------------------------------------------------------
	// Context menus + Link-to input
	// ---------------------------------------------------------------------------

	let nodeMenu = $state<{ x: number; y: number; node: SimNode } | null>(null);
	let edgeMenu = $state<{ x: number; y: number; edge: SemEdge } | null>(null);
	let linkToFor = $state<SimNode | null>(null);
	let linkToQuery = $state('');
	let linkToToast = $state<string | null>(null);

	// ---------------------------------------------------------------------------
	// Derived helpers
	// ---------------------------------------------------------------------------

	function vaultRelative(path: string): string {
		const vaultRoot = vaultStore.path;
		if (vaultRoot && path.startsWith(vaultRoot + '/')) {
			return path.slice(vaultRoot.length + 1);
		}
		return path;
	}

	function totalDegree(n: SimNode): number {
		return n.wikiDegree + n.semDegree;
	}

	function isOrphan(n: SimNode): boolean {
		return totalDegree(n) === 0;
	}

	function nodeRadius(n: SimNode): number {
		if (isOrphan(n)) return 2.5;
		return Math.max(3, Math.min(12, 3 + Math.sqrt(totalDegree(n)) * 1.2));
	}

	function nodeLabel(n: SimNode): string {
		const l = n.title || n.path.split('/').pop() || n.path;
		return l.length > 32 ? l.slice(0, 31) + '…' : l;
	}

	function folderOf(p: string): string {
		const i = p.lastIndexOf('/');
		return i === -1 ? '' : p.slice(0, i);
	}

	function hashHue(s: string): number {
		let h = 0x811c9dc5;
		for (let i = 0; i < s.length; i++) {
			h ^= s.charCodeAt(i);
			h = Math.imul(h, 0x01000193);
		}
		return (h >>> 0) % 360;
	}

	function primaryTagOf(n: SimNode): string | null {
		if (!n.tags || n.tags.length === 0) return null;
		let best = n.tags[0];
		for (let i = 1; i < n.tags.length; i++) {
			if (n.tags[i] < best) best = n.tags[i];
		}
		return best;
	}

	function nodeColor(n: SimNode, mode: ColorMode): string {
		if (mode === 'single') return 'hsl(230, 30%, 62%)';
		if (mode === 'folder') {
			const folder = folderOf(n.path);
			if (!folder) return 'hsl(220, 8%, 55%)';
			return `hsl(${hashHue(folder)}, 45%, 62%)`;
		}
		const tag = primaryTagOf(n);
		if (!tag) return 'hsl(220, 8%, 55%)';
		return `hsl(${hashHue(tag)}, 55%, 62%)`;
	}

	function semanticOpacity(score: number): number {
		const a = 0.35 + 0.5 * ((score - 0.55) / 0.45);
		return Math.max(0.1, Math.min(0.85, a));
	}

	function buildFilterPayload(s: GraphSettings): GraphFiltersPayload {
		const hasTag = s.tags.length > 0;
		const hasFolder = s.folder.trim().length > 0;
		const hasRecency = s.recencyDays > 0;
		const modifiedAfter = hasRecency
			? Math.floor(Date.now() / 1000) - s.recencyDays * 86400
			: null;
		return {
			tags: hasTag ? s.tags : null,
			folder: hasFolder ? s.folder.trim() : null,
			modified_after: modifiedAfter,
		};
	}

	function filterFingerprint(s: GraphSettings): string {
		const tags = [...s.tags].sort().join(',');
		return `${tags}|${s.folder.trim()}|${s.recencyDays}`;
	}

	function reducedMotionActive(): boolean {
		if (settings.reduceMotion === 'on') return true;
		if (settings.reduceMotion === 'off') return false;
		// auto: consult the media query
		if (typeof window === 'undefined') return false;
		return window.matchMedia('(prefers-reduced-motion: reduce)').matches;
	}

	// ---------------------------------------------------------------------------
	// Fetch + simulation setup
	// ---------------------------------------------------------------------------

	async function loadPersistedState(): Promise<PersistedGraphState | null> {
		try {
			const raw = await invoke<PersistedGraphState | null>('graph_state_load');
			if (!raw || typeof raw !== 'object') return null;
			if ((raw as PersistedGraphState).version !== STATE_VERSION) return null;
			return raw as PersistedGraphState;
		} catch {
			return null;
		}
	}

	function applyPersistedState(p: PersistedGraphState) {
		settings = { ...DEFAULT_SETTINGS, ...p.settings };
		if (p.viewport) {
			tx = p.viewport.x;
			ty = p.viewport.y;
			zk = p.viewport.k;
		}
		drawerOpen = !!p.drawerOpen;
		if (Array.isArray(p.openSections)) openSections = p.openSections;
	}

	async function fetchAndBuild(persisted: PersistedGraphState | null) {
		loading = true;
		errorMsg = null;
		try {
			const filters = buildFilterPayload(settings);
			filtersFetchedFor = filterFingerprint(settings);
			const data = await invoke<GraphDataDTO>('graph_get_full_vault', { filters });
			buildSimulation(data, persisted?.positions ?? null);
			void fetchSemanticEdges();
		} catch (e) {
			errorMsg = String(e);
			simNodes = [];
			simEdges = [];
			semanticEdges = [];
		} finally {
			loading = false;
		}
	}

	function buildSimulation(
		data: GraphDataDTO,
		positions: Record<string, [number, number]> | null,
	) {
		simulation?.stop();

		const cx = width / 2;
		const cy = height / 2;

		const wikiDegree = new Map<string, number>();
		for (const e of data.edges) {
			wikiDegree.set(e.source, (wikiDegree.get(e.source) ?? 0) + 1);
			wikiDegree.set(e.target, (wikiDegree.get(e.target) ?? 0) + 1);
		}

		const activeRel = untrack(() => activeFilePath);
		const hasPositions = !!positions && Object.keys(positions).length > 0;

		const nodes: SimNode[] = data.nodes.map((n) => {
			const rel = vaultRelative(n.path);
			const saved = positions?.[rel];
			let x: number;
			let y: number;
			if (saved) {
				[x, y] = saved;
			} else {
				// New note since last save (or fresh open): jitter near centroid.
				x = cx + (Math.random() - 0.5) * 40;
				y = cy + (Math.random() - 0.5) * 40;
			}
			return {
				id: n.id,
				path: n.path,
				title: n.title,
				tags: n.tags,
				wikiDegree: wikiDegree.get(n.id) ?? 0,
				semDegree: 0,
				x,
				y,
			};
		});

		if (!hasPositions && activeRel) {
			const center = nodes.find((n) => vaultRelative(n.path) === activeRel);
			if (center) {
				center.x = cx;
				center.y = cy;
			}
		}

		const byId = new Map(nodes.map((n) => [n.id, n]));
		const edges: SimEdge[] = [];
		for (const e of data.edges) {
			const s = byId.get(e.source);
			const t = byId.get(e.target);
			if (!s || !t) continue;
			edges.push({ source: s, target: t, kind: e.kind });
		}

		// Preliminary tier based on node count (can't call degradationTier() yet —
		// simNodes is set below). Inline the node-count branch here.
		const n = nodes.length;
		const highPerf = settings.performance === 'high';
		const lowPerf = settings.performance === 'low';
		let tier: 0 | 1 | 2 = highPerf
			? 0
			: lowPerf
				? 2
				: n <= 1500
					? 0
					: n <= 5000
						? 1
						: 2;
		if (
			!highPerf &&
			typeof navigator !== 'undefined' &&
			(navigator.hardwareConcurrency ?? 8) < 4 &&
			tier < 2
		) {
			tier = (tier + 1) as 0 | 1 | 2;
		}

		const alphaDecay = tier === 0 ? 0.05 : tier === 1 ? 0.04 : 0.04;

		const sim = forceSimulation<SimNode, SimEdge>(nodes)
			.force(
				'link',
				forceLink<SimNode, SimEdge>(edges)
					.id((d) => d.id)
					.distance(settings.linkDistance)
					.strength(0.7),
			)
			.force('charge', forceManyBody<SimNode>().strength(settings.chargeStrength))
			.force('center', forceCenter(cx, cy).strength(settings.centerStrength))
			.alphaDecay(alphaDecay);
		if (tier === 0) {
			sim.force('collide', forceCollide<SimNode>().radius((d) => nodeRadius(d) + 4));
		}

		const driftTarget = reducedMotionActive() || tier === 2 ? 0 : 0.005;

		sim.stop();
		if (!hasPositions) {
			// Fresh open — pre-tick to reach a reasonable layout before first paint.
			for (let i = 0; i < 30; i++) sim.tick();
			sim.alphaTarget(driftTarget).alpha(0.4).restart();
		} else {
			// Persisted layout — skip pre-ticks / fade-in, resume drift only.
			sim.alphaTarget(driftTarget).alpha(0).restart();
		}

		simulation = sim;
		simNodes = nodes;
		simEdges = edges;
		semanticEdges = [];
		nodesById = byId;
		rebuildQuadtree();
	}

	function rebuildQuadtree() {
		qt = quadtree<SimNode>()
			.x((d) => d.x ?? 0)
			.y((d) => d.y ?? 0)
			.addAll(simNodes);
	}

	// ---------------------------------------------------------------------------
	// Semantic edges
	// ---------------------------------------------------------------------------

	function newRequestId(): string {
		if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
			return crypto.randomUUID();
		}
		return `knn-${Date.now()}-${Math.random().toString(36).slice(2)}`;
	}

	async function fetchSemanticEdges() {
		if (activeRequestId !== null) {
			void invoke('graph_cancel_knn', { requestId: activeRequestId }).catch(() => {});
		}
		const requestId = newRequestId();
		activeRequestId = requestId;
		knnInProgress = true;
		try {
			const res = await invoke<GraphKnnResponse>('graph_get_mutual_knn', {
				k: settings.k,
				minSimilarity: settings.minSimilarity,
				filters: buildFilterPayload(settings),
				requestId,
			});
			if (activeRequestId !== requestId) return;
			applySemanticEdges(res.edges);
			lastKnnAt = performance.now();
			lastKnnNoteCount = simNodes.length;
		} catch (e) {
			if (activeRequestId === requestId) {
				console.warn('graph_get_mutual_knn failed:', e);
			}
		} finally {
			if (activeRequestId === requestId) {
				activeRequestId = null;
				knnInProgress = false;
			}
		}
	}

	function applySemanticEdges(edges: GraphEdgeDTO[]) {
		for (const n of simNodes) n.semDegree = 0;

		const resolved: SemEdge[] = [];
		for (const e of edges) {
			const s = nodesById.get(e.source);
			const t = nodesById.get(e.target);
			if (!s || !t) continue;
			resolved.push({ source: s, target: t, score: e.score ?? 0 });
			s.semDegree += 1;
			t.semDegree += 1;
		}
		semanticEdges = resolved;
		semanticFadeStart = performance.now();
		if (!reducedMotionActive()) {
			simulation?.alpha(0.15).restart();
		}
		ensureRaf();
	}

	// ---------------------------------------------------------------------------
	// RAF draw loop
	// ---------------------------------------------------------------------------

	function shouldAnimate(): boolean {
		return documentVisible && paneVisible && !simPaused;
	}

	function tickAndDraw() {
		rafHandle = null;
		if (!canvasEl) return;

		// Viewport animation runs regardless of reduce-motion? Plan says "Focus
		// active button animates viewport over ~300ms". Reduce-motion = On →
		// use instant camera jumps instead. The animation start path decides.
		if (viewportAnim) {
			const now = performance.now();
			const t = Math.min(1, (now - viewportAnim.startTime) / viewportAnim.duration);
			const eased = easeOutCubic(t);
			tx = viewportAnim.startTx + (viewportAnim.endTx - viewportAnim.startTx) * eased;
			ty = viewportAnim.startTy + (viewportAnim.endTy - viewportAnim.startTy) * eased;
			zk = viewportAnim.startZk + (viewportAnim.endZk - viewportAnim.startZk) * eased;
			if (t >= 1) viewportAnim = null;
		}

		const sim = simulation;
		if (sim && shouldAnimate()) {
			sim.tick();
			rebuildQuadtree();
		}
		draw();

		if (shouldAnimate() || viewportAnim) {
			rafHandle = requestAnimationFrame(tickAndDraw);
		}
	}

	function ensureRaf() {
		if (rafHandle !== null) return;
		if (!shouldAnimate() && !viewportAnim) return;
		rafHandle = requestAnimationFrame(tickAndDraw);
	}

	function easeOutCubic(t: number): number {
		return 1 - Math.pow(1 - t, 3);
	}

	function semanticFadeFactor(): number {
		if (semanticFadeStart === null) return 1;
		const elapsed = performance.now() - semanticFadeStart;
		if (elapsed >= 300) {
			semanticFadeStart = null;
			return 1;
		}
		return elapsed / 300;
	}

	function draw() {
		const canvas = canvasEl;
		if (!canvas) return;
		const ctx = canvas.getContext('2d');
		if (!ctx) return;

		ctx.save();
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		ctx.clearRect(0, 0, width, height);

		ctx.translate(tx, ty);
		ctx.scale(zk, zk);

		const matches = matchedIds;

		// Wiki edges
		ctx.lineWidth = 1 / zk;
		ctx.strokeStyle = 'rgba(128,140,170,0.35)';
		ctx.setLineDash([]);
		ctx.beginPath();
		for (const e of simEdges) {
			const s = typeof e.source === 'object' ? (e.source as SimNode) : null;
			const t = typeof e.target === 'object' ? (e.target as SimNode) : null;
			if (!s || !t) continue;
			if (matches && !matches.has(s.id) && !matches.has(t.id)) continue;
			ctx.moveTo(s.x ?? 0, s.y ?? 0);
			ctx.lineTo(t.x ?? 0, t.y ?? 0);
		}
		ctx.stroke();

		// Semantic edges (gated by `showSemanticEdges`)
		if (settings.showSemanticEdges && semanticEdges.length > 0) {
			const fade = semanticFadeFactor();
			ctx.setLineDash([3 / zk, 3 / zk]);
			ctx.lineWidth = 1 / zk;
			for (const e of semanticEdges) {
				if (matches && !matches.has(e.source.id) && !matches.has(e.target.id)) continue;
				const alpha = semanticOpacity(e.score) * fade;
				ctx.strokeStyle = `rgba(189,194,255,${alpha.toFixed(3)})`;
				ctx.beginPath();
				ctx.moveTo(e.source.x ?? 0, e.source.y ?? 0);
				ctx.lineTo(e.target.x ?? 0, e.target.y ?? 0);
				ctx.stroke();
			}
			ctx.setLineDash([]);
		}

		// Hover arrowheads on wiki edges
		if (hoveredId !== null) {
			ctx.strokeStyle = 'rgba(189,194,255,0.9)';
			ctx.fillStyle = 'rgba(189,194,255,0.9)';
			ctx.lineWidth = 1.25 / zk;
			for (const e of simEdges) {
				const s = typeof e.source === 'object' ? (e.source as SimNode) : null;
				const t = typeof e.target === 'object' ? (e.target as SimNode) : null;
				if (!s || !t) continue;
				if (s.id !== hoveredId && t.id !== hoveredId) continue;
				drawArrowhead(ctx, s, t);
			}
		}

		// Nodes
		const activeRel = activeFilePath;
		for (const n of simNodes) {
			const orphan = isOrphan(n);
			if (orphan && !settings.showOrphans) continue;
			const r = nodeRadius(n);
			const isActive = activeRel !== null && vaultRelative(n.path) === activeRel;
			const isHovered = hoveredId === n.id;
			const matched = matches ? matches.has(n.id) : true;

			if (isActive && matched) {
				ctx.beginPath();
				ctx.arc(n.x ?? 0, n.y ?? 0, r + 3 / zk, 0, Math.PI * 2);
				ctx.strokeStyle = 'rgba(189,194,255,0.9)';
				ctx.lineWidth = 1.5 / zk;
				ctx.stroke();
			}
			if (matches && matched) {
				// Accent glow stroke on find-matches.
				ctx.beginPath();
				ctx.arc(n.x ?? 0, n.y ?? 0, r + 2 / zk, 0, Math.PI * 2);
				ctx.strokeStyle = 'rgba(189,194,255,0.8)';
				ctx.lineWidth = 1.25 / zk;
				ctx.stroke();
			}

			ctx.beginPath();
			ctx.arc(n.x ?? 0, n.y ?? 0, r, 0, Math.PI * 2);
			if (isActive) {
				ctx.fillStyle = 'rgb(189,194,255)';
				ctx.globalAlpha = matched ? 1 : 0.2;
			} else if (isHovered) {
				ctx.fillStyle = 'hsl(230, 60%, 75%)';
				ctx.globalAlpha = matched ? 1 : 0.2;
			} else {
				ctx.fillStyle = nodeColor(n, settings.colorBy);
				const baseAlpha = orphan ? 0.35 : 1;
				ctx.globalAlpha = matched ? baseAlpha : 0.2;
			}
			ctx.fill();
			ctx.globalAlpha = 1;
		}

		// Labels
		ctx.font = `${12 / zk}px system-ui, sans-serif`;
		ctx.textAlign = 'center';
		ctx.textBaseline = 'top';
		ctx.fillStyle = 'rgba(220,224,235,0.85)';
		// "Labels at fit-all" override forces labels on even at k < 1.0.
		const labelAll = zk >= 1.6 || settings.labelsAtFitAll;
		const labelHubs = zk >= 0.9;
		for (const n of simNodes) {
			const orphan = isOrphan(n);
			if (orphan && !settings.showOrphans) continue;
			const isActive = activeRel !== null && vaultRelative(n.path) === activeRel;
			const isHovered = hoveredId === n.id;
			if (orphan && !isActive && !isHovered) continue;
			const eligible =
				isActive || isHovered || labelAll || (labelHubs && totalDegree(n) >= 4);
			if (!eligible) continue;
			const r = nodeRadius(n);
			ctx.fillText(nodeLabel(n), n.x ?? 0, (n.y ?? 0) + r + 3 / zk);
		}

		ctx.restore();
	}

	function drawArrowhead(
		ctx: CanvasRenderingContext2D,
		source: SimNode,
		target: SimNode,
	) {
		const sx = source.x ?? 0;
		const sy = source.y ?? 0;
		const tx0 = target.x ?? 0;
		const ty0 = target.y ?? 0;
		const dx = tx0 - sx;
		const dy = ty0 - sy;
		const len = Math.hypot(dx, dy);
		if (len < 1e-3) return;
		const ux = dx / len;
		const uy = dy / len;
		const tipR = nodeRadius(target) + 1 / zk;
		const tipX = tx0 - ux * tipR;
		const tipY = ty0 - uy * tipR;
		const size = 6 / zk;
		const px = -uy;
		const py = ux;
		const bx = tipX - ux * size;
		const by = tipY - uy * size;
		ctx.beginPath();
		ctx.moveTo(tipX, tipY);
		ctx.lineTo(bx + px * size * 0.6, by + py * size * 0.6);
		ctx.lineTo(bx - px * size * 0.6, by - py * size * 0.6);
		ctx.closePath();
		ctx.fill();
	}

	// ---------------------------------------------------------------------------
	// Pan / zoom / drag
	// ---------------------------------------------------------------------------

	function screenToWorld(sx: number, sy: number): [number, number] {
		return [(sx - tx) / zk, (sy - ty) / zk];
	}

	function onWheel(e: WheelEvent) {
		e.preventDefault();
		if (!canvasEl) return;
		const rect = canvasEl.getBoundingClientRect();
		const mx = e.clientX - rect.left;
		const my = e.clientY - rect.top;
		const factor = e.deltaY < 0 ? 1.1 : 1 / 1.1;
		const nextK = Math.min(4, Math.max(0.25, zk * factor));
		const scale = nextK / zk;
		tx = mx - (mx - tx) * scale;
		ty = my - (my - ty) * scale;
		zk = nextK;
		viewportAnim = null; // user override cancels in-flight animation
		ensureRaf();
	}

	let interaction:
		| { kind: 'none' }
		| { kind: 'pan'; startX: number; startY: number; startTx: number; startTy: number }
		| {
				kind: 'drag';
				node: SimNode;
				pointerId: number;
				movedPx: number;
				startX: number;
				startY: number;
		  } = { kind: 'none' };

	function pickNodeAt(sx: number, sy: number): SimNode | null {
		if (!qt) return null;
		const [wx, wy] = screenToWorld(sx, sy);
		const searchR = 14 / zk;
		const candidate = qt.find(wx, wy, searchR);
		if (!candidate) return null;
		const dx = (candidate.x ?? 0) - wx;
		const dy = (candidate.y ?? 0) - wy;
		const nr = nodeRadius(candidate) + 4 / zk;
		return dx * dx + dy * dy <= nr * nr ? candidate : null;
	}

	function onPointerDown(e: PointerEvent) {
		if (!canvasEl || e.button !== 0) return;
		viewportAnim = null;
		const rect = canvasEl.getBoundingClientRect();
		const sx = e.clientX - rect.left;
		const sy = e.clientY - rect.top;
		const hit = pickNodeAt(sx, sy);
		canvasEl.setPointerCapture(e.pointerId);
		if (hit) {
			const [wx, wy] = screenToWorld(sx, sy);
			hit.fx = wx;
			hit.fy = wy;
			simulation?.alphaTarget(0.3).restart();
			interaction = {
				kind: 'drag',
				node: hit,
				pointerId: e.pointerId,
				movedPx: 0,
				startX: sx,
				startY: sy,
			};
		} else {
			interaction = {
				kind: 'pan',
				startX: e.clientX,
				startY: e.clientY,
				startTx: tx,
				startTy: ty,
			};
		}
		ensureRaf();
	}

	function onPointerMove(e: PointerEvent) {
		if (!canvasEl) return;
		const rect = canvasEl.getBoundingClientRect();
		const sx = e.clientX - rect.left;
		const sy = e.clientY - rect.top;

		if (interaction.kind === 'pan') {
			tx = interaction.startTx + (e.clientX - interaction.startX);
			ty = interaction.startTy + (e.clientY - interaction.startY);
			ensureRaf();
			return;
		}
		if (interaction.kind === 'drag') {
			const [wx, wy] = screenToWorld(sx, sy);
			interaction.node.fx = wx;
			interaction.node.fy = wy;
			interaction.movedPx += Math.hypot(sx - interaction.startX, sy - interaction.startY);
			interaction.startX = sx;
			interaction.startY = sy;
			ensureRaf();
			return;
		}

		const hit = pickNodeAt(sx, sy);
		const nextId = hit?.id ?? null;
		if (nextId !== hoveredId) {
			hoveredId = nextId;
			if (canvasEl) canvasEl.style.cursor = hit ? 'pointer' : 'default';
			ensureRaf();
		}
	}

	function onPointerUp(e: PointerEvent) {
		if (!canvasEl) return;
		if (canvasEl.hasPointerCapture(e.pointerId)) canvasEl.releasePointerCapture(e.pointerId);
		if (interaction.kind === 'drag') {
			const { node, movedPx } = interaction;
			node.fx = null;
			node.fy = null;
			simulation?.alphaTarget(driftAlphaTarget());
			if (movedPx < 4) {
				openNode(node, e.metaKey || e.ctrlKey);
			}
		} else if (interaction.kind === 'pan') {
			simulation?.alphaTarget(driftAlphaTarget());
		}
		interaction = { kind: 'none' };
	}

	function openNode(node: SimNode, forceNew: boolean) {
		const vaultRoot = vaultStore.path;
		const absPath =
			vaultRoot && !node.path.startsWith(vaultRoot) ? `${vaultRoot}/${node.path}` : node.path;
		workspaceStore.openTab(absPath, { forceNew });
	}

	// ---------------------------------------------------------------------------
	// Context menus + Link-to
	// ---------------------------------------------------------------------------

	function pickSemanticEdgeAt(sx: number, sy: number): SemEdge | null {
		if (!settings.showSemanticEdges || semanticEdges.length === 0) return null;
		const [wx, wy] = screenToWorld(sx, sy);
		const tol = 6 / zk;
		let best: { edge: SemEdge; dist: number } | null = null;
		for (const e of semanticEdges) {
			const ax = e.source.x ?? 0;
			const ay = e.source.y ?? 0;
			const bx = e.target.x ?? 0;
			const by = e.target.y ?? 0;
			const dx = bx - ax;
			const dy = by - ay;
			const len2 = dx * dx + dy * dy;
			if (len2 < 1e-6) continue;
			const t = Math.max(0, Math.min(1, ((wx - ax) * dx + (wy - ay) * dy) / len2));
			const px = ax + t * dx;
			const py = ay + t * dy;
			const d = Math.hypot(wx - px, wy - py);
			if (d <= tol && (!best || d < best.dist)) best = { edge: e, dist: d };
		}
		return best?.edge ?? null;
	}

	function onContextMenu(e: MouseEvent) {
		if (!canvasEl) return;
		const rect = canvasEl.getBoundingClientRect();
		const sx = e.clientX - rect.left;
		const sy = e.clientY - rect.top;
		const hitNode = pickNodeAt(sx, sy);
		if (hitNode) {
			e.preventDefault();
			nodeMenu = { x: sx, y: sy, node: hitNode };
			edgeMenu = null;
			return;
		}
		const hitEdge = pickSemanticEdgeAt(sx, sy);
		if (hitEdge) {
			e.preventDefault();
			edgeMenu = { x: sx, y: sy, edge: hitEdge };
			nodeMenu = null;
		}
	}

	function closeMenus() {
		nodeMenu = null;
		edgeMenu = null;
	}

	function revealInExplorer() {
		workspaceStore.setActivePanel('files');
		if (!workspaceStore.sidebarOpen) workspaceStore.openSidebar();
	}

	async function copyNodePath(n: SimNode) {
		const rel = vaultRelative(n.path);
		try {
			await navigator.clipboard.writeText(rel);
			showLinkToast(`Copied ${rel}`);
		} catch {
			showLinkToast('Copy failed');
		}
	}

	function focusOnNode(n: SimNode) {
		const nextK = Math.max(zk, 1.5);
		const cx = width / 2;
		const cy = height / 2;
		animateTo(cx - (n.x ?? 0) * nextK, cy - (n.y ?? 0) * nextK, nextK);
	}

	function openLinkTo(n: SimNode) {
		linkToFor = n;
		linkToQuery = '';
	}

	function closeLinkTo() {
		linkToFor = null;
		linkToQuery = '';
	}

	const linkToMatches = $derived.by<SimNode[]>(() => {
		if (!linkToFor) return [];
		const q = linkToQuery.trim().toLowerCase();
		if (!q) return [];
		const src = linkToFor;
		return simNodes
			.filter((n) => {
				if (n.id === src.id) return false;
				const hay = `${n.title} ${n.path}`.toLowerCase();
				return hay.includes(q);
			})
			.slice(0, 8);
	});

	async function submitLinkTo(target: SimNode) {
		if (!linkToFor) return;
		const sourcePath = linkToFor.path;
		const targetPath = target.path;
		closeLinkTo();
		try {
			await invoke('graph_append_wiki_link', { sourcePath, targetPath });
			showLinkToast(`Linked → ${target.title || vaultRelative(targetPath)}`);
		} catch (err) {
			showLinkToast(`Link failed: ${String(err)}`);
		}
	}

	async function linkEdge(edge: SemEdge) {
		const sourcePath = edge.source.path;
		const targetPath = edge.target.path;
		try {
			await invoke('graph_append_wiki_link', { sourcePath, targetPath });
			showLinkToast(`Linked → ${edge.target.title || vaultRelative(targetPath)}`);
		} catch (err) {
			showLinkToast(`Link failed: ${String(err)}`);
		}
	}

	function autofocusAction(node: HTMLInputElement) {
		queueMicrotask(() => node.focus());
	}

	let toastTimer: ReturnType<typeof setTimeout> | null = null;
	function showLinkToast(msg: string) {
		linkToToast = msg;
		if (toastTimer) clearTimeout(toastTimer);
		toastTimer = setTimeout(() => {
			linkToToast = null;
			toastTimer = null;
		}, 2400);
	}

	// ---------------------------------------------------------------------------
	// Viewport actions
	// ---------------------------------------------------------------------------

	function animateTo(endTx: number, endTy: number, endZk: number, duration = 300) {
		if (reducedMotionActive()) {
			tx = endTx;
			ty = endTy;
			zk = endZk;
			ensureRaf();
			return;
		}
		viewportAnim = {
			startTx: tx,
			startTy: ty,
			startZk: zk,
			endTx,
			endTy,
			endZk,
			startTime: performance.now(),
			duration,
		};
		ensureRaf();
	}

	function focusActive() {
		const rel = activeFilePath;
		if (!rel) return;
		const target = simNodes.find((n) => vaultRelative(n.path) === rel);
		if (!target) return;
		const nextK = 1.5;
		const cx = width / 2;
		const cy = height / 2;
		// Solve: cx = target.x * nextK + endTx  →  endTx = cx - target.x * nextK
		const endTx = cx - (target.x ?? 0) * nextK;
		const endTy = cy - (target.y ?? 0) * nextK;
		animateTo(endTx, endTy, nextK);
	}

	function resetView() {
		animateTo(0, 0, 1);
	}

	async function resetLayout() {
		// Wipe persisted positions and re-run first-open seeding.
		try {
			await invoke('graph_state_save', {
				state: {
					version: STATE_VERSION,
					positions: {},
					viewport: { x: 0, y: 0, k: 1 },
					settings,
					drawerOpen,
					openSections,
				},
			});
		} catch {
			// Non-fatal — still re-seed in memory.
		}
		tx = 0;
		ty = 0;
		zk = 1;
		await fetchAndBuild(null);
	}

	// ---------------------------------------------------------------------------
	// Drawer / settings changes
	// ---------------------------------------------------------------------------

	function updateSettings(partial: Partial<GraphSettings>) {
		const prev = settings;
		settings = { ...prev, ...partial };

		// Live-update force parameters without rebuilding the sim.
		if (partial.chargeStrength !== undefined) {
			// d3-force: manyBody strength accepts a number or accessor.
			// eslint-disable-next-line @typescript-eslint/no-explicit-any
			const charge = simulation?.force('charge') as any;
			charge?.strength(partial.chargeStrength);
			simulation?.alpha(0.1).restart();
		}
		if (partial.linkDistance !== undefined) {
			// eslint-disable-next-line @typescript-eslint/no-explicit-any
			const link = simulation?.force('link') as any;
			link?.distance(partial.linkDistance);
			simulation?.alpha(0.1).restart();
		}
		if (partial.centerStrength !== undefined) {
			// eslint-disable-next-line @typescript-eslint/no-explicit-any
			const center = simulation?.force('center') as any;
			center?.strength(partial.centerStrength);
			simulation?.alpha(0.1).restart();
		}

		// K / min-sim changes → re-issue kNN (show-toggle alone doesn't refetch).
		if (partial.k !== undefined || partial.minSimilarity !== undefined) {
			void fetchSemanticEdges();
		}

		// Filter changes → refetch wiki graph AND semantic layer.
		const filterChanged =
			partial.tags !== undefined ||
			partial.folder !== undefined ||
			partial.recencyDays !== undefined;
		if (filterChanged) {
			const fp = filterFingerprint(settings);
			if (fp !== filtersFetchedFor) {
				void fetchAndBuild(null);
			}
		}

		// Reduce-motion flip: adjust alphaTarget on the fly.
		if (partial.reduceMotion !== undefined) {
			simulation?.alphaTarget(driftAlphaTarget());
			if (!reducedMotionActive() && !simPaused) simulation?.restart();
		}

		// Performance mode: tier change may toggle collide + alphaTarget. Full
		// rebuild via fetchAndBuild is overkill — just re-tune the sim.
		if (partial.performance !== undefined) {
			applyTierTuning();
		}

		ensureRaf();
		void scheduleSave(true);
	}

	function applyTierTuning() {
		if (!simulation) return;
		const tier = degradationTier();
		if (tier === 0) {
			if (!simulation.force('collide')) {
				simulation.force(
					'collide',
					forceCollide<SimNode>().radius((d) => nodeRadius(d) + 4),
				);
			}
		} else {
			simulation.force('collide', null);
		}
		simulation.alphaTarget(driftAlphaTarget());
		if (!simPaused && !reducedMotionActive() && tier < 2) simulation.restart();
	}

	// ---------------------------------------------------------------------------
	// Keyboard viewport helpers
	// ---------------------------------------------------------------------------

	function zoomAtCenter(factor: number) {
		const nextK = Math.min(4, Math.max(0.25, zk * factor));
		const cx = width / 2;
		const cy = height / 2;
		const scale = nextK / zk;
		tx = cx - (cx - tx) * scale;
		ty = cy - (cy - ty) * scale;
		zk = nextK;
		viewportAnim = null;
		ensureRaf();
	}

	function panBy(dxScreen: number, dyScreen: number) {
		tx += dxScreen;
		ty += dyScreen;
		viewportAnim = null;
		ensureRaf();
	}

	const arrowHoldStart: Record<string, number> = {};

	function panStepForKey(key: string): number {
		const start = arrowHoldStart[key];
		if (!start) return 20;
		const held = performance.now() - start;
		return held >= 300 ? 80 : 20;
	}

	function onGraphKeydown(e: KeyboardEvent) {
		// Don't hijack typing in inputs (the Find overlay has its own handler).
		const tgt = e.target as HTMLElement | null;
		if (tgt && (tgt.tagName === 'INPUT' || tgt.tagName === 'TEXTAREA' || tgt.isContentEditable)) {
			return;
		}

		if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'f') {
			e.preventDefault();
			openFind();
			return;
		}
		if (e.key === 'Escape') {
			if (findOpen) {
				e.preventDefault();
				closeFind();
			}
			return;
		}

		// Single-key shortcuts below: ignore when modifiers pressed.
		if (e.metaKey || e.ctrlKey || e.altKey) return;

		switch (e.key) {
			case 'ArrowLeft': {
				e.preventDefault();
				arrowHoldStart[e.key] ??= performance.now();
				panBy(panStepForKey(e.key), 0);
				break;
			}
			case 'ArrowRight': {
				e.preventDefault();
				arrowHoldStart[e.key] ??= performance.now();
				panBy(-panStepForKey(e.key), 0);
				break;
			}
			case 'ArrowUp': {
				e.preventDefault();
				arrowHoldStart[e.key] ??= performance.now();
				panBy(0, panStepForKey(e.key));
				break;
			}
			case 'ArrowDown': {
				e.preventDefault();
				arrowHoldStart[e.key] ??= performance.now();
				panBy(0, -panStepForKey(e.key));
				break;
			}
			case '+':
			case '=':
				e.preventDefault();
				zoomAtCenter(1.15);
				break;
			case '-':
			case '_':
				e.preventDefault();
				zoomAtCenter(1 / 1.15);
				break;
			case 'r':
			case 'R':
				e.preventDefault();
				resetView();
				break;
			case 'f':
			case 'F':
				e.preventDefault();
				focusActive();
				break;
			case ' ':
				e.preventDefault();
				toggleSimPause();
				break;
		}
	}

	function onGraphKeyup(e: KeyboardEvent) {
		if (e.key.startsWith('Arrow')) delete arrowHoldStart[e.key];
	}

	function toggleSection(id: string) {
		openSections = openSections.includes(id)
			? openSections.filter((s) => s !== id)
			: [...openSections, id];
		void scheduleSave(true);
	}

	function toggleDrawer() {
		drawerOpen = !drawerOpen;
		void scheduleSave(true);
	}

	function openFind() {
		findOpen = true;
	}

	function closeFind() {
		findOpen = false;
		findQuery = '';
		matchIndex = 0;
		ensureRaf();
	}

	function onFindQueryChange(q: string) {
		findQuery = q;
		matchIndex = 0;
		ensureRaf();
	}

	function fitToNodes(ns: SimNode[], padding = 48) {
		if (ns.length === 0) return;
		if (ns.length === 1) {
			const n = ns[0];
			const nextK = 2;
			const cx = width / 2;
			const cy = height / 2;
			animateTo(cx - (n.x ?? 0) * nextK, cy - (n.y ?? 0) * nextK, nextK);
			return;
		}
		let minX = Infinity;
		let minY = Infinity;
		let maxX = -Infinity;
		let maxY = -Infinity;
		for (const n of ns) {
			const x = n.x ?? 0;
			const y = n.y ?? 0;
			if (x < minX) minX = x;
			if (x > maxX) maxX = x;
			if (y < minY) minY = y;
			if (y > maxY) maxY = y;
		}
		const bw = Math.max(1, maxX - minX);
		const bh = Math.max(1, maxY - minY);
		const nextK = Math.min(
			4,
			Math.max(0.25, Math.min((width - padding * 2) / bw, (height - padding * 2) / bh)),
		);
		const cx = width / 2;
		const cy = height / 2;
		const midX = (minX + maxX) / 2;
		const midY = (minY + maxY) / 2;
		animateTo(cx - midX * nextK, cy - midY * nextK, nextK);
	}

	function onFindEnter() {
		const ns = matchedNodes;
		if (ns.length === 0) return;
		fitToNodes(ns);
	}

	function cycleMatch(delta: 1 | -1) {
		const ns = matchedNodes;
		if (ns.length === 0) return;
		matchIndex = (matchIndex + delta + ns.length) % ns.length;
		const n = ns[matchIndex];
		const nextK = Math.max(zk, 1.2);
		const cx = width / 2;
		const cy = height / 2;
		animateTo(cx - (n.x ?? 0) * nextK, cy - (n.y ?? 0) * nextK, nextK);
	}

	function resetSimPause() {
		simPaused = false;
	}

	function toggleSimPause() {
		simPaused = !simPaused;
		if (simPaused) {
			simulation?.alphaTarget(0).stop();
		} else {
			simulation?.alphaTarget(driftAlphaTarget()).restart();
			ensureRaf();
		}
	}

	// ---------------------------------------------------------------------------
	// Persistence — save
	// ---------------------------------------------------------------------------

	function snapshotState(): PersistedGraphState {
		const positions: Record<string, [number, number]> = {};
		for (const n of simNodes) {
			positions[vaultRelative(n.path)] = [n.x ?? 0, n.y ?? 0];
		}
		return {
			version: STATE_VERSION,
			positions,
			viewport: { x: tx, y: ty, k: zk },
			settings,
			drawerOpen,
			openSections,
		};
	}

	let savePending = false;
	async function scheduleSave(immediate = false) {
		if (!persistLoaded) return;
		if (savePending && !immediate) return;
		savePending = true;
		try {
			await invoke('graph_state_save', { state: snapshotState() });
		} catch {
			// Non-fatal — ignore.
		} finally {
			savePending = false;
		}
	}

	// ---------------------------------------------------------------------------
	// Sizing + lifecycle
	// ---------------------------------------------------------------------------

	function resizeCanvas() {
		if (!canvasEl) return;
		dpr = Math.max(1, window.devicePixelRatio || 1);
		canvasEl.width = Math.floor(width * dpr);
		canvasEl.height = Math.floor(height * dpr);
		canvasEl.style.width = `${width}px`;
		canvasEl.style.height = `${height}px`;
		ensureRaf();
	}

	onMount(() => {
		dpr = Math.max(1, window.devicePixelRatio || 1);
		documentVisible = document.visibilityState === 'visible';

		const ro = new ResizeObserver((entries) => {
			for (const entry of entries) {
				const rect = entry.contentRect;
				width = Math.max(120, rect.width);
				height = Math.max(120, rect.height);
				resizeCanvas();
			}
		});
		if (containerEl) ro.observe(containerEl);

		const onVisibilityChange = () => {
			documentVisible = document.visibilityState === 'visible';
			ensureRaf();
		};
		document.addEventListener('visibilitychange', onVisibilityChange);

		// Re-issue semantic edges when the index materially changes, but only
		// after a 30-second cooldown AND a ≥50-note delta from the last run.
		let unlistenStats: UnlistenFn | null = null;
		void listen<{ note_count: number }>('index:stats-changed', ({ payload }) => {
			const noteCount = payload?.note_count ?? simNodes.length;
			const now = performance.now();
			if (now - lastKnnAt < 30_000) return;
			if (Math.abs(noteCount - lastKnnNoteCount) < 50) return;
			void fetchSemanticEdges();
		}).then((fn) => {
			unlistenStats = fn;
		});

		// Boot sequence: (1) load persisted, (2) fetch tags for filter chips,
		// (3) fetch graph + seed from persisted positions.
		(async () => {
			const persisted = await loadPersistedState();
			if (persisted) applyPersistedState(persisted);
			persistLoaded = true;

			try {
				availableTags = await invoke<string[]>('index_list_all_tags');
			} catch {
				availableTags = [];
			}

			await fetchAndBuild(persisted);
		})();

		// Debounced 1s save loop while mounted.
		saveTimer = setInterval(() => {
			void scheduleSave();
		}, 1000);

		return () => {
			ro.disconnect();
			document.removeEventListener('visibilitychange', onVisibilityChange);
			if (unlistenStats) unlistenStats();
			if (saveTimer !== null) {
				clearInterval(saveTimer);
				saveTimer = null;
			}
			// Final save before teardown so we don't drop the last second of work.
			void scheduleSave(true);
			if (activeRequestId !== null) {
				void invoke('graph_cancel_knn', { requestId: activeRequestId }).catch(() => {});
				activeRequestId = null;
			}
			if (rafHandle !== null) cancelAnimationFrame(rafHandle);
			rafHandle = null;
			simulation?.stop();
			simulation = null;
		};
	});

	$effect(() => {
		void paneVisible;
		void documentVisible;
		ensureRaf();
	});

	$effect(() => {
		void activeFilePath;
		ensureRaf();
	});
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	bind:this={containerEl}
	class="relative flex h-full w-full flex-col overflow-hidden focus:outline-none"
	tabindex={-1}
	onkeydown={onGraphKeydown}
	onkeyup={onGraphKeyup}
	onpointerdowncapture={() => containerEl?.focus({ preventScroll: true })}
>
	<canvas
		bind:this={canvasEl}
		onwheel={onWheel}
		onpointerdown={onPointerDown}
		onpointermove={onPointerMove}
		onpointerup={onPointerUp}
		onpointercancel={onPointerUp}
		oncontextmenu={onContextMenu}
		class="block h-full w-full touch-none select-none"
		aria-label="Vault graph"
	></canvas>

	{#if nodeMenu}
		<GraphNodeContextMenu
			x={nodeMenu.x}
			y={nodeMenu.y}
			title={nodeMenu.node.title || vaultRelative(nodeMenu.node.path)}
			onOpenInNewTab={() => openNode(nodeMenu!.node, true)}
			onRevealInExplorer={revealInExplorer}
			onCopyPath={() => copyNodePath(nodeMenu!.node)}
			onLinkTo={() => openLinkTo(nodeMenu!.node)}
			onFocus={() => focusOnNode(nodeMenu!.node)}
			onClose={closeMenus}
		/>
	{/if}

	{#if edgeMenu}
		<GraphEdgeContextMenu
			x={edgeMenu.x}
			y={edgeMenu.y}
			sourceTitle={edgeMenu.edge.source.title || vaultRelative(edgeMenu.edge.source.path)}
			targetTitle={edgeMenu.edge.target.title || vaultRelative(edgeMenu.edge.target.path)}
			onLinkThese={() => linkEdge(edgeMenu!.edge)}
			onClose={closeMenus}
		/>
	{/if}

	{#if linkToFor}
		<div
			class="pointer-events-auto absolute left-1/2 top-3 z-30 w-[320px] -translate-x-1/2 rounded-md border border-outline-variant/20 bg-surface-container/95 p-2 text-[11px] text-on-surface shadow-xl backdrop-blur"
		>
			<div class="mb-1 truncate text-[10px] opacity-60">
				Link from {linkToFor.title || vaultRelative(linkToFor.path)} →
			</div>
			<input
				type="text"
				placeholder="Search target note…"
				value={linkToQuery}
				oninput={(e) => (linkToQuery = (e.target as HTMLInputElement).value)}
				onkeydown={(e) => {
					if (e.key === 'Escape') {
						e.preventDefault();
						closeLinkTo();
					} else if (e.key === 'Enter' && linkToMatches.length > 0) {
						e.preventDefault();
						void submitLinkTo(linkToMatches[0]);
					}
				}}
				class="w-full rounded border border-outline-variant/20 bg-transparent px-1.5 py-1 text-[11px] text-on-surface placeholder:opacity-40 focus:border-primary/40 focus:outline-none"
				aria-label="Target note"
				use:autofocusAction
			/>
			{#if linkToMatches.length > 0}
				<div class="mt-1 flex flex-col">
					{#each linkToMatches as m (m.id)}
						<button
							type="button"
							class="truncate rounded px-1.5 py-1 text-left hover:bg-surface-container-high"
							onclick={() => submitLinkTo(m)}
						>
							<span class="text-on-surface">{m.title || vaultRelative(m.path)}</span>
							<span class="ml-1 text-[10px] opacity-50">{vaultRelative(m.path)}</span>
						</button>
					{/each}
				</div>
			{/if}
		</div>
	{/if}

	{#if linkToToast}
		<div
			class="pointer-events-none absolute bottom-12 left-1/2 -translate-x-1/2 rounded-md bg-surface-container/95 px-3 py-1 text-[11px] text-on-surface shadow-lg"
		>
			{linkToToast}
		</div>
	{/if}

	<!-- Settings drawer trigger (top-right) -->
	<div class="absolute right-3 top-3 z-10 flex items-center gap-2">
		<button
			type="button"
			class="rounded-md border border-outline-variant/20 bg-surface-container/80 px-2 py-1 text-[11px] text-on-surface-variant opacity-70 transition-opacity hover:opacity-100"
			onclick={resetView}
			title="Reset view"
		>
			Reset
		</button>
		<button
			type="button"
			class="rounded-md border border-outline-variant/20 bg-surface-container/80 p-1.5 text-on-surface-variant opacity-70 transition-opacity hover:opacity-100"
			onclick={toggleDrawer}
			title="Graph settings"
			aria-expanded={drawerOpen}
		>
			<SlidersHorizontal size={14} />
		</button>
	</div>

	{#if findOpen}
		<GraphFindOverlay
			query={findQuery}
			matchCount={matchedNodes.length}
			{matchIndex}
			onQueryChange={onFindQueryChange}
			onEnter={onFindEnter}
			onNext={() => cycleMatch(1)}
			onPrev={() => cycleMatch(-1)}
			onClose={closeFind}
		/>
	{/if}

	{#if knnInProgress}
		<div
			class="pointer-events-none absolute bottom-3 left-3 rounded-md bg-surface-container/80 px-2 py-1 text-[11px] text-on-surface-variant opacity-80"
		>
			Computing similar notes…
		</div>
	{/if}

	{#if simPaused}
		<div
			class="pointer-events-none absolute bottom-3 right-3 rounded-md bg-surface-container/80 px-2 py-1 text-[10px] text-on-surface-variant opacity-80"
		>
			Paused — Space to resume
		</div>
	{/if}

	{#if drawerOpen}
		<GraphSettingsDrawer
			{settings}
			{availableTags}
			{openSections}
			onSettingsChange={updateSettings}
			onSectionToggle={toggleSection}
			onClose={toggleDrawer}
			onFocusActive={focusActive}
			onResetLayout={resetLayout}
			onFindNodes={openFind}
		/>
	{/if}

	{#if loading}
		<div class="pointer-events-none absolute inset-0 flex items-center justify-center">
			<p class="text-xs text-on-surface-variant opacity-50">Building graph…</p>
		</div>
	{:else if errorMsg}
		<div class="pointer-events-none absolute inset-0 flex items-center justify-center">
			<p class="text-xs text-red-400 opacity-70">{errorMsg}</p>
		</div>
	{:else if simNodes.length === 0}
		<div class="pointer-events-none absolute inset-0 flex items-center justify-center">
			<p class="text-xs text-on-surface-variant opacity-50">No notes in this vault yet.</p>
		</div>
	{/if}
</div>
