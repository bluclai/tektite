<script lang="ts">
    /**
     * EditorPane — CodeMirror 6 editor for a single open file.
     *
     * Lifecycle:
     *   1. On mount: create EditorView, load file content via editor_read_file.
     *   2. On document change: schedule autosave (1.5 s debounce).
     *   3. Ctrl+S / Cmd+S: flush immediately.
     *   4. On destroy (tab switch / close): cancel pending timer, destroy view.
     *
     * Extension compartments are exported so later phases (wiki-link syntax,
     * live preview, autocomplete) can reconfigure them without rebuilding the
     * base stack.
     */

    import { onMount } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';

    import { EditorView, keymap, lineNumbers, highlightActiveLine } from '@codemirror/view';
    const { lineWrapping } = EditorView;
    import { EditorState, Compartment } from '@codemirror/state';
    import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
    import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands';
    import { closeBrackets, autocompletion, closeBracketsKeymap } from '@codemirror/autocomplete';

    import { clayTheme } from '$lib/editor/theme';
    import { wikiLinkExtension, wikiLinkAutocomplete } from '$lib/editor/wiki-link';
    import { livePreviewExtension } from '$lib/editor/live-preview';
    import { editorStore } from '$lib/stores/editor.svelte';
    import { vaultStore } from '$lib/stores/vault.svelte';
    import { workspaceStore } from '$lib/stores/workspace.svelte';
    import AmbiguousLinkDialog from '$lib/components/AmbiguousLinkDialog.svelte';

    // ---------------------------------------------------------------------------
    // Props
    // ---------------------------------------------------------------------------

    interface Props {
        /** Absolute filesystem path — passed directly to read/write commands. */
        path: string;
        /** Explicit reactive preview-mode prop from parent pane. */
        previewMode?: boolean;
    }

    let { path, previewMode = false }: Props = $props();

    // ---------------------------------------------------------------------------
    // DOM ref
    // ---------------------------------------------------------------------------

    let containerEl = $state<HTMLDivElement | null>(null);

    // ---------------------------------------------------------------------------
    // Ambiguous link dialog state (Phase 9)
    // ---------------------------------------------------------------------------

    let ambiguousOpen = $state(false);
    let ambiguousTarget = $state('');
    let ambiguousPaths = $state<string[]>([]);

    // ---------------------------------------------------------------------------
    // Compartments — reconfigurable extension slots for future phases.
    // Each compartment wraps a single extension group that can be swapped
    // without reconstructing the full EditorState.
    // ---------------------------------------------------------------------------

    /** Slot for wiki-link syntax + decorations (Phase 7). */
    export const wikiLinkCompartment = new Compartment();

    /** Slot for live preview decorations (Phase 10). */
    export const livePreviewCompartment = new Compartment();

    /** Slot for autocomplete sources (Phase 7+). */
    export const autocompleteCompartment = new Compartment();

    // ---------------------------------------------------------------------------
    // View instance
    // ---------------------------------------------------------------------------

    let view: EditorView | null = null;

    // ---------------------------------------------------------------------------
    // Save state tracking
    // ---------------------------------------------------------------------------

    let autosaveTimer: ReturnType<typeof setTimeout> | null = null;
    const AUTOSAVE_DELAY_MS = 1500;

    let lastSavedContent = $state('');

    // Phase 10: external change conflict banner (non-modal).
    let externalBannerOpen = $state(false);
    let externalConflict = $state(false);
    let externalContentSnapshot = $state<string | null>(null);

    async function saveFile(content: string): Promise<void> {
        editorStore.setSaveState('saving');
        try {
            await invoke<void>('editor_write_file', { path, content });
            lastSavedContent = content;
            if (externalContentSnapshot === content) {
                externalBannerOpen = false;
                externalConflict = false;
                externalContentSnapshot = null;
            }
            editorStore.setSaveState('saved');
        } catch {
            editorStore.setSaveState('error');
        }
    }

    function scheduleAutosave(content: string) {
        editorStore.setSaveState('unsaved');
        if (autosaveTimer !== null) clearTimeout(autosaveTimer);
        autosaveTimer = setTimeout(() => {
            autosaveTimer = null;
            void saveFile(content);
        }, AUTOSAVE_DELAY_MS);
    }

    function flushSave() {
        if (autosaveTimer !== null) {
            clearTimeout(autosaveTimer);
            autosaveTimer = null;
        }
        if (view) {
            void saveFile(view.state.doc.toString());
        }
    }

    function relativePathForCurrentFile(): string {
        const vaultRoot = vaultStore.path ?? '';
        if (vaultRoot && path.startsWith(vaultRoot + '/')) {
            return path.slice(vaultRoot.length + 1);
        }
        return path;
    }

    async function onExternalChange(): Promise<void> {
        if (!view) return;

        let diskContent = '';
        try {
            diskContent = await invoke<string>('editor_read_file', { path });
        } catch {
            return;
        }

        const localContent = view.state.doc.toString();

        // No divergence: nothing to show.
        if (diskContent === localContent) {
            externalBannerOpen = false;
            externalConflict = false;
            externalContentSnapshot = null;
            return;
        }

        externalContentSnapshot = diskContent;
        externalBannerOpen = true;
        // Conflict = local buffer diverged from the last explicitly saved content.
        externalConflict = localContent !== lastSavedContent;
    }

    function reloadFromDisk() {
        if (!view || externalContentSnapshot === null) return;
        if (autosaveTimer !== null) {
            clearTimeout(autosaveTimer);
            autosaveTimer = null;
        }

        const next = externalContentSnapshot;
        const currentLen = view.state.doc.length;
        view.dispatch({
            changes: { from: 0, to: currentLen, insert: next },
            selection: { anchor: Math.min(view.state.selection.main.head, next.length) },
        });

        lastSavedContent = next;
        editorStore.setSaveState('saved');
        externalBannerOpen = false;
        externalConflict = false;
        externalContentSnapshot = null;
    }

    function keepMyChanges() {
        externalBannerOpen = false;
        externalConflict = false;
        externalContentSnapshot = null;
    }

    // ---------------------------------------------------------------------------
    // CM6 update listener — fires on every document change
    // ---------------------------------------------------------------------------

    const saveListener = EditorView.updateListener.of((update) => {
        if (update.docChanged) {
            scheduleAutosave(update.state.doc.toString());
        }
    });

    // ---------------------------------------------------------------------------
    // Explicit save keybinding (Ctrl+S / Cmd+S)
    // ---------------------------------------------------------------------------

    const saveKeymap = keymap.of([
        {
            key: 'Mod-s',
            run() {
                flushSave();
                return true;
            },
        },
    ]);

    // ---------------------------------------------------------------------------
    // Extension stack
    //
    // Ordered array of compartments and static extensions. Later phases insert
    // into named compartments rather than appending to this array, so the
    // relative ordering (theme → syntax → keymaps → save listener) is stable.
    // ---------------------------------------------------------------------------

    function buildExtensions(initialContent: string) {
        return [
            // --- Theme (static — Clay design system tokens) ---
            clayTheme,

            // --- Syntax: markdown (embedded language highlighting added in Phase 7+) ---
            markdown({
                base: markdownLanguage,
                addKeymap: true,
            }),

            // --- Editor behaviour ---
            history(),
            closeBrackets(),
            lineNumbers(),
            highlightActiveLine(),
            // Wrap long lines — eliminates horizontal scrolling entirely.
            // Column width is capped in the theme via .cm-content max-width.
            lineWrapping,

            // --- Keymaps (ordered: most specific first) ---
            keymap.of([
                indentWithTab,
                ...closeBracketsKeymap,
                ...defaultKeymap,
                ...historyKeymap,
            ]),

            // Save keybinding (Mod-s) — before the default keymap
            saveKeymap,

            // --- Future compartments (start empty) ---
            wikiLinkCompartment.of([]),
            livePreviewCompartment.of([]),
            autocompleteCompartment.of([autocompletion()]),

            // --- Save listener ---
            saveListener,
        ];
    }

    // ---------------------------------------------------------------------------
    // Mount / destroy
    // ---------------------------------------------------------------------------

    onMount(() => {
        if (!containerEl) return;

        let destroyed = false;

        (async () => {
            // Load file content. On error show empty document; don't crash.
            let initialContent = '';
            try {
                initialContent = await invoke<string>('editor_read_file', { path });
                lastSavedContent = initialContent;
                editorStore.setSaveState('saved');
            } catch {
                editorStore.setSaveState('error');
            }

            if (destroyed) return;

            const state = EditorState.create({
                doc: initialContent,
                extensions: buildExtensions(initialContent),
            });

            view = new EditorView({
                state,
                parent: containerEl!,
            });

            // Derive vault-relative path for proximity tiebreaking.
            const vaultRoot = vaultStore.path ?? '';
            const sourcePath = relativePathForCurrentFile();

            // Configure wiki-link syntax + decorations and autocomplete.
            view.dispatch({
                effects: [
                    wikiLinkCompartment.reconfigure(
                        wikiLinkExtension({
                            vaultRoot,
                            sourcePath,
                            onFollow(resolvedPath) {
                                // index_resolve_link returns vault-relative paths.
                                // Tabs store vault-relative paths; LeafPane prepends
                                // vaultStore.path to construct absolute paths for EditorPane.
                                workspaceStore.openTab(resolvedPath);
                            },
                            onAmbiguous(t, ps) {
                                ambiguousTarget = t;
                                ambiguousPaths = ps;
                                ambiguousOpen = true;
                            },
                        }),
                    ),
                    livePreviewCompartment.reconfigure(
                        previewMode ? livePreviewExtension() : [],
                    ),
                    autocompleteCompartment.reconfigure(wikiLinkAutocomplete()),
                ],
            });
        })();

        let unlistenExternal: UnlistenFn | null = null;
        void listen<{ paths: string[] }>('vault-files-changed', (event) => {
            const currentRel = relativePathForCurrentFile();
            if (!event.payload.paths.includes(currentRel)) return;
            void onExternalChange();
        }).then((fn) => {
            unlistenExternal = fn;
        });

        return () => {
            destroyed = true;
            // Cancel any pending autosave — don't flush; the user navigated away
            // and the last explicit save / last autosave is the authoritative state.
            if (autosaveTimer !== null) {
                clearTimeout(autosaveTimer);
                autosaveTimer = null;
            }
            unlistenExternal?.();
            view?.destroy();
            view = null;
        };
    });

    // ---------------------------------------------------------------------------
    // Reactive: path and preview mode changes are handled by the parent keying
    // this component on both active tab ID and preview mode. We intentionally
    // remount the editor on preview toggle because it's the smallest reliable
    // path and avoids subtle CM6 reconfigure failures.
    // ---------------------------------------------------------------------------
</script>

<!--
    The editor container fills the pane. CM6 appends its own DOM into this div.
    overflow-hidden prevents double scrollbars — CM6 manages its own scroll.
-->
<div class="relative h-full w-full overflow-hidden" aria-label="Editor">
    {#if externalBannerOpen}
        <div class="absolute top-2 left-1/2 z-20 w-[min(860px,calc(100%-2rem))] -translate-x-1/2 rounded-md bg-surface-container-high/95 px-3 py-2 backdrop-blur-md shadow-[0_12px_32px_rgba(0,0,0,0.22)]">
            <div class="flex items-center gap-2 text-xs text-on-surface-variant">
                <span class="text-[0.72rem] text-on-surface/90">
                    {#if externalConflict}
                        This file changed outside Tektite and your editor has local edits.
                    {:else}
                        This file changed outside Tektite.
                    {/if}
                </span>
                <div class="ml-auto flex items-center gap-2">
                    <button
                        type="button"
                        class="rounded px-2 py-1 text-[0.72rem] text-on-surface-variant hover:bg-surface-container"
                        onclick={keepMyChanges}
                    >
                        Keep mine
                    </button>
                    <button
                        type="button"
                        class="rounded bg-primary/20 px-2 py-1 text-[0.72rem] text-primary hover:bg-primary/30"
                        onclick={reloadFromDisk}
                    >
                        Reload
                    </button>
                </div>
            </div>
        </div>
    {/if}

    <div bind:this={containerEl} class="h-full w-full overflow-hidden"></div>
</div>

<!-- Ambiguous link disambiguation dialog (Phase 9) -->
<AmbiguousLinkDialog
    bind:open={ambiguousOpen}
    target={ambiguousTarget}
    paths={ambiguousPaths}
    onClose={() => { ambiguousOpen = false; }}
/>
