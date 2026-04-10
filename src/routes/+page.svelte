<script lang="ts">
	import PaneLayout from '$lib/components/PaneLayout.svelte';
	import { workspaceStore } from '$lib/stores/workspace.svelte';
</script>

<PaneLayout layout={workspaceStore.paneTree} />
