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

	interface CommandAction {
		id: string;
		label: string;
		shortcut?: string;
		action: () => void;
	}

	let { open = $bindable(false) } = $props();

	let query = $state('');
	let fileResults = $state<FuzzyFileRow[]>([]);

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
	const mode = $derived(query.startsWith('>') ? 'commands' : 'files');

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
			: 'Search files...'
	);

	// Debounced search
	$effect(() => {
		if (debounceTimer) clearTimeout(debounceTimer);

		// Clear results for empty search
		if (!searchTerm.trim()) {
			fileResults = [];
			return;
		}

		// Only search in files mode.
		if (mode === 'commands') return;

		debounceTimer = setTimeout(async () => {
			// Don't search if vault not open
			if (!vaultStore.path) {
				fileResults = [];
				return;
			}

			try {
				const rows = await invoke<FuzzyFileRow[]>('search_fuzzy_files', {
					query: searchTerm,
					limit: 20,
				});
				fileResults = rows;
			} catch (e) {
				console.error('Search error:', e);
				fileResults = [];
			}
		}, DEBOUNCE_MS);
	});

	function toAbsolutePath(path: string): string {
		if (!vaultStore.path || path.startsWith(vaultStore.path)) {
			return path;
		}

		return `${vaultStore.path}/${path}`;
	}

	function openFile(path: string) {
		workspaceStore.openTab(toAbsolutePath(path));
		open = false;
		query = '';
	}

	function runCommand(cmd: CommandAction) {
		cmd.action();
		open = false;
		query = '';
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
			{/if}
		</Command.List>
	{/snippet}
</Command.Dialog>
