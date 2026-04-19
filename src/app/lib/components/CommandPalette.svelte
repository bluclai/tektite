<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import {
		dedupeSemanticHitsByFile,
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
	import { embedStatusStore } from '$lib/stores/embedStatus.svelte';
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

	interface SemanticHit {
		chunk_id: string;
		file_path: string;
		heading_path: string | null;
		heading_text: string | null;
		heading_level: number | null;
		snippet: string;
		score: number;
	}

	const SEMANTIC_OVER_FETCH = 25;
	const SEMANTIC_DISPLAY_LIMIT = 10;

	let { open = $bindable(false) } = $props();

	let query = $state('');
	let fileResults = $state<FuzzyFileRow[]>([]);
	let headingResults = $state<HeadingRow[]>([]);
	let tagResults = $state<TagRow[]>([]);
	let semanticResults = $state<SemanticHit[]>([]);
	let semanticSearching = $state(false);
	let isExecuting = $state(false);

	let debounceTimer: ReturnType<typeof setTimeout> | null = null;
	let latestRequestId = 0;
	const FTS_DEBOUNCE_MS = 100;
	const SEMANTIC_DEBOUNCE_MS = 250;

	/**
	 * bits.Command `onSelect` does not receive the originating event, so we
	 * can't read modifier keys on the selection itself. Capture them on the
	 * dialog root and let the next `onSelect` consume the flag.
	 */
	let nextForceNew = $state(false);

	function consumeForceNew(): boolean {
		const v = nextForceNew;
		nextForceNew = false;
		return v;
	}

	function onDialogPointerDown(e: PointerEvent) {
		nextForceNew = e.metaKey || e.ctrlKey;
	}

	function onDialogKeyDown(e: KeyboardEvent) {
		// Cmd/Ctrl+Enter triggers an append from the current selection.
		if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
			nextForceNew = true;
		}
	}

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
			semanticResults = [];
			semanticSearching = false;
			// Any in-flight result is now stale — bump the id so it can't land.
			latestRequestId += 1;
			return;
		}

		if (mode === 'commands' || mode === 'templates') {
			fileResults = [];
			headingResults = [];
			tagResults = [];
			semanticResults = [];
			semanticSearching = false;
			return;
		}

		const delay = mode === 'semantic' ? SEMANTIC_DEBOUNCE_MS : FTS_DEBOUNCE_MS;

		// Indicate pending semantic work up front so the UI can show a
		// "searching" row during the debounce window without a flash of
		// "no matches" after a cleared result.
		if (mode === 'semantic') {
			semanticSearching = true;
		}

		debounceTimer = setTimeout(async () => {
			if (!vaultStore.path) {
				fileResults = [];
				headingResults = [];
				tagResults = [];
				semanticResults = [];
				semanticSearching = false;
				return;
			}

			// Tag this request so concurrent / superseded responses can be
			// discarded when they finally resolve.
			const requestId = ++latestRequestId;

			try {
				if (mode === 'files') {
					const result = await invoke<FuzzyFileRow[]>('search_fuzzy_files', {
						query: searchTerm,
						limit: 20,
					});
					if (requestId !== latestRequestId) return;
					fileResults = result;
					headingResults = [];
					tagResults = [];
					semanticResults = [];
					return;
				}

				if (mode === 'headings') {
					const result = await invoke<HeadingRow[]>('search_headings', {
						query: searchTerm,
						limit: 20,
					});
					if (requestId !== latestRequestId) return;
					headingResults = result;
					fileResults = [];
					tagResults = [];
					semanticResults = [];
					return;
				}

				if (mode === 'semantic') {
					const hits = await invoke<SemanticHit[]>('search_semantic', {
						query: searchTerm,
						limit: SEMANTIC_OVER_FETCH,
					});
					if (requestId !== latestRequestId) return;
					semanticResults = dedupeSemanticHitsByFile(hits, SEMANTIC_DISPLAY_LIMIT);
					semanticSearching = false;
					fileResults = [];
					headingResults = [];
					tagResults = [];
					return;
				}

				const result = await invoke<TagRow[]>('search_tags', {
					query: searchTerm,
					limit: 20,
				});
				if (requestId !== latestRequestId) return;
				tagResults = result;
				fileResults = [];
				headingResults = [];
				semanticResults = [];
			} catch (error) {
				if (requestId !== latestRequestId) return;
				console.error('Command palette search error:', error);
				fileResults = [];
				headingResults = [];
				tagResults = [];
				semanticResults = [];
				semanticSearching = false;
				editorStore.setSaveState('error', {
					detail: formatPaletteError(error, `Failed to search ${mode}.`),
					target: searchTerm,
				});
			}
		}, delay);
	});

	function resetPalette() {
		open = false;
		query = '';
		fileResults = [];
		headingResults = [];
		tagResults = [];
		semanticResults = [];
		semanticSearching = false;
		latestRequestId += 1;
	}

	function toAbsolutePath(path: string): string {
		if (!vaultStore.path || path.startsWith(vaultStore.path)) {
			return path;
		}

		return `${vaultStore.path}/${path}`;
	}

	function openVaultPath(path: string, detail: string) {
		const forceNew = consumeForceNew();
		workspaceStore.openTab(toAbsolutePath(path), { forceNew });
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

	function openSemanticHit(hit: SemanticHit) {
		if (hit.heading_text && hit.heading_level) {
			editorNavigationStore.requestHeading(hit.file_path, hit.heading_text, hit.heading_level);
			openVaultPath(
				hit.file_path,
				`Opened ${'#'.repeat(hit.heading_level)} ${hit.heading_text}`,
			);
			return;
		}
		openFile(hit.file_path);
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
			// Creating a note from a template is an intentional new doc —
			// commit as a fresh tab instead of swapping.
			workspaceStore.openTab(built.path, { forceNew: true });
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

	function filenameOf(path: string): string {
		const slash = path.lastIndexOf('/');
		const base = slash === -1 ? path : path.slice(slash + 1);
		return base.replace(/\.md$/i, '');
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

		if (mode === 'semantic') {
			return term.trim() ? 'No matches found.' : 'Type to search by meaning…';
		}

		return term.trim() ? 'No templates found.' : 'Type to search templates...';
	}
</script>

<Command.Dialog {open} title="Command Palette" description="" shouldFilter={false}>
	{#snippet children()}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div onpointerdowncapture={onDialogPointerDown} onkeydowncapture={onDialogKeyDown} class="contents">
		<Command.Input bind:value={query} placeholder={modePlaceholder} autofocus disabled={isExecuting} />
		<Command.List>
			{#if query === ''}
				<div
					class="select-none px-3 py-2 text-xs"
					style="color: var(--color-text-faint);"
				>
					<span class="tabular-nums">?</span> by meaning &nbsp;
					<span class="tabular-nums">&gt;</span> commands &nbsp;
					<span class="tabular-nums">#</span> headings &nbsp;
					<span class="tabular-nums">@</span> tags &nbsp;
					<span class="tabular-nums">/</span> templates
				</div>
			{/if}
			{#if mode === 'semantic' && embedStatusStore.inProgress}
				<div
					class="select-none px-3 py-1.5 text-xs"
					style="color: var(--color-text-muted); background-color: var(--color-surface);"
				>
					Indexing {embedStatusStore.done}/{embedStatusStore.total} notes — results are partial.
				</div>
			{/if}
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
			{:else if mode === 'templates'}
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
			{:else if mode === 'semantic'}
				{#if !embedStatusStore.available}
					<Command.Empty>Semantic search unavailable.</Command.Empty>
				{:else if !searchTerm.trim()}
					<Command.Empty>{emptyMessageFor(mode, searchTerm)}</Command.Empty>
				{:else if semanticSearching && semanticResults.length === 0}
					<Command.Empty>Searching…</Command.Empty>
				{:else if semanticResults.length === 0}
					<Command.Empty>{emptyMessageFor(mode, searchTerm)}</Command.Empty>
				{:else}
					<Command.Group heading="Semantic matches">
						{#each semanticResults as hit (hit.chunk_id)}
							<Command.Item onSelect={() => openSemanticHit(hit)}>
								<div class="min-w-0 flex-1">
									<div class="truncate text-sm font-medium">{filenameOf(hit.file_path)}</div>
									{#if hit.heading_path}
										<div class="truncate text-xs text-on-surface-variant/50">
											{hit.heading_path}
										</div>
									{/if}
									<div
										class="text-xs line-clamp-2"
										style="color: var(--color-text-muted);"
									>
										{hit.snippet}
									</div>
								</div>
							</Command.Item>
						{/each}
					</Command.Group>
				{/if}
			{/if}
		</Command.List>
		</div>
	{/snippet}
</Command.Dialog>
