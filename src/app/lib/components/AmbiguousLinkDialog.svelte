<script lang="ts">
	/**
	 * AmbiguousLinkDialog — disambiguation picker for wiki-links that resolve
	 * to multiple vault notes.
	 *
	 * Shows a list of candidate paths; clicking one opens it in the active pane.
	 */
	import { workspaceStore } from '$lib/stores/workspace.svelte';
	import { vaultStore } from '$lib/stores/vault.svelte';

	interface Props {
		open: boolean;
		target: string;
		paths: string[];
		onClose?: () => void;
	}

	let { open = $bindable(), target, paths, onClose }: Props = $props();

	function pick(relPath: string) {
		const vaultRoot = vaultStore.path;
		const absPath = vaultRoot ? `${vaultRoot}/${relPath}` : relPath;
		workspaceStore.openTab(absPath);
		open = false;
		onClose?.();
	}

	function close() {
		open = false;
		onClose?.();
	}

	function onKeydown(e: KeyboardEvent) {
		if (e.key === 'Escape') close();
	}

	/** Strip the .md extension for a friendlier display name. */
	function displayName(relPath: string): string {
		const base = relPath.split('/').pop() ?? relPath;
		return base.endsWith('.md') ? base.slice(0, -3) : base;
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
			class="w-[400px] max-w-[90vw] rounded-xl border border-outline-variant/20 bg-surface-container shadow-2xl"
			role="dialog"
			aria-modal="true"
			aria-label="Ambiguous link"
			tabindex="-1"
			onkeydown={onKeydown}
		>
			<!-- Header -->
			<div class="border-b border-outline-variant/15 px-5 py-4">
				<h2 class="text-sm font-medium text-on-surface">Ambiguous link</h2>
				<p class="mt-0.5 text-xs text-on-surface-variant opacity-50">
					<span class="font-mono">[[{target}]]</span> matches multiple notes, so Tektite did not guess. Choose one to open:
				</p>
			</div>

			<!-- Candidate list -->
			<div class="max-h-64 overflow-y-auto py-1">
				{#each paths as relPath (relPath)}
					<button
						type="button"
						onclick={() => pick(relPath)}
						class="w-full border-none bg-transparent px-5 py-2.5 text-left hover:bg-surface-container-low focus:bg-surface-container-low focus:outline-none"
					>
						<div class="text-sm font-medium text-primary">{displayName(relPath)}</div>
						<div class="mt-0.5 truncate font-mono text-xs text-on-surface-variant opacity-50">{relPath}</div>
					</button>
				{/each}
			</div>

			<!-- Footer -->
			<div class="flex justify-end border-t border-outline-variant/15 px-5 py-3">
				<button
					type="button"
					onclick={close}
					class="rounded-lg px-3 py-1.5 text-xs text-on-surface-variant opacity-60 transition-opacity hover:opacity-100"
				>
					Cancel
				</button>
			</div>
		</div>
	</div>
{/if}
