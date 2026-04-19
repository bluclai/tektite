<script lang="ts">
	import { X } from 'lucide-svelte';
	import { tick } from 'svelte';

	interface Props {
		query: string;
		matchCount: number;
		matchIndex: number;
		onQueryChange: (q: string) => void;
		onEnter: () => void;
		onNext: () => void;
		onPrev: () => void;
		onClose: () => void;
	}

	let { query, matchCount, matchIndex, onQueryChange, onEnter, onNext, onPrev, onClose }: Props =
		$props();

	let inputEl = $state<HTMLInputElement | null>(null);

	$effect(() => {
		void inputEl;
		if (inputEl) {
			tick().then(() => {
				inputEl?.focus();
				inputEl?.select();
			});
		}
	});

	function handleKey(e: KeyboardEvent) {
		if (e.key === 'Escape') {
			e.preventDefault();
			onClose();
		} else if (e.key === 'Enter') {
			e.preventDefault();
			onEnter();
		} else if (e.key === 'ArrowDown') {
			e.preventDefault();
			onNext();
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			onPrev();
		}
	}
</script>

<div
	class="pointer-events-auto absolute left-1/2 top-3 z-30 flex min-w-[280px] -translate-x-1/2 items-center gap-2 rounded-md border border-outline-variant/20 bg-surface-container/95 px-2 py-1.5 text-[11px] text-on-surface shadow-lg backdrop-blur"
	role="search"
>
	<input
		bind:this={inputEl}
		type="text"
		placeholder="Find nodes — tag:foo path:bar/ text"
		value={query}
		oninput={(e) => onQueryChange((e.target as HTMLInputElement).value)}
		onkeydown={handleKey}
		class="flex-1 bg-transparent text-[11px] text-on-surface placeholder:opacity-40 focus:outline-none"
		aria-label="Find nodes"
	/>
	{#if query.trim().length > 0}
		<span class="whitespace-nowrap text-[10px] opacity-60">
			{matchCount === 0 ? 'no matches' : `${matchIndex + 1} / ${matchCount}`}
		</span>
	{/if}
	<button
		type="button"
		class="rounded p-0.5 opacity-60 hover:opacity-100"
		onclick={onClose}
		aria-label="Close find"
	>
		<X size={12} />
	</button>
</div>
