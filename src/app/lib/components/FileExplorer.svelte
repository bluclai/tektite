<script lang="ts">
    /**
     * FileExplorer — Files sidebar panel.
     *
     * Renders the vault directory tree, opens markdown files into the active
     * tab on click, and provides inline new-file / new-folder actions.
     * Delete is intentionally hidden for v0.1 because it is not yet trustworthy
     * enough to keep in the daily-use path.
     */
    import { ChevronRight, File, FileText, FolderClosed, FolderOpen } from 'lucide-svelte';
    import { filesStore, type TreeEntry } from '$lib/stores/files.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import RenameDialog from '$lib/components/RenameDialog.svelte';

    interface RenameResult {
        old_path: string;
        new_path: string;
        changed_paths: string[];
    }

    // ---------------------------------------------------------------------------
    // Collapse state
    // ---------------------------------------------------------------------------

    let collapsed = $state<Set<string>>(new Set());

    function toggleDir(path: string) {
        const next = new Set(collapsed);
        if (next.has(path)) {
            next.delete(path);
        } else {
            next.add(path);
        }
        collapsed = next;
    }

    function expandDir(path: string) {
        if (!path) return;
        const next = new Set(collapsed);
        next.delete(path);
        collapsed = next;
    }

    // ---------------------------------------------------------------------------
    // Pending create (inline input)
    // ---------------------------------------------------------------------------

    let pendingCreate = $state<{ parentPath: string; type: 'file' | 'folder' } | null>(null);
    let pendingCreateName = $state('');
    let createError = $state<string | null>(null);
    let createInputEl = $state<HTMLInputElement | null>(null);

    function normalizeCreateName(rawName: string, type: 'file' | 'folder'): string | null {
        const trimmed = rawName.trim();
        if (!trimmed) {
            createError = null;
            return null;
        }
        if (trimmed === '.' || trimmed === '..') {
            createError = 'Name cannot be . or ..';
            return null;
        }
        if (trimmed.includes('/') || trimmed.includes('\\')) {
            createError = 'Use the current folder instead of typing path separators';
            return null;
        }

        if (type === 'file' && !/\.[^./]+$/.test(trimmed)) {
            return `${trimmed}.md`;
        }

        return trimmed;
    }

    function startCreate(parentPath: string, type: 'file' | 'folder', e: MouseEvent) {
        e.stopPropagation();
        expandDir(parentPath);
        pendingCreate = { parentPath, type };
        pendingCreateName = '';
        createError = null;
        filesStore.clearError();
        setTimeout(() => createInputEl?.focus(), 0);
    }

    async function commitCreate() {
        if (!pendingCreate) {
            return;
        }

        const normalizedName = normalizeCreateName(pendingCreateName, pendingCreate.type);
        if (!normalizedName) {
            if (!pendingCreateName.trim()) {
                pendingCreate = null;
                createError = null;
            }
            return;
        }

        const { parentPath, type } = pendingCreate;
        const relPath = parentPath ? `${parentPath}/${normalizedName}` : normalizedName;

        try {
            filesStore.clearError();
            if (type === 'file') {
                await filesStore.createFile(relPath);
                workspaceStore.openTab(relPath);
            } else {
                await filesStore.createFolder(relPath);
                expandDir(relPath);
            }
            pendingCreate = null;
            pendingCreateName = '';
            createError = null;
        } catch (error) {
            createError = error instanceof Error ? error.message : String(error);
            setTimeout(() => createInputEl?.focus(), 0);
        }
    }

    function cancelCreate() {
        pendingCreate = null;
        pendingCreateName = '';
        createError = null;
    }

    function onCreateKeydown(e: KeyboardEvent) {
        if (e.key === 'Enter') {
            e.preventDefault();
            void commitCreate();
        } else if (e.key === 'Escape') {
            e.preventDefault();
            cancelCreate();
        }
    }

    // ---------------------------------------------------------------------------
    // File actions
    // ---------------------------------------------------------------------------

    function openFile(entry: TreeEntry) {
        if (entry.is_dir || !entry.is_markdown) return;
        workspaceStore.openTab(entry.path);
    }

    function canOpen(entry: TreeEntry) {
        return entry.is_dir || entry.is_markdown;
    }

    // ---------------------------------------------------------------------------
    // Rename dialog state
    // ---------------------------------------------------------------------------

    let renameDialogOpen = $state(false);
    let renameTarget = $state<TreeEntry | null>(null);

    function startRename(entry: TreeEntry, e: MouseEvent) {
        e.stopPropagation();
        renameTarget = entry;
        renameDialogOpen = true;
    }

    function handleRename(result: RenameResult) {
        workspaceStore.renamePath(result.old_path, result.new_path);

        const vaultRoot = vaultStore.path;
        if (vaultRoot) {
            workspaceStore.renamePath(`${vaultRoot}/${result.old_path}`, `${vaultRoot}/${result.new_path}`);
        }

        renameDialogOpen = false;
        renameTarget = null;
    }
</script>

<div class="flex h-full flex-col overflow-hidden">
    <div class="flex h-8 shrink-0 items-center gap-0.5 px-2">
        <span class="flex-1"></span>

        <button
            class="flex h-6 w-6 items-center justify-center rounded text-on-surface-variant opacity-50 transition-opacity duration-150 hover:opacity-100 hover:bg-surface-container-high"
            title="New file"
            onclick={(e) => startCreate('', 'file', e)}
            aria-label="New file"
        >
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
            <svg width="13" height="13" viewBox="0 0 13 13" fill="none" aria-hidden="true">
                <path d="M1 3.5C1 3 1.4 2.5 2 2.5h3l1 1.5h5.5c.6 0 1 .5 1 1V10c0 .5-.4 1-1 1H2c-.6 0-1-.5-1-1V3.5Z" stroke="currentColor" stroke-width="1.2"/>
                <line x1="7" y1="6" x2="7" y2="9" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
                <line x1="5.5" y1="7.5" x2="8.5" y2="7.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            </svg>
        </button>
    </div>

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
            {#if createError}
                <p class="pt-1 text-[11px] text-red-400">{createError}</p>
            {/if}
        </div>
    {/if}

    <div class="flex-1 overflow-y-auto py-0.5" role="tree" aria-label="Vault files">
        {#if filesStore.error}
            <p class="px-4 py-2 text-xs text-red-400">{filesStore.error}</p>
        {/if}

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

{#snippet treeNode(entry: TreeEntry, depth: number)}
    {@const isCollapsed = collapsed.has(entry.path)}
    {@const isMd = entry.is_markdown}
    {@const openable = canOpen(entry)}

    <div role="treeitem" aria-expanded={entry.is_dir ? !isCollapsed : undefined} aria-selected="false">
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
            class="group flex h-6 select-none items-center gap-1.5 rounded px-1 text-xs
                   transition-colors duration-100 hover:bg-surface-container-high
                   {openable ? 'cursor-pointer' : 'cursor-default'}
                   {!isMd && !entry.is_dir ? 'text-on-surface-variant opacity-45' : 'text-on-surface'}"
            style="padding-left: {depth * 12 + 4}px"
            onclick={() => (entry.is_dir ? toggleDir(entry.path) : openFile(entry))}
            tabindex={openable ? 0 : undefined}
            role="treeitem"
            aria-selected="false"
        >
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
                <span class="w-2.5 shrink-0" aria-hidden="true"></span>
                {#if isMd}
                    <FileText size={13} strokeWidth={1.4} class="shrink-0 text-on-surface-variant opacity-50" aria-hidden="true" />
                {:else}
                    <File size={13} strokeWidth={1.4} class="shrink-0 text-on-surface-variant opacity-25" aria-hidden="true" />
                {/if}
            {/if}

            <span class="min-w-0 flex-1 truncate">{entry.name}</span>

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
                    <button
                        class="flex h-4 w-4 items-center justify-center rounded text-on-surface-variant hover:text-on-surface"
                        title="New folder in {entry.name}"
                        onclick={(e) => startCreate(entry.path, 'folder', e)}
                        tabindex="-1"
                        aria-label="New folder in {entry.name}"
                    >
                        <svg width="9" height="9" viewBox="0 0 9 9" fill="none" aria-hidden="true">
                            <line x1="4.5" y1="1" x2="4.5" y2="8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                            <line x1="1" y1="4.5" x2="8" y2="4.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
                        </svg>
                    </button>
                {/if}
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
            </span>
        </div>

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
                {#if createError}
                    <p class="pt-1 text-[11px] text-red-400">{createError}</p>
                {/if}
            </div>
        {/if}

        {#if entry.is_dir && !isCollapsed}
            {#each entry.children as child (child.path)}
                {@render treeNode(child, depth + 1)}
            {/each}
        {/if}
    </div>
{/snippet}

{#if renameTarget}
    <RenameDialog
        bind:open={renameDialogOpen}
        oldRelPath={renameTarget.path}
        onRenamed={handleRename}
        onClose={() => {
            renameDialogOpen = false;
            renameTarget = null;
        }}
    />
{/if}
