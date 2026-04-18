<script lang="ts">
	import { filesStore } from '$lib/stores/files.svelte';
	import { workspaceStore } from '$lib/stores/workspace.svelte';

	interface Props {
		open: boolean;
		/** Vault-relative path of the file or folder to delete. */
		relPath: string;
		/** Whether the target is a directory. */
		isDir: boolean;
		onClose?: () => void;
		onDeleted?: () => void;
	}

	let { open = $bindable(), relPath, isDir, onClose, onDeleted }: Props = $props();

	let deleting = $state(false);
	let error = $state<string | null>(null);

	async function handleDelete() {
		deleting = true;
		error = null;
		try {
			await filesStore.delete(relPath);
			// Close any open tabs for this path
			if (isDir) {
				workspaceStore.closeTabsByPathPrefix(relPath);
			} else {
				workspaceStore.closeTabsByPath(relPath);
			}
			onDeleted?.();
			open = false;
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			deleting = false;
		}
	}

	function close() {
		open = false;
		onClose?.();
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
			class="relative w-[400px] max-w-[90vw] rounded-xl bg-surface-container shadow-2xl"
			role="dialog"
			aria-modal="true"
			aria-label="Delete {isDir ? 'folder' : 'file'}"
			tabindex="-1"
			onkeydown={(e) => e.key === 'Escape' && close()}
		>
			<div class="px-5 pt-5 pb-2">
				<h2 class="text-sm font-medium text-on-surface">
					Delete {isDir ? 'folder' : 'file'}
				</h2>
			</div>

			<div class="px-5 py-4">
				<p class="text-xs text-on-surface-variant">
					Are you sure you want to delete
					<span class="font-medium text-on-surface">{relPath}</span>?
					{#if isDir}
						All files inside will be permanently removed.
					{:else}
						This action cannot be undone.
					{/if}
				</p>

				{#if error}
					<p class="mt-3 rounded-lg border border-red-500/20 bg-red-500/10 px-3 py-2 text-xs text-red-300">
						{error}
					</p>
				{/if}
			</div>

			<div class="flex items-center justify-end gap-2 px-5 pt-2 pb-4">
				<button
					type="button"
					onclick={close}
					disabled={deleting}
					class="rounded-lg px-3 py-1.5 text-xs text-on-surface-variant opacity-60 transition-opacity hover:opacity-100 disabled:opacity-30"
				>
					Cancel
				</button>
				<button
					type="button"
					onclick={() => void handleDelete()}
					disabled={deleting}
					class="rounded-lg bg-red-500/90 px-4 py-1.5 text-xs font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-30"
				>
					{deleting ? 'Deleting…' : 'Delete'}
				</button>
			</div>
		</div>
	</div>
{/if}