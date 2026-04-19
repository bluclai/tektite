<script lang="ts">
	/**
	 * RelatedNotesPanel — semantically related notes for the active file.
	 *
	 * Queries `search_related_notes` when the active tab changes, refetches
	 * on autosave (debounced) and when the embed backlog makes progress
	 * against an empty result set. Mirrors BacklinksPanel's shape for
	 * derivation, but rows open via the shared semantic navigation helper.
	 */
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { Link2 } from 'lucide-svelte';
	import { workspaceStore, allLeaves } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';
	import { embedStatusStore } from '$lib/stores/embedStatus.svelte';
	import { editorStore } from '$lib/stores/editor.svelte';
	import { openSemanticHit } from '$lib/semantic-navigation';

	interface SemanticHit {
		chunk_id: string;
		file_path: string;
		heading_path: string | null;
		heading_text: string | null;
		heading_level: number | null;
		snippet: string;
		score: number;
	}

	const SAVE_REFETCH_DEBOUNCE_MS = 800;
	const RELATED_LIMIT = 10;

	// ---------------------------------------------------------------------------
	// Active file derivation (vault-relative, matches BacklinksPanel)
	// ---------------------------------------------------------------------------

	let activeFilePath = $derived.by(() => {
		const leaves = allLeaves(workspaceStore.paneTree);
		const activeLeaf = leaves.find((l) => l.id === workspaceStore.activePaneId);
		if (!activeLeaf || !activeLeaf.activeTabId) return null;
		const tab = activeLeaf.tabs.find((t) => t.id === activeLeaf.activeTabId);
		if (!tab || tab.kind !== 'file') return null;

		const vaultRoot = vaultStore.path;
		if (vaultRoot && tab.path.startsWith(vaultRoot + '/')) {
			return tab.path.slice(vaultRoot.length + 1);
		}
		return tab.path;
	});

	// ---------------------------------------------------------------------------
	// Fetch state
	// ---------------------------------------------------------------------------

	let hits = $state<SemanticHit[]>([]);
	let loading = $state(false);
	let latestRequestId = 0;
	let lastFetchedPath: string | null = null;
	let lastSeenProgressDone = 0;

	function fetchRelated(filePath: string) {
		loading = true;
		const requestId = ++latestRequestId;

		invoke<SemanticHit[]>('search_related_notes', {
			filePath,
			limit: RELATED_LIMIT,
		})
			.then((rows) => {
				if (requestId !== latestRequestId) return;
				hits = rows;
			})
			.catch(() => {
				if (requestId !== latestRequestId) return;
				hits = [];
			})
			.finally(() => {
				if (requestId !== latestRequestId) return;
				loading = false;
				lastFetchedPath = filePath;
				lastSeenProgressDone = embedStatusStore.done;
			});
	}

	// Fetch on active-file change.
	$effect(() => {
		const filePath = activeFilePath;

		if (!filePath || !filePath.endsWith('.md')) {
			hits = [];
			loading = false;
			latestRequestId += 1;
			lastFetchedPath = null;
			return;
		}

		fetchRelated(filePath);
	});

	// Debounced refetch on autosave: watch editorStore's last-save timestamp,
	// refire when the active file is the one that just saved.
	let saveRefetchTimer: ReturnType<typeof setTimeout> | null = null;
	$effect(() => {
		const savedAt = editorStore.lastSavedAt;
		const target = editorStore.statusTarget;
		const state = editorStore.saveState;
		const filePath = activeFilePath;

		if (state !== 'saved' || savedAt === null || !filePath || !target) return;
		if (!target.endsWith(filePath)) return;

		if (saveRefetchTimer !== null) clearTimeout(saveRefetchTimer);
		saveRefetchTimer = setTimeout(() => {
			saveRefetchTimer = null;
			if (activeFilePath) fetchRelated(activeFilePath);
		}, SAVE_REFETCH_DEBOUNCE_MS);
	});

	// Heuristic: when backlog progresses while we have no related rows, re-run.
	// Covers the "newly-created note indexed in background" case without a
	// per-file readiness query.
	$effect(() => {
		const done = embedStatusStore.done;
		const filePath = activeFilePath;
		if (!filePath || !filePath.endsWith('.md')) return;
		if (loading) return;
		if (hits.length > 0) return;
		if (done <= lastSeenProgressDone) return;
		fetchRelated(filePath);
	});

	onMount(() => () => {
		if (saveRefetchTimer !== null) clearTimeout(saveRefetchTimer);
	});

	// ---------------------------------------------------------------------------
	// Display helpers
	// ---------------------------------------------------------------------------

	function filenameOf(path: string): string {
		const slash = path.lastIndexOf('/');
		const base = slash === -1 ? path : path.slice(slash + 1);
		return base.replace(/\.md$/i, '');
	}
</script>

<div class="flex h-full flex-col overflow-hidden">
	{#if !activeFilePath || !activeFilePath.endsWith('.md')}
		<div class="flex h-full items-center justify-center p-6">
			<div class="flex flex-col items-center gap-2 text-center">
				<Link2 size={20} strokeWidth={1.2} class="text-on-surface-variant opacity-30" />
				<p class="text-xs text-on-surface-variant opacity-30">Open a note to see related.</p>
			</div>
		</div>
	{:else if !embedStatusStore.available}
		<div class="flex h-full items-center justify-center p-6">
			<p class="text-xs text-on-surface-variant opacity-40">Semantic search unavailable.</p>
		</div>
	{:else if loading && hits.length === 0}
		<div class="flex h-full items-center justify-center p-6">
			<p class="text-xs text-on-surface-variant opacity-40">…</p>
		</div>
	{:else if hits.length === 0}
		<div class="flex h-full items-center justify-center p-6">
			<p class="text-xs text-on-surface-variant opacity-40">
				{embedStatusStore.inProgress
					? 'Still indexing — check back shortly.'
					: 'No related notes.'}
			</p>
		</div>
	{:else}
		<div class="shrink-0 px-3 pb-1 pt-2">
			<p class="text-xs text-on-surface-variant opacity-50">
				{hits.length}
				{hits.length === 1 ? 'related note' : 'related notes'}
			</p>
		</div>

		<div class="flex-1 overflow-y-auto divide-y divide-outline-variant/10">
			{#each hits as hit (hit.chunk_id)}
				<button
					type="button"
					onclick={(e) => openSemanticHit(hit, { forceNew: e.metaKey || e.ctrlKey })}
					class="w-full border-none bg-transparent p-3 text-left hover:bg-surface-container-low focus:bg-surface-container-low focus:outline-none"
				>
					<div class="truncate text-xs font-medium text-primary">
						{filenameOf(hit.file_path)}
					</div>
					{#if hit.heading_path}
						<div class="mt-0.5 truncate text-xs text-on-surface-variant opacity-50">
							{hit.heading_path}
						</div>
					{/if}
				</button>
			{/each}
		</div>
	{/if}
</div>
