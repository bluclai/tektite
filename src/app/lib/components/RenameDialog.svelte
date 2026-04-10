<script lang="ts">
	/**
	 * RenameDialog — preview-and-apply rename flow.
	 *
	 * Shows the proposed new name (editable), calls vault_plan_rename to fetch
	 * the list of wiki-link rewrites, presents the before/after diff, and
	 * applies on confirm.
	 *
	 * Props:
	 *   open        — controls dialog visibility (bindable)
	 *   oldRelPath  — vault-relative path of the file being renamed
	 *   vaultRoot   — absolute vault root (used to build absolute paths)
	 *   onRenamed   — callback fired after a successful rename
	 */
	import { invoke } from '@tauri-apps/api/core';
	import { filesStore } from '$lib/stores/files.svelte';

	// ---------------------------------------------------------------------------
	// Types (mirror Rust RenamePlan / RenameEdit)
	// ---------------------------------------------------------------------------

	interface RenameEdit {
		file_path: string;
		before: string;
		after: string;
	}

	interface RenamePlan {
		old_path: string;
		new_path: string;
		edits: RenameEdit[];
	}

	// ---------------------------------------------------------------------------
	// Props
	// ---------------------------------------------------------------------------

	interface Props {
		open: boolean;
		oldRelPath: string;
		vaultRoot: string;
		onRenamed?: (newRelPath: string) => void;
		onClose?: () => void;
	}

	let { open = $bindable(), oldRelPath, vaultRoot, onRenamed, onClose }: Props = $props();

	// ---------------------------------------------------------------------------
	// Local state
	// ---------------------------------------------------------------------------

	/** Editable new filename (just the basename, without extension if .md). */
	let newName = $state('');
	let plan = $state<RenamePlan | null>(null);
	let planning = $state(false);
	let applying = $state(false);
	let planError = $state<string | null>(null);
	let applyError = $state<string | null>(null);

	/** Group edits by file path for display. */
	let editsByFile = $derived.by(() => {
		if (!plan) return [];
		const map = new Map<string, RenameEdit[]>();
		for (const edit of plan.edits) {
			const list = map.get(edit.file_path) ?? [];
			list.push(edit);
			map.set(edit.file_path, list);
		}
		return [...map.entries()].map(([filePath, edits]) => ({ filePath, edits }));
	});

	// ---------------------------------------------------------------------------
	// Derived: new vault-relative path
	// ---------------------------------------------------------------------------

	let newRelPath = $derived.by(() => {
		if (!newName.trim()) return '';
		const parts = oldRelPath.split('/');
		const oldBasename = parts[parts.length - 1];
		const isMarkdown = oldBasename.endsWith('.md');
		const newBasename = isMarkdown
			? newName.trim().endsWith('.md')
				? newName.trim()
				: `${newName.trim()}.md`
			: newName.trim();
		return [...parts.slice(0, -1), newBasename].join('/');
	});

	// ---------------------------------------------------------------------------
	// Initialise when dialog opens
	// ---------------------------------------------------------------------------

	$effect(() => {
		if (open) {
			// Pre-fill the name field with the stem (without .md)
			const basename = oldRelPath.split('/').pop() ?? oldRelPath;
			newName = basename.endsWith('.md') ? basename.slice(0, -3) : basename;
			plan = null;
			planError = null;
			applyError = null;
		}
	});

	// ---------------------------------------------------------------------------
	// Plan fetch — debounced on newName changes
	// ---------------------------------------------------------------------------

	let planTimer: ReturnType<typeof setTimeout> | null = null;

	$effect(() => {
		// Track newRelPath so this reruns when it changes
		const target = newRelPath;
		plan = null;
		planError = null;

		if (!target || target === oldRelPath) return;

		if (planTimer) clearTimeout(planTimer);
		planTimer = setTimeout(async () => {
			planning = true;
			try {
				plan = await invoke<RenamePlan>('vault_plan_rename', {
					oldPath: oldRelPath,
					newPath: target,
				});
			} catch (e) {
				planError = String(e);
			} finally {
				planning = false;
			}
		}, 300);
	});

	// ---------------------------------------------------------------------------
	// Apply
	// ---------------------------------------------------------------------------

	async function applyRename() {
		if (!plan) return;
		applying = true;
		applyError = null;
		try {
			await invoke<void>('vault_apply_rename', { plan });
			// Refresh the file tree
			await filesStore.refresh();
			onRenamed?.(newRelPath);
			open = false;
		} catch (e) {
			applyError = String(e);
		} finally {
			applying = false;
		}
	}

	function close() {
		open = false;
		onClose?.();
	}

	function onKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') close();
		if (e.key === 'Enter' && plan && !applying) void applyRename();
	}
</script>

{#if open}
	<!-- Backdrop -->
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<div
		class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm"
		role="presentation"
		onclick={(e) => { if (e.target === e.currentTarget) close(); }}
	>
		<!-- Dialog surface -->
		<div
			class="relative w-[520px] max-w-[90vw] rounded-xl border border-outline-variant/20 bg-surface-container shadow-2xl"
			role="dialog"
			aria-modal="true"
			aria-label="Rename file"
			tabindex="-1"
			onkeydown={onKeydown}
		>
			<!-- Header -->
			<div class="border-b border-outline-variant/15 px-5 py-4">
				<h2 class="text-sm font-medium text-on-surface">Rename file</h2>
				<p class="mt-0.5 truncate text-xs text-on-surface-variant opacity-50">{oldRelPath}</p>
			</div>

			<!-- Body -->
			<div class="px-5 py-4">
				<!-- New name input -->
				<label class="mb-3 block">
					<span class="mb-1.5 block text-xs font-medium text-on-surface-variant opacity-70">New name</span>
					<!-- svelte-ignore a11y_autofocus -->
					<input
						type="text"
						bind:value={newName}
						autofocus
						placeholder="note-name"
						class="w-full rounded-lg border border-outline-variant/30 bg-surface-container-low px-3 py-2 text-sm text-on-surface outline-none transition-colors focus:border-primary focus:ring-1 focus:ring-primary/40"
					/>
				</label>

				<!-- Preview section -->
				{#if newRelPath && newRelPath !== oldRelPath}
					<div class="mb-3 rounded-lg bg-surface-container-low p-3">
						<div class="flex items-center gap-2 text-xs text-on-surface-variant opacity-60">
							<span class="line-through">{oldRelPath}</span>
							<span>→</span>
							<span class="text-primary">{newRelPath}</span>
						</div>
					</div>
				{/if}

				<!-- Link rewrite preview -->
				{#if planning}
					<p class="text-xs text-on-surface-variant opacity-40">Computing link rewrites…</p>
				{:else if planError}
					<p class="text-xs text-red-400">{planError}</p>
				{:else if plan}
					{#if plan.edits.length === 0}
						<p class="text-xs text-on-surface-variant opacity-50">No link rewrites required.</p>
					{:else}
						<div class="mb-1 text-xs font-medium text-on-surface-variant opacity-60">
							{plan.edits.length}
							{plan.edits.length === 1 ? 'link' : 'links'} will be updated across
							{editsByFile.length}
							{editsByFile.length === 1 ? 'file' : 'files'}:
						</div>
						<div class="max-h-48 overflow-y-auto rounded-lg border border-outline-variant/15 bg-surface-container-low">
							{#each editsByFile as { filePath, edits }}
								<div class="border-b border-outline-variant/10 px-3 py-2 last:border-b-0">
									<div class="mb-1.5 truncate text-xs font-medium text-on-surface opacity-70">
										{filePath}
									</div>
									{#each edits as edit}
										<div class="mb-1 last:mb-0">
											<div class="font-mono text-xs text-red-400/80">
												− {edit.before}
											</div>
											<div class="font-mono text-xs text-green-400/80">
												+ {edit.after}
											</div>
										</div>
									{/each}
								</div>
							{/each}
						</div>
					{/if}
				{/if}

				{#if applyError}
					<p class="mt-2 text-xs text-red-400">{applyError}</p>
				{/if}
			</div>

			<!-- Footer -->
			<div class="flex items-center justify-end gap-2 border-t border-outline-variant/15 px-5 py-3">
				<button
					type="button"
					onclick={close}
					class="rounded-lg px-3 py-1.5 text-xs text-on-surface-variant opacity-60 transition-opacity hover:opacity-100"
				>
					Cancel
				</button>
				<button
					type="button"
					onclick={() => void applyRename()}
					disabled={!plan || applying || !!planError || !newRelPath || newRelPath === oldRelPath}
					class="rounded-lg bg-primary px-4 py-1.5 text-xs font-medium text-on-primary transition-opacity disabled:opacity-30 hover:opacity-90"
				>
					{applying ? 'Renaming…' : 'Rename'}
				</button>
			</div>
		</div>
	</div>
{/if}
