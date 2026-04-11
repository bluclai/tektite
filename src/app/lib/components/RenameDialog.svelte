<script lang="ts">
	import { invoke } from '@tauri-apps/api/core';
	import { filesStore } from '$lib/stores/files.svelte';

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

	interface RenameResult {
		old_path: string;
		new_path: string;
		changed_paths: string[];
	}

	interface Props {
		open: boolean;
		oldRelPath: string;
		onRenamed?: (result: RenameResult) => void;
		onClose?: () => void;
	}

	let { open = $bindable(), oldRelPath, onRenamed, onClose }: Props = $props();

	let newName = $state('');
	let plan = $state<RenamePlan | null>(null);
	let planning = $state(false);
	let applying = $state(false);
	let planError = $state<string | null>(null);
	let applyError = $state<string | null>(null);
	let lastPlannedTarget = $state('');
	let previewRequestId = 0;
	let planTimer: ReturnType<typeof setTimeout> | null = null;

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

	let trimmedName = $derived(newName.trim());
	let newRelPath = $derived.by(() => {
		if (!trimmedName) return '';
		const parts = oldRelPath.split('/');
		const oldBasename = parts[parts.length - 1];
		const isMarkdown = oldBasename.endsWith('.md');
		const newBasename = isMarkdown
			? trimmedName.endsWith('.md')
				? trimmedName
				: `${trimmedName}.md`
			: trimmedName;
		return [...parts.slice(0, -1), newBasename].join('/');
	});
	let previewReady = $derived(plan !== null && lastPlannedTarget === newRelPath);
	let canApply = $derived(
		previewReady && !planning && !applying && !planError && newRelPath !== oldRelPath,
	);

	$effect(() => {
		if (!open) {
			if (planTimer) {
				clearTimeout(planTimer);
				planTimer = null;
			}
			planning = false;
			return;
		}

		const basename = oldRelPath.split('/').pop() ?? oldRelPath;
		newName = basename.endsWith('.md') ? basename.slice(0, -3) : basename;
		plan = null;
		planning = false;
		planError = null;
		applyError = null;
		lastPlannedTarget = '';
	});

	$effect(() => {
		const target = newRelPath;
		plan = null;
		planError = null;
		lastPlannedTarget = '';

		if (planTimer) {
			clearTimeout(planTimer);
			planTimer = null;
		}

		if (!open || !target || target === oldRelPath) {
			planning = false;
			return;
		}

		const requestId = ++previewRequestId;
		planTimer = setTimeout(async () => {
			planning = true;
			try {
				const nextPlan = await invoke<RenamePlan>('vault_plan_rename', {
					oldPath: oldRelPath,
					newPath: target,
				});
				if (requestId !== previewRequestId || !open) return;
				plan = nextPlan;
				lastPlannedTarget = target;
			} catch (error) {
				if (requestId !== previewRequestId || !open) return;
				planError = error instanceof Error ? error.message : String(error);
			} finally {
				if (requestId === previewRequestId) {
					planning = false;
				}
			}
		}, 250);
	});

	async function applyRename() {
		if (!plan || !previewReady) return;
		applying = true;
		applyError = null;
		try {
			const result = await invoke<RenameResult>('vault_apply_rename', { plan });
			await filesStore.refresh();
			onRenamed?.(result);
			open = false;
		} catch (error) {
			applyError = error instanceof Error ? error.message : String(error);
		} finally {
			applying = false;
		}
	}

	function close() {
		open = false;
		onClose?.();
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape') close();
		if (event.key === 'Enter' && canApply) void applyRename();
	}
</script>

{#if open}
	<!-- svelte-ignore a11y_click_events_have_key_events -->
	<div
		class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm"
		role="presentation"
		onclick={(event) => {
			if (event.target === event.currentTarget) close();
		}}
	>
		<div
			class="relative w-[560px] max-w-[90vw] rounded-xl border border-outline-variant/20 bg-surface-container shadow-2xl"
			role="dialog"
			aria-modal="true"
			aria-label="Rename file"
			tabindex="-1"
			onkeydown={onKeydown}
		>
			<div class="border-b border-outline-variant/15 px-5 py-4">
				<h2 class="text-sm font-medium text-on-surface">Rename file</h2>
				<p class="mt-0.5 truncate text-xs text-on-surface-variant opacity-50">{oldRelPath}</p>
			</div>

			<div class="px-5 py-4">
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

				{#if newRelPath && newRelPath !== oldRelPath}
					<div class="mb-3 rounded-lg bg-surface-container-low p-3">
						<div class="flex items-center gap-2 text-xs text-on-surface-variant opacity-60">
							<span class="line-through">{oldRelPath}</span>
							<span>→</span>
							<span class="text-primary">{newRelPath}</span>
						</div>
					</div>
				{/if}

				{#if !trimmedName}
					<p class="text-xs text-on-surface-variant opacity-45">Enter a new name to preview the rename.</p>
				{:else if newRelPath === oldRelPath}
					<p class="text-xs text-on-surface-variant opacity-45">Choose a different name to preview the rename.</p>
				{:else if planning}
					<p class="text-xs text-on-surface-variant opacity-45">Checking the rename and computing affected link edits…</p>
				{:else if planError}
					<p class="rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2 text-xs text-red-300">{planError}</p>
				{:else if plan}
					<div class="space-y-3">
						<div class="rounded-lg border border-outline-variant/15 bg-surface-container-low px-3 py-2 text-xs text-on-surface-variant opacity-75">
							{#if plan.edits.length === 0}
								Preview ready. No link rewrites are required.
							{:else}
								Preview ready. {plan.edits.length} {plan.edits.length === 1 ? 'link edit' : 'link edits'} will be applied across {editsByFile.length} {editsByFile.length === 1 ? 'file' : 'files'}.
							{/if}
						</div>

						{#if plan.edits.length > 0}
							<div class="max-h-56 overflow-y-auto rounded-lg border border-outline-variant/15 bg-surface-container-low">
								{#each editsByFile as { filePath, edits }}
									<div class="border-b border-outline-variant/10 px-3 py-2 last:border-b-0">
										<div class="mb-1.5 truncate text-xs font-medium text-on-surface opacity-70">{filePath}</div>
										{#each edits as edit}
											<div class="mb-2 rounded-md bg-surface px-2 py-1.5 last:mb-0">
												<div class="font-mono text-xs text-red-400/85">− {edit.before}</div>
												<div class="mt-1 font-mono text-xs text-green-400/85">+ {edit.after}</div>
											</div>
										{/each}
									</div>
								{/each}
							</div>
						{/if}
					</div>
				{/if}

				{#if applyError}
					<p class="mt-3 rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2 text-xs text-red-300">{applyError}</p>
				{/if}
			</div>

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
					disabled={!canApply}
					class="rounded-lg bg-primary px-4 py-1.5 text-xs font-medium text-on-primary transition-opacity hover:opacity-90 disabled:opacity-30"
				>
					{applying ? 'Renaming…' : 'Apply rename'}
				</button>
			</div>
		</div>
	</div>
{/if}
