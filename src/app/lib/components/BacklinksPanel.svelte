<script lang="ts">
	/**
	 * BacklinksPanel — shows all notes that link to the currently open file.
	 *
	 * Reactively watches the active pane's active tab. When the active file
	 * changes, fetches fresh backlinks from the backend index. Clicking a
	 * backlink entry opens that note in the active pane.
	 */
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';
	import { Network } from 'lucide-svelte';
	import { workspaceStore, allLeaves } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';

	// ---------------------------------------------------------------------------
	// Types (mirrors BacklinkEntry on the Rust side)
	// ---------------------------------------------------------------------------

	interface BacklinkEntry {
		source_path: string;
		source_title: string;
		target: string;
		fragment: string | null;
		alias: string | null;
	}

	// ---------------------------------------------------------------------------
	// Derive the active file's vault-relative path from the pane tree
	// ---------------------------------------------------------------------------

	/** Vault-relative path of the file currently open in the active pane. */
	let activeFilePath = $derived.by(() => {
		const leaves = allLeaves(workspaceStore.paneTree);
		const activeLeaf = leaves.find((l) => l.id === workspaceStore.activePaneId);
		if (!activeLeaf || !activeLeaf.activeTabId) return null;
		const tab = activeLeaf.tabs.find((t) => t.id === activeLeaf.activeTabId);
		if (!tab) return null;

		// Tabs store absolute paths; strip vault root to get vault-relative path.
		const vaultRoot = vaultStore.path;
		if (vaultRoot && tab.path.startsWith(vaultRoot + '/')) {
			return tab.path.slice(vaultRoot.length + 1);
		}
		return tab.path;
	});

	// ---------------------------------------------------------------------------
	// Backlink state
	// ---------------------------------------------------------------------------

	let backlinks = $state<BacklinkEntry[]>([]);
	let loading = $state(false);
	let error = $state<string | null>(null);

	// ---------------------------------------------------------------------------
	// Fetch backlinks whenever the active file changes
	// ---------------------------------------------------------------------------

	/** Fetch backlinks for the given vault-relative path. */
	function fetchBacklinks(filePath: string) {
		loading = true;
		error = null;

		invoke<BacklinkEntry[]>('index_get_backlinks', { filePath })
			.then((rows) => {
				backlinks = rows;
			})
			.catch((e) => {
				error = String(e);
				backlinks = [];
			})
			.finally(() => {
				loading = false;
			});
	}

	$effect(() => {
		const filePath = activeFilePath;

		if (!filePath || !filePath.endsWith('.md')) {
			backlinks = [];
			loading = false;
			error = null;
			return;
		}

		fetchBacklinks(filePath);
	});

	// Re-fetch when the vault index changes (file created/modified/deleted).
	// This keeps the panel current without requiring the user to switch tabs.
	onMount(() => {
		const unlisten = listen('file-tree-updated', () => {
			const filePath = activeFilePath;
			if (filePath && filePath.endsWith('.md')) {
				fetchBacklinks(filePath);
			}
		});

		return () => {
			void unlisten.then((fn) => fn());
		};
	});

	// ---------------------------------------------------------------------------
	// Actions
	// ---------------------------------------------------------------------------

	function openSource(entry: BacklinkEntry) {
		// source_path is vault-relative; openTab expects absolute path.
		const vaultRoot = vaultStore.path;
		const absPath =
			vaultRoot && !entry.source_path.startsWith(vaultRoot)
				? `${vaultRoot}/${entry.source_path}`
				: entry.source_path;
		workspaceStore.openTab(absPath);
	}

	/** Format the link reference as it appears in the source — e.g. [[Note#heading|alias]] */
	function formatLink(entry: BacklinkEntry): string {
		let text = entry.target;
		if (entry.fragment) text += `#${entry.fragment}`;
		if (entry.alias) text += `|${entry.alias}`;
		return `[[${text}]]`;
	}

	function getDisplayTitle(entry: BacklinkEntry): string {
		return entry.source_title || entry.source_path.split('/').pop() || entry.source_path;
	}
</script>

<div class="flex h-full flex-col overflow-hidden">
	{#if !activeFilePath || !activeFilePath.endsWith('.md')}
		<!-- No active markdown file -->
		<div class="flex h-full items-center justify-center p-6">
			<div class="flex flex-col items-center gap-2 text-center">
				<Network size={20} strokeWidth={1.2} class="text-on-surface-variant opacity-30" />
				<p class="text-xs text-on-surface-variant opacity-30">Open a note to see its backlinks</p>
			</div>
		</div>
	{:else if loading}
		<div class="flex h-full items-center justify-center p-6">
			<p class="text-xs text-on-surface-variant opacity-40">Loading backlinks…</p>
		</div>
	{:else if error}
		<div class="flex h-full items-center justify-center p-6">
			<p class="text-xs text-red-400 opacity-70">{error}</p>
		</div>
	{:else if backlinks.length === 0}
		<div class="flex h-full items-center justify-center p-6">
			<div class="flex flex-col items-center gap-2 text-center">
				<Network size={20} strokeWidth={1.2} class="text-on-surface-variant opacity-20" />
				<p class="text-xs text-on-surface-variant opacity-30">No backlinks found</p>
			</div>
		</div>
	{:else}
		<!-- Backlink count header -->
		<div class="shrink-0 px-3 pb-1 pt-2">
			<p class="text-xs text-on-surface-variant opacity-50">
				{backlinks.length}
				{backlinks.length === 1 ? 'link' : 'links'} to this note
			</p>
		</div>

		<!-- Results list -->
		<div class="flex-1 overflow-y-auto divide-y divide-outline-variant/10">
			{#each backlinks as entry (entry.source_path + entry.target + (entry.fragment ?? ''))}
				<button
					type="button"
					onclick={() => openSource(entry)}
					class="w-full border-none bg-transparent p-3 text-left hover:bg-surface-container-low focus:bg-surface-container-low focus:outline-none"
				>
					<!-- Source note name -->
					<div class="mb-0.5 text-xs font-medium text-primary">
						{getDisplayTitle(entry)}
					</div>

					<!-- Source path hint -->
					<div class="mb-1 truncate text-xs text-on-surface-variant opacity-40">
						{entry.source_path}
					</div>

					<!-- Link reference as written -->
					<div class="font-mono text-xs text-on-surface-variant opacity-60">
						{formatLink(entry)}
					</div>
				</button>
			{/each}
		</div>
	{/if}
</div>
