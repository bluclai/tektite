<script lang="ts">
    /**
     * EditorPane — CodeMirror 6 editor for a single open file.
     *
     * Lifecycle:
     *   1. On mount: create EditorView, load file content via editor_read_file.
     *   2. On document change: schedule autosave (1.5 s debounce).
     *   3. Ctrl+S / Cmd+S: flush immediately.
     *   4. On destroy (tab switch / close): flush any pending autosave, destroy view.
     *
     * The wiki-link syntax/decorations and autocomplete source are wired in
     * after construction via dedicated compartments so they can be reconfigured
     * without rebuilding the base extension stack.
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
    import { findMarkdownHeadingPosition, findMarkdownTagPosition } from '$lib/editor-navigation';
    import { editorNavigationStore } from '$lib/stores/editor-navigation.svelte';
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
    }

    let { path }: Props = $props();

    // ---------------------------------------------------------------------------
    // DOM ref
    // ---------------------------------------------------------------------------

    let containerEl = $state<HTMLDivElement | null>(null);

    // ---------------------------------------------------------------------------
    // Ambiguous link dialog state — opened when wiki-link resolution finds
    // multiple candidates and the user needs to disambiguate.
    // ---------------------------------------------------------------------------

    let ambiguousOpen = $state(false);
    let ambiguousTarget = $state('');
    let ambiguousPaths = $state<string[]>([]);

    // ---------------------------------------------------------------------------
    // Compartments — reconfigurable extension slots. Each compartment wraps a
    // single extension group that can be swapped without reconstructing the
    // full EditorState.
    // ---------------------------------------------------------------------------

    /** Slot for wiki-link syntax + decorations. */
    export const wikiLinkCompartment = new Compartment();

    /** Slot for autocomplete sources. */
    export const autocompleteCompartment = new Compartment();

    // ---------------------------------------------------------------------------
    // View instance
    // ---------------------------------------------------------------------------

    let view = $state<EditorView | null>(null);

    // ---------------------------------------------------------------------------
    // Save state tracking
    // ---------------------------------------------------------------------------

    let autosaveTimer: ReturnType<typeof setTimeout> | null = null;
    const AUTOSAVE_DELAY_MS = 1500;

    let lastSavedContent = $state('');
    let loadError = $state<string | null>(null);

    // External change conflict banner (non-modal) — surfaces when the file
    // changes on disk while the editor has it open.
    let externalBannerOpen = $state(false);
    let externalConflict = $state(false);
    let externalContentSnapshot = $state<string | null>(null);

    function formatCommandError(error: unknown, fallback: string): string {
        if (error instanceof Error && error.message.trim().length > 0) {
            return error.message;
        }

        if (typeof error === 'string' && error.trim().length > 0) {
            return error;
        }

        return fallback;
    }

    async function saveFile(content: string, reason: 'autosave' | 'manual' | 'teardown' = 'autosave'): Promise<void> {
        const savingDetail =
            reason === 'manual'
                ? 'Saving file…'
                : reason === 'teardown'
                    ? 'Saving pending changes before closing…'
                    : 'Autosaving…';

        editorStore.setSaveState('saving', { detail: savingDetail, target: path });
        try {
            await invoke<void>('editor_write_file', { path, content });
            lastSavedContent = content;
            if (externalContentSnapshot === content) {
                externalBannerOpen = false;
                externalConflict = false;
                externalContentSnapshot = null;
            }
            const successDetail = reason === 'manual' ? 'Saved' : 'Autosave complete';
            editorStore.setSaveState('saved', { detail: successDetail, target: path });
        } catch (error) {
            editorStore.setSaveState('error', {
                detail: formatCommandError(error, 'Failed to save file.'),
                target: path,
            });
        }
    }

    function scheduleAutosave(content: string) {
        editorStore.setSaveState('unsaved', { detail: 'Waiting to autosave…', target: path });
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
            const content = view.state.doc.toString();
            if (content === lastSavedContent) {
                editorStore.setSaveState('saved', { detail: 'No changes to save', target: path });
                return;
            }
            void saveFile(content, 'manual');
        }
    }

    function flushPendingAutosaveOnTeardown() {
        if (autosaveTimer !== null) {
            clearTimeout(autosaveTimer);
            autosaveTimer = null;
        }

        if (!view) return;

        const content = view.state.doc.toString();
        if (content === lastSavedContent) return;

        void saveFile(content, 'teardown');
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
        } catch (error) {
            editorStore.setSaveState('error', {
                detail: formatCommandError(error, 'Failed to reload file from disk.'),
                target: path,
            });
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

    function jumpToPosition(position: number) {
        if (!view) return;

        view.dispatch({
            selection: { anchor: position },
            scrollIntoView: true,
        });
        view.focus();
    }

    function jumpToHeading(request: { id: number; headingText: string; level: number; path: string }): void {
        if (!view) return;

        const position = findMarkdownHeadingPosition(
            view.state.doc.toString(),
            request.headingText,
            request.level,
        );

        if (position === null) {
            editorStore.setSaveState('error', {
                detail: `Couldn't find heading ${request.headingText}`,
                target: request.path,
            });
            editorNavigationStore.consume(request.id);
            return;
        }

        jumpToPosition(position);
        editorStore.setSaveState('saved', {
            detail: `Jumped to heading ${request.headingText}`,
            target: request.path,
        });
        editorNavigationStore.consume(request.id);
    }

    function jumpToTag(request: { id: number; tagName: string; path: string }): void {
        if (!view) return;

        const position = findMarkdownTagPosition(view.state.doc.toString(), request.tagName);
        if (position === null) {
            editorStore.setSaveState('error', {
                detail: `Couldn't find tag #${request.tagName}`,
                target: request.path,
            });
            editorNavigationStore.consume(request.id);
            return;
        }

        jumpToPosition(position);
        editorStore.setSaveState('saved', {
            detail: `Jumped to tag #${request.tagName}`,
            target: request.path,
        });
        editorNavigationStore.consume(request.id);
    }

    $effect(() => {
        const request = editorNavigationStore.request;
        if (!view || !request) return;
        if (request.path !== relativePathForCurrentFile()) return;

        if (request.kind === 'heading') {
            jumpToHeading(request);
            return;
        }

        if (request.kind === 'tag') {
            jumpToTag(request);
        }
    });

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

            // --- Syntax: markdown ---
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

            // --- Reconfigurable compartments (start empty) ---
            wikiLinkCompartment.of([]),
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
                loadError = null;
                editorStore.setSaveState('saved', { detail: 'File opened', target: path });
            } catch (error) {
                loadError = formatCommandError(error, 'Failed to open file.');
                editorStore.setSaveState('error', { detail: loadError, target: path });
            }

            if (destroyed) return;
            if (loadError !== null) return;

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
            flushPendingAutosaveOnTeardown();
            unlistenExternal?.();
            view?.destroy();
            view = null;
        };
    });

    // ---------------------------------------------------------------------------
    // Reactive: path changes are handled by the parent keying this component
    // on the active tab ID, giving each tab its own EditorView instance.
    // ---------------------------------------------------------------------------
</script>

<!--
    The editor container fills the pane. CM6 appends its own DOM into this div.
    overflow-hidden prevents double scrollbars — CM6 manages its own scroll.
-->
<div class="relative h-full w-full overflow-hidden" aria-label="Editor">
    {#if loadError}
        <div class="absolute inset-x-4 top-4 z-20 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-sm text-red-200 shadow-[0_12px_32px_rgba(0,0,0,0.22)]">
            Couldn’t open this file: {loadError}
        </div>
    {/if}

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

<!-- Ambiguous link disambiguation dialog -->
<AmbiguousLinkDialog
    bind:open={ambiguousOpen}
    target={ambiguousTarget}
    paths={ambiguousPaths}
    onClose={() => { ambiguousOpen = false; }}
/>
