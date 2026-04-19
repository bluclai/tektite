<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { listen } from '@tauri-apps/api/event';
    import { onMount } from 'svelte';
    import { ChevronDown, ChevronRight, Copy, Plus, Unlink } from 'lucide-svelte';
    import AmbiguousLinkDialog from '$lib/components/AmbiguousLinkDialog.svelte';
    import { rootNotePathForTarget, initialContentForTarget } from '$lib/editor/wiki-link-parse';
    import { filesStore } from '$lib/stores/files.svelte';
    import { editorStore } from '$lib/stores/editor.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';

    type UnresolvedTargetKind = 'unresolved' | 'ambiguous';

    interface UnresolvedTargetRow {
        target: string;
        kind: UnresolvedTargetKind;
        reference_count: number;
        sample_sources: string[];
        has_more_sources: boolean;
    }

    interface UnresolvedReport {
        rows: UnresolvedTargetRow[];
        total_count: number;
    }

    interface UnresolvedSourceRef {
        source_path: string;
        source_title: string;
        target: string;
        fragment: string | null;
        alias: string | null;
    }

    type LinkResolutionResult =
        | { kind: 'resolved'; path: string }
        | { kind: 'unresolved' }
        | { kind: 'ambiguous'; paths: string[] };

    const DEFAULT_LIMIT = 500;
    const REFRESH_DEBOUNCE_MS = 200;
    const SOURCE_LIMIT = 500;

    let report = $state<UnresolvedReport>({ rows: [], total_count: 0 });
    let loading = $state(false);
    let error = $state<string | null>(null);
    let ambiguousDialogOpen = $state(false);
    let ambiguousTarget = $state('');
    let ambiguousPaths = $state<string[]>([]);
    let expandedTargets = $state<Record<string, boolean>>({});
    let sourceRowsByTarget = $state<Record<string, UnresolvedSourceRef[]>>({});
    let sourceLoadingByTarget = $state<Record<string, boolean>>({});
    let sourceErrorByTarget = $state<Record<string, string | null>>({});
    let createLoadingByTarget = $state<Record<string, boolean>>({});

    function normalizeTarget(target: string): string {
        return target.toLowerCase();
    }

    function isExpanded(target: string): boolean {
        return expandedTargets[normalizeTarget(target)] === true;
    }

    function sourceRows(target: string): UnresolvedSourceRef[] {
        return sourceRowsByTarget[normalizeTarget(target)] ?? [];
    }

    function sourceLoading(target: string): boolean {
        return sourceLoadingByTarget[normalizeTarget(target)] === true;
    }

    function sourceError(target: string): string | null {
        return sourceErrorByTarget[normalizeTarget(target)] ?? null;
    }

    function createLoading(target: string): boolean {
        return createLoadingByTarget[normalizeTarget(target)] === true;
    }

    function formatCount(count: number): string {
        return `${count} ${count === 1 ? 'reference' : 'references'}`;
    }

    function badgeLabel(kind: UnresolvedTargetKind): string {
        return kind === 'ambiguous' ? 'Ambiguous' : 'Unresolved';
    }

    function badgeClass(kind: UnresolvedTargetKind): string {
        return kind === 'ambiguous'
            ? 'border-amber-500/20 bg-amber-500/10 text-amber-200/90'
            : 'border-sky-500/20 bg-sky-500/10 text-sky-200/90';
    }

    function overflowCount(): number {
        return Math.max(0, report.total_count - report.rows.length);
    }

    function formatLink(entry: UnresolvedSourceRef): string {
        let text = entry.target;
        if (entry.fragment) text += `#${entry.fragment}`;
        if (entry.alias) text += `|${entry.alias}`;
        return `[[${text}]]`;
    }

    function getDisplayTitle(entry: UnresolvedSourceRef): string {
        return entry.source_title || entry.source_path.split('/').pop() || entry.source_path;
    }

    function openSource(entry: UnresolvedSourceRef, e: MouseEvent | KeyboardEvent) {
        const vaultRoot = vaultStore.path;
        const absPath =
            vaultRoot && !entry.source_path.startsWith(vaultRoot)
                ? `${vaultRoot}/${entry.source_path}`
                : entry.source_path;
        const forceNew = e.metaKey || e.ctrlKey;
        workspaceStore.openTab(absPath, { forceNew });
    }

    async function fetchReport() {
        loading = true;
        error = null;

        try {
            report = await invoke<UnresolvedReport>('index_unresolved_link_report', {
                limit: DEFAULT_LIMIT,
            });

            const validTargets = new Set(report.rows.map((row) => normalizeTarget(row.target)));
            expandedTargets = Object.fromEntries(
                Object.entries(expandedTargets).filter(([key, value]) => validTargets.has(key) && value),
            );
            sourceRowsByTarget = Object.fromEntries(
                Object.entries(sourceRowsByTarget).filter(([key]) => validTargets.has(key)),
            );
            sourceLoadingByTarget = Object.fromEntries(
                Object.entries(sourceLoadingByTarget).filter(([key, value]) => validTargets.has(key) && value),
            );
            sourceErrorByTarget = Object.fromEntries(
                Object.entries(sourceErrorByTarget).filter(([key, value]) => validTargets.has(key) && value),
            );
        } catch (e) {
            error = String(e);
            report = { rows: [], total_count: 0 };
        } finally {
            loading = false;
        }
    }

    async function loadSources(target: string) {
        const key = normalizeTarget(target);
        sourceLoadingByTarget = { ...sourceLoadingByTarget, [key]: true };
        sourceErrorByTarget = { ...sourceErrorByTarget, [key]: null };

        try {
            const rows = await invoke<UnresolvedSourceRef[]>('index_unresolved_target_sources', {
                target,
                limit: SOURCE_LIMIT,
            });
            sourceRowsByTarget = { ...sourceRowsByTarget, [key]: rows };
        } catch (e) {
            sourceErrorByTarget = { ...sourceErrorByTarget, [key]: String(e) };
            sourceRowsByTarget = { ...sourceRowsByTarget, [key]: [] };
        } finally {
            sourceLoadingByTarget = { ...sourceLoadingByTarget, [key]: false };
        }
    }

    async function toggleExpanded(target: string) {
        const key = normalizeTarget(target);
        const nextExpanded = !isExpanded(target);
        expandedTargets = { ...expandedTargets, [key]: nextExpanded };

        if (nextExpanded && sourceRows(target).length === 0 && !sourceLoading(target) && !sourceError(target)) {
            await loadSources(target);
        }
    }

    async function createNote(target: string) {
        const key = normalizeTarget(target);
        const relPath = rootNotePathForTarget(target);
        const initialContent = initialContentForTarget(target);

        if (!relPath || !initialContent) {
            editorStore.setSaveState('error', {
                detail: 'Invalid target for note creation',
                target,
            });
            return;
        }

        createLoadingByTarget = { ...createLoadingByTarget, [key]: true };

        try {
            await filesStore.createFile(relPath, initialContent);
            // Fresh note from an unresolved link — commit as a new tab so the
            // current tab's content isn't displaced by the creation flow.
            workspaceStore.openTab(relPath, { forceNew: true });
            editorStore.setSaveState('saved', {
                detail: 'Created note from unresolved link',
                target: relPath,
            });
        } catch (e) {
            editorStore.setSaveState('error', {
                detail: e instanceof Error ? e.message : String(e),
                target: relPath,
            });
        } finally {
            createLoadingByTarget = { ...createLoadingByTarget, [key]: false };
        }
    }

    async function resolveAmbiguous(target: string) {
        try {
            const result = await invoke<LinkResolutionResult>('index_resolve_link', {
                target,
                sourcePath: null,
            });

            if (result.kind !== 'ambiguous') {
                editorStore.setSaveState('error', {
                    detail: 'Target is no longer ambiguous',
                    target,
                });
                return;
            }

            ambiguousTarget = target;
            ambiguousPaths = result.paths;
            ambiguousDialogOpen = true;
        } catch (e) {
            editorStore.setSaveState('error', {
                detail: e instanceof Error ? e.message : String(e),
                target,
            });
        }
    }

    async function copyLink(target: string) {
        const text = `[[${target}]]`;
        try {
            await navigator.clipboard.writeText(text);
            editorStore.setSaveState('saved', {
                detail: 'Copied wiki-link to clipboard',
                target: text,
            });
        } catch (e) {
            editorStore.setSaveState('error', {
                detail: e instanceof Error ? e.message : String(e),
                target: text,
            });
        }
    }

    onMount(() => {
        void fetchReport();

        let refreshTimer: ReturnType<typeof setTimeout> | null = null;

        const unlisten = listen('file-tree-updated', () => {
            if (refreshTimer) {
                clearTimeout(refreshTimer);
            }
            refreshTimer = setTimeout(() => {
                void fetchReport();
                refreshTimer = null;
            }, REFRESH_DEBOUNCE_MS);
        });

        return () => {
            if (refreshTimer) {
                clearTimeout(refreshTimer);
            }
            void unlisten.then((fn) => fn());
        };
    });
</script>

<div class="flex h-full flex-col overflow-hidden">
    {#if loading}
        <div class="flex h-full items-center justify-center p-6">
            <p class="text-xs text-on-surface-variant opacity-40">Loading unresolved links…</p>
        </div>
    {:else if error}
        <div class="flex h-full items-center justify-center p-6">
            <p class="text-xs text-red-400 opacity-70">{error}</p>
        </div>
    {:else if report.rows.length === 0}
        <div class="flex h-full items-center justify-center p-6">
            <div class="flex flex-col items-center gap-2 text-center">
                <Unlink size={20} strokeWidth={1.2} class="text-on-surface-variant opacity-20" />
                <p class="text-xs text-on-surface-variant opacity-30">No unresolved links found</p>
            </div>
        </div>
    {:else}
        <div class="shrink-0 px-3 pb-1 pt-2">
            <p class="text-xs text-on-surface-variant opacity-50">
                {report.total_count}
                {report.total_count === 1 ? 'target' : 'targets'} with unresolved links
            </p>
        </div>

        <div class="flex-1 overflow-y-auto divide-y divide-outline-variant/10">
            {#each report.rows as row (row.target)}
                <div class="p-3">
                    <div class="flex items-start justify-between gap-3">
                        <button
                            type="button"
                            class="flex min-w-0 flex-1 items-start gap-2 rounded border-none bg-transparent p-0 text-left hover:bg-surface-container-low/30 focus:bg-surface-container-low/30 focus:outline-none"
                            onclick={() => void toggleExpanded(row.target)}
                            aria-expanded={isExpanded(row.target)}
                        >
                            <span class="mt-0.5 shrink-0 text-on-surface-variant opacity-50">
                                {#if isExpanded(row.target)}
                                    <ChevronDown size={14} />
                                {:else}
                                    <ChevronRight size={14} />
                                {/if}
                            </span>
                            <div class="min-w-0 flex-1">
                                <div class="truncate font-mono text-xs text-primary">
                                    [[{row.target}]]
                                </div>
                                <div class="mt-1 text-xs text-on-surface-variant opacity-45">
                                    {formatCount(row.reference_count)}
                                </div>
                            </div>
                        </button>
                        <div class="flex shrink-0 items-center gap-2">
                            <button
                                type="button"
                                class="inline-flex items-center gap-1 rounded border border-outline-variant/20 bg-surface-container-low px-2 py-1 text-[10px] font-medium tracking-wide text-on-surface-variant/90"
                                onclick={() => void copyLink(row.target)}
                            >
                                <Copy size={12} />
                                Copy
                            </button>
                            {#if row.kind === 'unresolved'}
                                <button
                                    type="button"
                                    class="inline-flex items-center gap-1 rounded border border-emerald-500/20 bg-emerald-500/10 px-2 py-1 text-[10px] font-medium tracking-wide text-emerald-200/90 disabled:opacity-50"
                                    onclick={() => void createNote(row.target)}
                                    disabled={createLoading(row.target)}
                                >
                                    <Plus size={12} />
                                    {createLoading(row.target) ? 'Creating…' : 'Create note'}
                                </button>
                            {:else if row.kind === 'ambiguous'}
                                <button
                                    type="button"
                                    class="inline-flex items-center gap-1 rounded border border-amber-500/20 bg-amber-500/10 px-2 py-1 text-[10px] font-medium tracking-wide text-amber-200/90"
                                    onclick={() => void resolveAmbiguous(row.target)}
                                >
                                    Resolve
                                </button>
                            {/if}
                            <span
                                class={`shrink-0 rounded-full border px-2 py-0.5 text-[10px] font-medium tracking-wide ${badgeClass(row.kind)}`}
                            >
                                {badgeLabel(row.kind)}
                            </span>
                        </div>
                    </div>

                    {#if isExpanded(row.target)}
                        <div class="mt-3 ml-6 rounded-md bg-surface-container-low/40">
                            {#if sourceLoading(row.target)}
                                <div class="p-3 text-xs text-on-surface-variant opacity-40">
                                    Loading sources…
                                </div>
                            {:else if sourceError(row.target)}
                                <div class="p-3 text-xs text-red-400 opacity-70">
                                    {sourceError(row.target)}
                                </div>
                            {:else if sourceRows(row.target).length === 0}
                                <div class="p-3 text-xs text-on-surface-variant opacity-35">
                                    No source notes found
                                </div>
                            {:else}
                                <div class="divide-y divide-outline-variant/10">
                                    {#each sourceRows(row.target) as source (`${row.target}:${source.source_path}:${source.target}:${source.fragment ?? ''}:${source.alias ?? ''}`)}
                                        <button
                                            type="button"
                                            class="w-full border-none bg-transparent p-3 text-left hover:bg-surface-container-low focus:bg-surface-container-low focus:outline-none"
                                            onclick={(e) => openSource(source, e)}
                                        >
                                            <div class="mb-0.5 text-xs font-medium text-primary">
                                                {getDisplayTitle(source)}
                                            </div>
                                            <div class="mb-1 truncate text-xs text-on-surface-variant opacity-40">
                                                {source.source_path}
                                            </div>
                                            <div class="font-mono text-xs text-on-surface-variant opacity-60">
                                                {formatLink(source)}
                                            </div>
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                    {/if}
                </div>
            {/each}

            {#if overflowCount() > 0}
                <div class="p-3 text-xs text-on-surface-variant opacity-35">
                    {overflowCount()} more hidden
                </div>
            {/if}
        </div>
    {/if}
</div>

<AmbiguousLinkDialog
    bind:open={ambiguousDialogOpen}
    target={ambiguousTarget}
    paths={ambiguousPaths}
    onClose={() => {
        ambiguousDialogOpen = false;
    }}
/>
