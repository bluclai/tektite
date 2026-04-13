<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import {
		filterCommands,
		formatPaletteError,
		parsePaletteQuery,
		placeholderForMode,
		type PaletteMode,
	} from '$lib/command-palette';
	import * as Command from '$lib/components/ui/command';
	import { collectMarkdownPaths, noteTemplates } from '$lib/note-templates';
	import { filesStore } from '$lib/stores/files.svelte';
	import { editorNavigationStore } from '$lib/stores/editor-navigation.svelte';
	import { editorStore } from '$lib/stores/editor.svelte';
	import { commandStore, type CommandAction } from '$lib/stores/commands.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';
	import { workspaceStore } from '$lib/stores/workspace.svelte';

	interface FuzzyFileRow {
		path: string;
		name: string;
		score: number;
	}

	interface HeadingRow {
		file_id: string;
		file_path: string;
		level: number;
		text: string;
	}

	interface TagRow {
		file_id: string;
		file_path: string;
		name: string;
	}

	let { open = $bindable(false) } = $props();

	let query = $state('');
	let fileResults = $state<FuzzyFileRow[]>([]);
	let headingResults = $state<HeadingRow[]>([]);
	let tagResults = $state<TagRow[]>([]);
	let isExecuting = $state(false);

	let debounceTimer: ReturnType<typeof setTimeout> | null = null;
	const DEBOUNCE_MS = 100;

	const parsedQuery = $derived(parsePaletteQuery(query));
	const mode = $derived(parsedQuery.mode);
	const searchTerm = $derived(parsedQuery.searchTerm);

	const filteredCommands = $derived(
		mode === 'commands' ? filterCommands(commandStore.commands, searchTerm) : []
	);
	const filteredTemplates = $derived(
		mode === 'templates'
			? noteTemplates.filter((template) => {
				const normalized = searchTerm.trim().toLowerCase();
				if (!normalized) return true;
				return [template.label, template.detail].some((value) =>
					value.toLowerCase().includes(normalized)
				);
			})
			: []
	);
	const modePlaceholder = $derived(placeholderForMode(mode));

	$effect(() => {
		if (debounceTimer) clearTimeout(debounceTimer);

		if (!searchTerm.trim()) {
			fileResults = [];
			headingResults = [];
			tagResults = [];
			return;
		}

		if (mode === 'commands' || mode === 'templates') {
			fileResults = [];
			headingResults = [];
			tagResults = [];
			return;
		}

		debounceTimer = setTimeout(async () => {
			if (!vaultStore.path) {
				fileResults = [];
				headingResults = [];
				tagResults = [];
				return;
			}

			try {
				if (mode === 'files') {
					fileResults = await invoke<FuzzyFileRow[]>('search_fuzzy_files', {
						query: searchTerm,
						limit: 20,
					});
					headingResults = [];
					tagResults = [];
					return;
				}

				if (mode === 'headings') {
					headingResults = await invoke<HeadingRow[]>('search_headings', {
						query: searchTerm,
						limit: 20,
					});
					fileResults = [];
					tagResults = [];
					return;
				}

				tagResults = await invoke<TagRow[]>('search_tags', {
					query: searchTerm,
					limit: 20,
				});
				fileResults = [];
				headingResults = [];
			} catch (error) {
				console.error('Command palette search error:', error);
				fileResults = [];
				headingResults = [];
				tagResults = [];
				editorStore.setSaveState('error', {
					detail: formatPaletteError(error, `Failed to search ${mode}.`),
					target: searchTerm,
				});
			}
		}, DEBOUNCE_MS);
	});

	function resetPalette() {
		open = false;
		query = '';
		fileResults = [];
		headingResults = [];
		tagResults = [];
	}

	function toAbsolutePath(path: string): string {
		if (!vaultStore.path || path.startsWith(vaultStore.path)) {
			return path;
		}

		return `${vaultStore.path}/${path}`;
	}

	function openVaultPath(path: string, detail: string) {
		workspaceStore.openTab(toAbsolutePath(path));
		editorStore.setSaveState('saved', {
			detail,
			target: path,
		});
		resetPalette();
	}

	function openFile(path: string) {
		openVaultPath(path, 'Opened file from command palette');
	}

	function openHeading(row: HeadingRow) {
		editorNavigationStore.requestHeading(row.file_path, row.text, row.level);
		openVaultPath(row.file_path, `Opened heading ${'#'.repeat(row.level)} ${row.text}`);
	}

	function openTag(row: TagRow) {
		editorNavigationStore.requestTag(row.file_path, row.name);
		openVaultPath(row.file_path, `Opened tag #${row.name}`);
	}

	async function runCommand(cmd: CommandAction) {
		if (isExecuting) return;

		isExecuting = true;
		try {
			await cmd.action();
			editorStore.setSaveState('saved', {
				detail: 'Command executed',
				target: cmd.label,
			});
			resetPalette();
		} catch (error) {
			editorStore.setSaveState('error', {
				detail: formatPaletteError(error, 'Failed to run command.'),
				target: cmd.label,
			});
		} finally {
			isExecuting = false;
		}
	}

	async function runTemplate(template: (typeof noteTemplates)[number]) {
		if (isExecuting) return;

		isExecuting = true;
		try {
			const existingPaths = collectMarkdownPaths(filesStore.tree);
			const built = template.build({
				existingPaths,
				now: new Date(),
			});
			await filesStore.createFile(built.path, built.content);
			workspaceStore.openTab(built.path);
			editorStore.setSaveState('saved', {
				detail: built.successDetail,
				target: built.path,
			});
			resetPalette();
		} catch (error) {
			editorStore.setSaveState('error', {
				detail: formatPaletteError(error, 'Failed to run template.'),
				target: template.label,
			});
		} finally {
			isExecuting = false;
		}
	}

	function emptyMessageFor(mode: PaletteMode, term: string): string {
		if (mode === 'files') {
			return term.trim() ? 'No files found.' : 'Type to search files...';
		}

		if (mode === 'commands') {
			return term.trim() ? 'No commands found.' : 'Type to find a command...';
		}

		if (mode === 'headings') {
			return term.trim() ? 'No headings found.' : 'Type to search headings...';
		}

		if (mode === 'tags') {
			return term.trim() ? 'No tags found.' : 'Type to search tags...';
		}

		return term.trim() ? 'No templates found.' : 'Type to search templates...';
	}
</script>

<Command.Dialog {open} title="Command Palette" description="" shouldFilter={false}>
	{#snippet children()}
		<Command.Input bind:value={query} placeholder={modePlaceholder} autofocus disabled={isExecuting} />
		<Command.List>
			{#if mode === 'files'}
				{#if fileResults.length === 0}
					<Command.Empty>{emptyMessageFor(mode, searchTerm)}</Command.Empty>
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
					<Command.Empty>{emptyMessageFor(mode, searchTerm)}</Command.Empty>
				{:else}
					<Command.Group heading="Commands">
						{#each filteredCommands as cmd (cmd.id)}
							<Command.Item onSelect={() => runCommand(cmd)} disabled={isExecuting}>
								<div class="flex flex-1 items-center justify-between gap-3">
									<div class="min-w-0">
										<div class="truncate">{cmd.label}</div>
										{#if cmd.category}
											<div class="text-xs text-on-surface-variant/50">{cmd.category}</div>
										{/if}
									</div>
									{#if cmd.shortcut}
										<Command.Shortcut>{cmd.shortcut}</Command.Shortcut>
									{/if}
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{:else if mode === 'headings'}
				{#if headingResults.length === 0}
					<Command.Empty>{emptyMessageFor(mode, searchTerm)}</Command.Empty>
				{:else}
					<Command.Group heading="Headings">
						{#each headingResults as heading (`${heading.file_path}:${heading.level}:${heading.text}`)}
							<Command.Item onSelect={() => openHeading(heading)}>
								<div class="flex-1">
									<div class="truncate text-sm font-medium">
										{'#'.repeat(heading.level)} {heading.text}
									</div>
									<div class="text-xs text-on-surface-variant/50">{heading.file_path}</div>
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{:else if mode === 'tags'}
				{#if tagResults.length === 0}
					<Command.Empty>{emptyMessageFor(mode, searchTerm)}</Command.Empty>
				{:else}
					<Command.Group heading="Tags">
						{#each tagResults as tag (`${tag.file_path}:${tag.name}`)}
							<Command.Item onSelect={() => openTag(tag)}>
								<div class="flex-1">
									<div class="truncate text-sm font-medium">#{tag.name}</div>
									<div class="text-xs text-on-surface-variant/50">{tag.file_path}</div>
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{:else}
				{#if filteredTemplates.length === 0}
					<Command.Empty>{emptyMessageFor(mode, searchTerm)}</Command.Empty>
				{:else}
					<Command.Group heading="Templates">
						{#each filteredTemplates as template (template.id)}
							<Command.Item onSelect={() => runTemplate(template)} disabled={isExecuting}>
								<div class="flex-1">
									<div class="truncate text-sm font-medium">{template.label}</div>
									<div class="text-xs text-on-surface-variant/50">{template.detail}</div>
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{/if}
		</Command.List>
	{/snippet}
</Command.Dialog>
