<script lang="ts">
    /**
     * FileExplorer — Files sidebar panel.
     *
     * Renders the pinned rows + vault directory tree, opens markdown files
     * into the active tab on click, and provides inline new-file / new-folder
     * actions. Context menu provides pin, rename, and delete.
     */
    import {
        ChevronRight,
        File,
        FileText,
        FolderClosed,
        FolderOpen,
        Plus,
        Pin,
    } from 'lucide-svelte';
    import { filesStore, type TreeEntry } from '$lib/stores/files.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import { pinnedStore } from '$lib/stores/pinned-notes.svelte';
    import RenameDialog from '$lib/components/RenameDialog.svelte';
    import DeleteDialog from '$lib/components/DeleteDialog.svelte';
    import {
        ContextMenu,
        ContextMenuContent,
        ContextMenuItem,
        ContextMenuSeparator,
        ContextMenuTrigger,
    } from '$lib/components/ui/context-menu';

    interface RenameResult {
        old_path: string;
        new_path: string;
        changed_paths: string[];
    }

    let collapsed = $state<Set<string>>(new Set());

    function toggleDir(path: string) {
        const next = new Set(collapsed);
        if (next.has(path)) next.delete(path);
        else next.add(path);
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

    function startCreate(parentPath: string, type: 'file' | 'folder') {
        expandDir(parentPath);
        pendingCreate = { parentPath, type };
        pendingCreateName = '';
        createError = null;
        filesStore.clearError();
        setTimeout(() => createInputEl?.focus(), 0);
    }

    async function commitCreate() {
        if (!pendingCreate) return;

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
    // Rename / delete state
    // ---------------------------------------------------------------------------

    let renameDialogOpen = $state(false);
    let renameTarget = $state<TreeEntry | null>(null);

    function startRename(entry: TreeEntry) {
        renameTarget = entry;
        renameDialogOpen = true;
    }

    function handleRename(result: RenameResult) {
        workspaceStore.renamePath(result.old_path, result.new_path);
        pinnedStore.renamePath(result.old_path, result.new_path);

        const vaultRoot = vaultStore.path;
        if (vaultRoot) {
            workspaceStore.renamePath(`${vaultRoot}/${result.old_path}`, `${vaultRoot}/${result.new_path}`);
        }

        renameDialogOpen = false;
        renameTarget = null;
    }

    let deleteDialogOpen = $state(false);
    let deleteTarget = $state<TreeEntry | null>(null);

    function startDelete(entry: TreeEntry) {
        deleteTarget = entry;
        deleteDialogOpen = true;
    }

    // ---------------------------------------------------------------------------
    // Active file + pinned rows
    // ---------------------------------------------------------------------------

    const activeRelPath = $derived.by<string | null>(() => {
        const abs = workspaceStore.activeFilePath;
        if (!abs) return null;
        const root = vaultStore.path;
        if (root && abs.startsWith(root + '/')) return abs.slice(root.length + 1);
        return abs;
    });

    function countDescendants(entries: TreeEntry[]): number {
        let n = 0;
        for (const e of entries) {
            if (e.is_dir) n += countDescendants(e.children);
            else n += 1;
        }
        return n;
    }

    function flattenPaths(entries: TreeEntry[], out: Map<string, TreeEntry> = new Map()) {
        for (const e of entries) {
            if (!e.is_dir) out.set(e.path, e);
            else flattenPaths(e.children, out);
        }
        return out;
    }

    const pathIndex = $derived(flattenPaths(filesStore.tree));

    const pinnedEntries = $derived(
        pinnedStore.paths
            .map((p) => pathIndex.get(p))
            .filter((e): e is TreeEntry => !!e),
    );
</script>

<div class="flex h-full flex-col overflow-hidden pt-2">
    <!-- Pinned section -->
    {#if pinnedEntries.length > 0}
        <div class="flex h-6 shrink-0 items-center gap-2 px-4">
            <span class="eyebrow">Pinned</span>
            <span class="font-sans text-[10.5px] text-text-faint">{pinnedEntries.length}</span>
        </div>
        <div class="flex shrink-0 flex-col gap-0.5 px-2 pt-1 pb-2">
            {#each pinnedEntries as entry (entry.path)}
                {@render fileRow(entry, 0, true)}
            {/each}
        </div>
    {/if}

    <!-- Vault section -->
    <div class="flex h-6 shrink-0 items-center gap-2 px-4">
        <span class="eyebrow">Vault</span>
        <span class="flex-1"></span>
        <button
            class="flex h-5 w-5 cursor-pointer items-center justify-center rounded border-none bg-transparent text-text-ghost transition-colors duration-200 ease-out hover:text-text-secondary"
            onclick={(e) => { e.stopPropagation(); startCreate('', 'file'); }}
            aria-label="New note"
            title="New note"
        >
            <Plus size={12} strokeWidth={1.75} />
        </button>
    </div>

    {#if pendingCreate?.parentPath === ''}
        <div class="px-3 pt-1 pb-1">
            <input
                bind:this={createInputEl}
                bind:value={pendingCreateName}
                onkeydown={onCreateKeydown}
                onblur={cancelCreate}
                placeholder={pendingCreate.type === 'file' ? 'filename.md' : 'folder name'}
                class="w-full rounded-[6px] px-2 py-1 font-sans text-[12px] text-text-primary outline-none ring-1 ring-primary/50 focus:ring-primary"
                style="background-color: rgba(255,255,255,0.04);"
            />
            {#if createError}
                <p class="pt-1 font-sans text-[10.5px] text-red-400">{createError}</p>
            {/if}
        </div>
    {/if}

    <div class="flex-1 overflow-y-auto px-2 pb-2" role="tree" aria-label="Vault files">
        {#if filesStore.error}
            <p class="px-2 py-2 font-sans text-[11px] text-red-400">{filesStore.error}</p>
        {/if}

        {#if filesStore.loading && filesStore.tree.length === 0}
            <p class="px-2 py-2 font-sans text-[11px] text-text-ghost">Loading…</p>
        {:else if filesStore.tree.length === 0}
            <p class="px-2 py-2 font-sans text-[11px] text-text-ghost">Empty vault</p>
        {:else}
            {#each filesStore.tree as entry (entry.path)}
                {@render treeNode(entry, 0)}
            {/each}
        {/if}
    </div>
</div>

{#snippet fileRow(entry: TreeEntry, depth: number, inPinned: boolean)}
    {@const isActive = activeRelPath === entry.path}
    {@const isMd = entry.is_markdown}
    <ContextMenu>
        <ContextMenuTrigger>
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
                role="treeitem"
                aria-selected={isActive}
                tabindex="0"
                class="group relative flex h-[26px] cursor-pointer select-none items-center gap-1.5 rounded-[6px] pr-2 font-sans text-[13px] transition-colors duration-200 ease-out {isActive ? 'text-text-primary' : !isMd ? 'text-text-ghost' : 'text-text-secondary hover:bg-[rgba(255,255,255,0.03)]'}"
                style="padding-left: {24 + depth * 12}px; {isActive ? 'background: linear-gradient(90deg, rgba(189,194,255,0.08) 0%, rgba(189,194,255,0.02) 100%);' : ''}"
                onclick={() => openFile(entry)}
            >
                {#if isActive}
                    <span
                        class="pointer-events-none absolute top-1/2 left-0 h-4 w-[2px] -translate-y-1/2 rounded-r-full"
                        style="background-color: var(--color-primary); box-shadow: 0 0 6px rgba(189,194,255,0.55);"
                        aria-hidden="true"
                    ></span>
                {/if}
                {#if isMd}
                    <FileText size={13} strokeWidth={1.5} class="shrink-0 text-text-muted" aria-hidden="true" />
                {:else}
                    <File size={13} strokeWidth={1.5} class="shrink-0 text-text-ghost" aria-hidden="true" />
                {/if}
                <span class="min-w-0 flex-1 truncate">{entry.name.replace(/\.md$/i, '')}</span>
            </div>
        </ContextMenuTrigger>
        <ContextMenuContent>
            {#if isMd}
                <ContextMenuItem onselect={() => pinnedStore.toggle(entry.path)}>
                    {pinnedStore.has(entry.path) ? 'Unpin' : 'Pin'}
                </ContextMenuItem>
                <ContextMenuSeparator />
            {/if}
            <ContextMenuItem onselect={() => startRename(entry)}>Rename</ContextMenuItem>
            <ContextMenuItem variant="destructive" onselect={() => startDelete(entry)}>
                Delete
            </ContextMenuItem>
        </ContextMenuContent>
    </ContextMenu>
{/snippet}

{#snippet treeNode(entry: TreeEntry, depth: number)}
    {@const isCollapsed = collapsed.has(entry.path)}
    {@const isMd = entry.is_markdown}
    {@const openable = canOpen(entry)}
    {@const isActive = !entry.is_dir && activeRelPath === entry.path}

    <div role="treeitem" aria-expanded={entry.is_dir ? !isCollapsed : undefined} aria-selected={isActive}>
        <ContextMenu>
            <ContextMenuTrigger>
                <!-- svelte-ignore a11y_click_events_have_key_events -->
                <div
                    class="group relative flex h-[26px] select-none items-center gap-1.5 rounded-[6px] pr-2 font-sans text-[13px] transition-colors duration-200 ease-out
                        {openable ? 'cursor-pointer' : 'cursor-default'}
                        {isActive
                            ? 'text-text-primary'
                            : entry.is_dir
                                ? 'text-text-secondary hover:bg-[rgba(255,255,255,0.03)]'
                                : !isMd
                                    ? 'text-text-ghost'
                                    : 'text-text-secondary hover:bg-[rgba(255,255,255,0.03)]'}"
                    style="padding-left: {entry.is_dir ? 8 + depth * 12 : 24 + depth * 12}px; {isActive ? 'background: linear-gradient(90deg, rgba(189,194,255,0.08) 0%, rgba(189,194,255,0.02) 100%);' : ''}"
                    onclick={() => (entry.is_dir ? toggleDir(entry.path) : openFile(entry))}
                    tabindex={openable ? 0 : undefined}
                    role="treeitem"
                    aria-selected={isActive}
                >
                    {#if isActive}
                        <span
                            class="pointer-events-none absolute top-1/2 left-0 h-4 w-[2px] -translate-y-1/2 rounded-r-full"
                            style="background-color: var(--color-primary); box-shadow: 0 0 6px rgba(189,194,255,0.55);"
                            aria-hidden="true"
                        ></span>
                    {/if}

                    {#if entry.is_dir}
                        <ChevronRight
                            size={11}
                            strokeWidth={1.75}
                            class="shrink-0 text-text-muted transition-transform duration-200 {isCollapsed ? '' : 'rotate-90'}"
                            aria-hidden="true"
                        />
                        {#if isCollapsed}
                            <FolderClosed size={13} strokeWidth={1.5} class="shrink-0 text-text-muted" aria-hidden="true" />
                        {:else}
                            <FolderOpen size={13} strokeWidth={1.5} class="shrink-0 text-text-muted" aria-hidden="true" />
                        {/if}
                        <span class="min-w-0 flex-1 truncate font-medium">{entry.name}</span>
                        <span class="ml-1 shrink-0 font-sans text-[10.5px] text-text-faint">
                            {countDescendants(entry.children)}
                        </span>
                    {:else}
                        {#if isMd}
                            <FileText size={13} strokeWidth={1.5} class="shrink-0 text-text-muted" aria-hidden="true" />
                        {:else}
                            <File size={13} strokeWidth={1.5} class="shrink-0 text-text-ghost" aria-hidden="true" />
                        {/if}
                        <span class="min-w-0 flex-1 truncate">{entry.name.replace(/\.md$/i, '')}</span>
                        {#if isMd && pinnedStore.has(entry.path)}
                            <Pin size={10} strokeWidth={1.75} class="shrink-0 text-text-ghost" aria-hidden="true" />
                        {/if}
                    {/if}

                    <span class="ml-auto flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity duration-200 group-hover:opacity-100">
                        {#if entry.is_dir}
                            <button
                                class="flex h-4 w-4 items-center justify-center rounded border-none bg-transparent text-text-muted hover:text-text-secondary"
                                title="New file in {entry.name}"
                                onclick={(e) => { e.stopPropagation(); startCreate(entry.path, 'file'); }}
                                tabindex="-1"
                                aria-label="New file in {entry.name}"
                            >
                                <Plus size={10} strokeWidth={1.75} />
                            </button>
                        {/if}
                    </span>
                </div>
            </ContextMenuTrigger>
            <ContextMenuContent>
                {#if entry.is_dir}
                    <ContextMenuItem onselect={() => startCreate(entry.path, 'file')}>New file</ContextMenuItem>
                    <ContextMenuItem onselect={() => startCreate(entry.path, 'folder')}>New folder</ContextMenuItem>
                    <ContextMenuSeparator />
                {:else if isMd}
                    <ContextMenuItem onselect={() => pinnedStore.toggle(entry.path)}>
                        {pinnedStore.has(entry.path) ? 'Unpin' : 'Pin'}
                    </ContextMenuItem>
                    <ContextMenuSeparator />
                {/if}
                <ContextMenuItem onselect={() => startRename(entry)}>Rename</ContextMenuItem>
                <ContextMenuItem variant="destructive" onselect={() => startDelete(entry)}>Delete</ContextMenuItem>
            </ContextMenuContent>
        </ContextMenu>

        {#if pendingCreate?.parentPath === entry.path}
            <div style="padding-left: {(depth + 1) * 12 + 24}px" class="pr-2 pt-1 pb-1">
                <input
                    bind:this={createInputEl}
                    bind:value={pendingCreateName}
                    onkeydown={onCreateKeydown}
                    onblur={cancelCreate}
                    placeholder={pendingCreate.type === 'file' ? 'filename.md' : 'folder name'}
                    class="w-full rounded-[6px] px-2 py-1 font-sans text-[12px] text-text-primary outline-none ring-1 ring-primary/50 focus:ring-primary"
                    style="background-color: rgba(255,255,255,0.04);"
                />
                {#if createError}
                    <p class="pt-1 font-sans text-[10.5px] text-red-400">{createError}</p>
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

{#if deleteTarget}
    <DeleteDialog
        bind:open={deleteDialogOpen}
        relPath={deleteTarget.path}
        isDir={deleteTarget.is_dir}
        onClose={() => {
            deleteDialogOpen = false;
            deleteTarget = null;
        }}
    />
{/if}
