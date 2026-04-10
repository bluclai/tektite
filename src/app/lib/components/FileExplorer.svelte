<script lang="ts">
    /**
     * FileExplorer — Files sidebar panel.
     *
     * Renders the vault directory tree, opens markdown files into the active
     * tab on click, and provides inline new-file / new-folder / delete actions.
     * Phase 9: adds rename action that opens the RenameDialog.
     */
    import { ChevronRight, File, FileText, FolderClosed, FolderOpen } from 'lucide-svelte';
    import { filesStore, type TreeEntry } from '$lib/stores/files.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import RenameDialog from '$lib/components/RenameDialog.svelte';

    // ---------------------------------------------------------------------------
    // Collapse state
    // ---------------------------------------------------------------------------

    // Vault-relative paths of directories that are currently collapsed.
    let collapsed = $state<Set<string>>(new Set());

    function toggleDir(path: string) {
        const next = new Set(collapsed);
        if (next.has(path)) { next.delete(path); } else { next.add(path); }
        collapsed = next;
    }

    // ---------------------------------------------------------------------------
    // Pending create (inline input)
    // ---------------------------------------------------------------------------

    let pendingCreate = $state<{ parentPath: string; type: 'file' | 'folder' } | null>(null);
    let pendingCreateName = $state('');
    let createInputEl = $state<HTMLInputElement | null>(null);

    function startCreate(parentPath: string, type: 'file' | 'folder', e: MouseEvent) {
        e.stopPropagation();
        pendingCreate = { parentPath, type };
        pendingCreateName = '';
        setTimeout(() => createInputEl?.focus(), 0);
    }

    async function commitCreate() {
        if (!pendingCreate || !pendingCreateName.trim()) {
            pendingCreate = null;
            return;
        }
        const { parentPath, type } = pendingCreate;
        const name = pendingCreateName.trim();
        const relPath = parentPath ? `${parentPath}/${name}` : name;
        pendingCreate = null;
        pendingCreateName = '';
        if (type === 'file') {
            await filesStore.createFile(relPath);
        } else {
            await filesStore.createFolder(relPath);
        }
    }

    function cancelCreate() {
        pendingCreate = null;
        pendingCreateName = '';
    }

    function onCreateKeydown(e: KeyboardEvent) {
        if (e.key === 'Enter') { void commitCreate(); }
        else if (e.key === 'Escape') { cancelCreate(); }
    }

    // ---------------------------------------------------------------------------
    // File actions
    // ---------------------------------------------------------------------------

    function openFile(entry: TreeEntry) {
        if (entry.is_dir) return;
        workspaceStore.openTab(entry.path);
    }

    async function deleteEntry(entry: TreeEntry, e: MouseEvent) {
        e.stopPropagation();
        await filesStore.delete(entry.path);
    }

    function isMarkdown(entry: TreeEntry) {
        return !entry.is_dir && entry.name.endsWith('.md');
    }

    // ---------------------------------------------------------------------------
    // Rename dialog state (Phase 9)
    // ---------------------------------------------------------------------------

    let renameDialogOpen = $state(false);
    let renameTarget = $state<TreeEntry | null>(null);

    function startRename(entry: TreeEntry, e: MouseEvent) {
        e.stopPropagation();
        renameTarget = entry;
        renameDialogOpen = true;
    }
</script>

<!-- -----------------------------------------------------------------------
     Panel toolbar
     ----------------------------------------------------------------------- -->
<div class="flex h-full flex-col overflow-hidden">
    <div class="flex h-8 shrink-0 items-center gap-0.5 px-2">
        <span class="flex-1"></span>

        <button
            class="flex h-6 w-6 items-center justify-center rounded text-on-surface-variant opacity-50 transition-opacity duration-150 hover:opacity-100 hover:bg-surface-container-high"
            title="New file"
            onclick={(e) => startCreate('', 'file', e)}
            aria-label="New file"
        >
            <!-- document + plus icon -->
            <svg width="13" height="13" viewBox="0 0 13 13" fill="none" aria-hidden="true">
                <path d="M2 2a1 1 0 0 1 1-1h4.5L10 3.5V11a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V2Z" stroke="currentColor" stroke-width="1.2"/>
                <path d="M7 1v3h3" stroke="currentColor" stroke-width="1.2"/>
                <line x1="6.5" y1="6" x2="6.5" y2="9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
                <line x1="5" y1="7.5" x2="8" y2="7.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            </svg>
        </button>

        <button
            class="flex h-6 w-6 items-center justify-center rounded text-on-surface-variant opacity-50 transition-opacity duration-150 hover:opacity-100 hover:bg-surface-container-high"
            title="New folder"
            onclick={(e) => startCreate('', 'folder', e)}
            aria-label="New folder"
        >
            <!-- folder + plus icon -->
            <svg width="13" height="13" viewBox="0 0 13 13" fill="none" aria-hidden="true">
                <path d="M1 3.5C1 3 1.4 2.5 2 2.5h3l1 1.5h5.5c.6 0 1 .5 1 1V10c0 .5-.4 1-1 1H2c-.6 0-1-.5-1-1V3.5Z" stroke="currentColor" stroke-width="1.2"/>
                <line x1="7" y1="6" x2="7" y2="9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
                <line x1="5.5" y1="7.5" x2="8.5" y2="7.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            </svg>
        </button>
    </div>

    <!-- Root-level pending create input -->
    {#if pendingCreate?.parentPath === ''}
        <div class="px-2 pb-0.5">
            <input
                bind:this={createInputEl}
                bind:value={pendingCreateName}
                onkeydown={onCreateKeydown}
                onblur={cancelCreate}
                placeholder={pendingCreate.type === 'file' ? 'filename.md' : 'folder name'}
                class="w-full rounded bg-surface-container-high px-2 py-0.5 text-xs text-on-surface outline-none ring-1 ring-primary/50 focus:ring-primary"
            />
        </div>
    {/if}

    <!-- Tree -->
    <div class="flex-1 overflow-y-auto py-0.5" role="tree" aria-label="Vault files">
        {#if filesStore.loading && filesStore.tree.length === 0}
            <p class="px-4 py-2 text-xs text-on-surface-variant opacity-40">Loading…</p>
        {:else if filesStore.tree.length === 0}
            <p class="px-4 py-2 text-xs text-on-surface-variant opacity-40">Empty vault</p>
        {:else}
            {#each filesStore.tree as entry (entry.path)}
                {@render treeNode(entry, 0)}
            {/each}
        {/if}
    </div>
</div>

<!-- -----------------------------------------------------------------------
     Recursive tree node snippet
     ----------------------------------------------------------------------- -->
{#snippet treeNode(entry: TreeEntry, depth: number)}
    {@const isCollapsed = collapsed.has(entry.path)}
    {@const isMd = isMarkdown(entry)}

    <div role="treeitem" aria-expanded={entry.is_dir ? !isCollapsed : undefined} aria-selected="false">
        <!-- Row -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
            class="group flex h-6 cursor-pointer select-none items-center gap-1.5 rounded px-1 text-xs
                   transition-colors duration-100 hover:bg-surface-container-high
                   {!isMd && !entry.is_dir ? 'text-on-surface-variant opacity-50' : 'text-on-surface'}"
            style="padding-left: {depth * 12 + 4}px"
            onclick={() => (entry.is_dir ? toggleDir(entry.path) : openFile(entry))}
            tabindex="0"
            role="treeitem"
            aria-selected="false"
        >
            <!-- Chevron + icon -->
            {#if entry.is_dir}
                <ChevronRight
                    size={10}
                    strokeWidth={1.8}
                    class="shrink-0 text-on-surface-variant transition-transform duration-100 {isCollapsed ? '' : 'rotate-90'}"
                    aria-hidden="true"
                />
                {#if isCollapsed}
                    <FolderClosed size={13} strokeWidth={1.4} class="shrink-0 text-on-surface-variant opacity-70" aria-hidden="true" />
                {:else}
                    <FolderOpen size={13} strokeWidth={1.4} class="shrink-0 text-on-surface-variant opacity-70" aria-hidden="true" />
                {/if}
            {:else}
                <!-- Indent spacer (aligns with dirs that have chevron) -->
                <span class="w-2.5 shrink-0" aria-hidden="true"></span>
                {#if isMd}
                    <FileText size={13} strokeWidth={1.4} class="shrink-0 text-on-surface-variant opacity-50" aria-hidden="true" />
                {:else}
                    <File size={13} strokeWidth={1.4} class="shrink-0 text-on-surface-variant opacity-30" aria-hidden="true" />
                {/if}
            {/if}

            <span class="min-w-0 flex-1 truncate">{entry.name}</span>

            <!-- Hover actions -->
            <span class="ml-auto flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity duration-100 group-hover:opacity-100">
                {#if entry.is_dir}
                    <button
                        class="flex h-4 w-4 items-center justify-center rounded text-on-surface-variant hover:text-on-surface"
                        title="New file in {entry.name}"
                        onclick={(e) => startCreate(entry.path, 'file', e)}
                        tabindex="-1"
                        aria-label="New file in {entry.name}"
                    >
                        <svg width="9" height="9" viewBox="0 0 9 9" fill="none" aria-hidden="true">
                            <line x1="4.5" y1="1" x2="4.5" y2="8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                            <line x1="1" y1="4.5" x2="8" y2="4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                        </svg>
                    </button>
                {/if}
                <!-- Rename button (pencil icon) -->
                <button
                    class="flex h-4 w-4 items-center justify-center rounded text-on-surface-variant hover:text-on-surface"
                    title="Rename {entry.name}"
                    onclick={(e) => startRename(entry, e)}
                    tabindex="-1"
                    aria-label="Rename {entry.name}"
                >
                    <svg width="8" height="8" viewBox="0 0 8 8" fill="none" aria-hidden="true">
                        <path d="M5.5 1 L7 2.5 L2.5 7 L1 7 L1 5.5 Z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>
                        <line x1="4.5" y1="2" x2="6" y2="3.5" stroke="currentColor" stroke-width="1.2"/>
                    </svg>
                </button>
                <button
                    class="flex h-4 w-4 items-center justify-center rounded text-on-surface-variant hover:text-red-400"
                    title="Delete {entry.name}"
                    onclick={(e) => deleteEntry(entry, e)}
                    tabindex="-1"
                    aria-label="Delete {entry.name}"
                >
                    <svg width="7" height="7" viewBox="0 0 7 7" fill="none" aria-hidden="true">
                        <line x1="1" y1="1" x2="6" y2="6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                        <line x1="6" y1="1" x2="1" y2="6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                    </svg>
                </button>
            </span>
        </div>

        <!-- Inline create input inside an expanded directory -->
        {#if pendingCreate?.parentPath === entry.path}
            <div style="padding-left: {(depth + 1) * 12 + 4}px" class="pr-2 pb-0.5">
                <input
                    bind:this={createInputEl}
                    bind:value={pendingCreateName}
                    onkeydown={onCreateKeydown}
                    onblur={cancelCreate}
                    placeholder={pendingCreate.type === 'file' ? 'filename.md' : 'folder name'}
                    class="w-full rounded bg-surface-container-high px-2 py-0.5 text-xs text-on-surface outline-none ring-1 ring-primary/50 focus:ring-primary"
                />
            </div>
        {/if}

        <!-- Recurse into children when expanded -->
        {#if entry.is_dir && !isCollapsed}
            {#each entry.children as child (child.path)}
                {@render treeNode(child, depth + 1)}
            {/each}
        {/if}
    </div>
{/snippet}

<!-- Rename dialog (Phase 9) — rendered outside the tree so it overlays the full app -->
{#if renameTarget}
    <RenameDialog
        bind:open={renameDialogOpen}
        oldRelPath={renameTarget.path}
        vaultRoot={vaultStore.path ?? ''}
        onRenamed={() => { renameTarget = null; }}
        onClose={() => { renameTarget = null; }}
    />
{/if}
