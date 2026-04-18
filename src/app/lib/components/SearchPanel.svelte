<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { workspaceStore } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';

	interface FtsRow {
		id: string;
		path: string;
		title: string;
		snippet: string;
		rank: number;
	}

	let query = $state('');
	let results = $state<FtsRow[]>([]);
	let loading = $state(false);

	let debounceTimer: ReturnType<typeof setTimeout> | null = null;
	const DEBOUNCE_MS = 150;

	$effect(() => {
		const q = query;

		// Clear results if query is empty
		if (!q.trim()) {
			results = [];
			return;
		}

		// Debounce the search
		if (debounceTimer) clearTimeout(debounceTimer);
		debounceTimer = setTimeout(async () => {
			loading = true;
			try {
				const rows = await invoke<FtsRow[]>('search_full_text', {
					query: q,
					limit: 20,
				});
				results = rows;
			} catch (e) {
				console.error('Search error:', e);
				results = [];
			} finally {
				loading = false;
			}
		}, DEBOUNCE_MS);
	});

	/**
	 * Formats FTS snippet by converting __MATCH__ markers to <mark> tags.
	 * Escapes HTML first to prevent injection, then inserts safe markup.
	 */
	function formatSnippet(snippet: string): string {
		// HTML-escape the content
		const escaped = snippet
			.replace(/&/g, '&amp;')
			.replace(/</g, '&lt;')
			.replace(/>/g, '&gt;')
			.replace(/"/g, '&quot;');

		// Replace markers with HTML tags
		return escaped
			.replace(/__MATCH__/g, '<mark>')
			.replace(/__ENDMATCH__/g, '</mark>');
	}

	function toAbsolutePath(path: string): string {
		if (!vaultStore.path || path.startsWith(vaultStore.path)) {
			return path;
		}

		return `${vaultStore.path}/${path}`;
	}

	function openFile(path: string) {
		workspaceStore.openTab(toAbsolutePath(path));
		query = '';
	}

	function getFileName(path: string): string {
		return path.split('/').pop() ?? path;
	}

	function getTitleOrPath(row: FtsRow): string {
		return row.title || getFileName(row.path);
	}

	function getResultKey(row: FtsRow, index: number): string {
		return `${row.path}:${row.id}:${index}`;
	}
</script>

<div class="flex h-full flex-col overflow-hidden bg-surface">
	<!-- Search input -->
	<div class="p-3 pb-4">
		<input
			type="text"
			bind:value={query}
			placeholder="Search vault..."
			class="w-full bg-surface-container-low px-3 py-2 text-xs outline-none"
		/>
	</div>

	<!-- Results -->
	<div class="flex-1 overflow-y-auto">
		{#if loading}
			<div class="flex items-center justify-center p-8">
				<p class="text-xs text-on-surface-variant/50">Searching...</p>
			</div>
		{:else if results.length === 0}
			{#if query.trim()}
				<div class="flex items-center justify-center p-8">
					<p class="text-xs text-on-surface-variant/50">No results found</p>
				</div>
			{:else}
				<div class="flex items-center justify-center p-8">
					<p class="text-xs text-on-surface-variant/30">Start typing to search...</p>
				</div>
			{/if}
		{:else}
			<div class="divide-y divide-outline-variant/10">
				{#each results as row, index (getResultKey(row, index))}
					<button
						type="button"
						onclick={() => openFile(row.path)}
						class="w-full border-none bg-transparent p-3 text-left hover:bg-surface-container-low focus:bg-surface-container-low focus:outline-none"
					>
						<!-- File title/name -->
						<div class="text-xs font-medium text-primary mb-1">
							{getTitleOrPath(row)}
						</div>

						<!-- Path hint -->
						<div class="text-xs text-on-surface-variant/50 mb-1">
							{row.path}
						</div>

						<!-- Snippet with highlighted matches -->
						<div class="text-xs text-on-surface-variant leading-relaxed">
							<!-- eslint-disable-next-line svelte/no-at-html-tags -->
							{@html formatSnippet(row.snippet)}
						</div>
					</button>
				{/each}
			</div>
		{/if}
	</div>
</div>

<style>
	:global(mark) {
		background-color: color-mix(in srgb, var(--color-primary) 20%, transparent);
		color: var(--color-primary);
		font-weight: 600;
	}
</style>
