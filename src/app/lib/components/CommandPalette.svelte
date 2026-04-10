<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { workspaceStore } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';
	import * as Command from '$lib/components/ui/command';

	interface FuzzyFileRow {
		path: string;
		name: string;
		score: number;
	}

	interface HeadingSearchRow {
		file_id: string;
		file_path: string;
		level: number;
		text: string;
	}

	interface CommandAction {
		id: string;
		label: string;
		shortcut?: string;
		action: () => void;
	}

	let { open = $bindable(false) } = $props();

	let query = $state('');
	let fileResults = $state<FuzzyFileRow[]>([]);
	let headingResults = $state<HeadingSearchRow[]>([]);
	let loading = $state(false);

	let debounceTimer: ReturnType<typeof setTimeout> | null = null;
	const DEBOUNCE_MS = 100;

	const commands: CommandAction[] = [
		{
			id: 'toggle-sidebar',
			label: 'Toggle Sidebar',
			shortcut: '⌘B',
			action: () => workspaceStore.toggleSidebar(),
		},
		{
			id: 'toggle-preview',
			label: 'Toggle Preview Mode',
			shortcut: '⌘⇧L',
			action: () => workspaceStore.togglePreviewMode(),
		},
		{
			id: 'panel-files',
			label: 'Go to Files panel',
			action: () => workspaceStore.setActivePanel('files'),
		},
		{
			id: 'panel-search',
			label: 'Go to Search panel',
			action: () => workspaceStore.setActivePanel('search'),
		},
		{
			id: 'panel-backlinks',
			label: 'Go to Backlinks panel',
			action: () => workspaceStore.setActivePanel('backlinks'),
		},
	];

	// Detect mode from query prefix
	const mode = $derived(
		query.startsWith('>') ? 'commands' : query.startsWith('#') ? 'headings' : 'files'
	);

	// Extract search term (remove prefix)
	const searchTerm = $derived(mode === 'files' ? query : query.slice(1).trimStart());

	// Filter commands for command mode
	const filteredCommands = $derived(
		mode === 'commands'
			? commands.filter((c) => c.label.toLowerCase().includes(searchTerm.toLowerCase()))
			: []
	);

	// Mode placeholder text
	const modePlaceholder = $derived(
		mode === 'commands'
			? 'Type a command...'
			: mode === 'headings'
				? 'Search headings...'
				: 'Search files...'
	);

	// Debounced search
	$effect(() => {
		if (debounceTimer) clearTimeout(debounceTimer);

		// Clear results for empty search
		if (!searchTerm.trim()) {
			fileResults = [];
			headingResults = [];
			return;
		}

		// Only search in files or headings mode
		if (mode === 'commands') return;

		debounceTimer = setTimeout(async () => {
			// Don't search if vault not open
			if (!vaultStore.path) {
				fileResults = [];
				headingResults = [];
				return;
			}

			loading = true;
			try {
				if (mode === 'files') {
					const rows = await invoke<FuzzyFileRow[]>('search_fuzzy_files', {
						query: searchTerm,
						limit: 20,
					});
					fileResults = rows;
				} else if (mode === 'headings') {
					const rows = await invoke<HeadingSearchRow[]>('search_headings', {
						query: searchTerm,
						limit: 20,
					});
					headingResults = rows;
				}
			} catch (e) {
				console.error('Search error:', e);
				fileResults = [];
				headingResults = [];
			} finally {
				loading = false;
			}
		}, DEBOUNCE_MS);
	});

	function openFile(path: string) {
		workspaceStore.openTab(path);
		open = false;
		query = '';
	}

	function runCommand(cmd: CommandAction) {
		cmd.action();
		open = false;
		query = '';
	}

	function getFileName(path: string): string {
		return path.split('/').pop() ?? path;
	}

	function getLevelIndicator(level: number): string {
		return '#'.repeat(level);
	}
</script>

<Command.Dialog {open} title="Command Palette" description="" shouldFilter={false}>
	{#snippet children()}
		<Command.Input bind:value={query} placeholder={modePlaceholder} autofocus />
		<Command.List>
			{#if mode === 'files'}
				{#if fileResults.length === 0}
					{#if searchTerm.trim()}
						<Command.Empty>No files found.</Command.Empty>
					{:else}
						<Command.Empty>Type to search files...</Command.Empty>
					{/if}
				{:else}
					<Command.Group heading="Files">
						{#each fileResults as file (file.path)}
							<Command.Item onSelect={() => openFile(file.path)}>
								<div class="flex-1">
									<div class="text-sm font-medium">{file.name}</div>
									<div class="text-xs text-on-surface-variant/50">{file.path}</div>
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{:else if mode === 'commands'}
				{#if filteredCommands.length === 0}
					{#if searchTerm.trim()}
						<Command.Empty>No commands found.</Command.Empty>
					{:else}
						<Command.Empty>Type to find a command...</Command.Empty>
					{/if}
				{:else}
					<Command.Group heading="Commands">
						{#each filteredCommands as cmd (cmd.id)}
							<Command.Item onSelect={() => runCommand(cmd)}>
								<div class="flex flex-1 items-center justify-between">
									<span>{cmd.label}</span>
									{#if cmd.shortcut}
										<Command.Shortcut>{cmd.shortcut}</Command.Shortcut>
									{/if}
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{:else}
				<!-- Headings mode -->
				{#if headingResults.length === 0}
					{#if searchTerm.trim()}
						<Command.Empty>No headings found.</Command.Empty>
					{:else}
						<Command.Empty>Type to search headings...</Command.Empty>
					{/if}
				{:else}
					<Command.Group heading="Headings">
						{#each headingResults as heading (heading.file_id + heading.text)}
							<Command.Item onSelect={() => openFile(heading.file_path)}>
								<div class="flex flex-1 items-center gap-2">
									<span class="text-xs text-on-surface-variant/50">{getLevelIndicator(heading.level)}</span>
									<div class="flex-1">
										<div class="text-sm">{heading.text}</div>
										<div class="text-xs text-on-surface-variant/50">{heading.file_path}</div>
									</div>
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{/if}
		</Command.List>
	{/snippet}
</Command.Dialog>
