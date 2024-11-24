<script lang="ts" module>
    type Attrs = {
        [name: string]: string;
    };
    type AttrSource = Attrs | ((view: EditorView) => Attrs | null);
</script>

<script lang="ts">
    import { run } from 'svelte/legacy';

    // look at https://github.com/sveltejs/learn.svelte.dev/blob/main/src/routes/tutorial/%5Bslug%5D/Editor.svelte
    // import { javascript } from "@codemirror/lang-javascript";
    import { onDestroy, onMount, createEventDispatcher } from "svelte";
    import { theme } from "$lib/theme";
    import { githubLight, githubDark } from "$lib/themes/github";
    import { EditorView } from "@codemirror/view";
    import {
        EditorState,
        StateEffect,
        type Extension,
    } from "@codemirror/state";

    import { type LanguageSupport } from "@codemirror/language";
    import { get_base_extensions } from "./editorExtensions";



    interface Props {
        value?: string;
        contentAttributes?: AttrSource | null;
        lang?: LanguageSupport | null;
        useTab?: boolean;
        tabSize?: number;
        lineWrapping?: boolean;
        editable?: boolean;
        readonly?: boolean;
        placeholder?: string | HTMLElement | null | undefined;
        header?: import('svelte').Snippet;
    }

    let {
        value = $bindable(""),
        contentAttributes = null,
        lang = null,
        useTab = true,
        tabSize = 2,
        lineWrapping = false,
        editable = true,
        readonly = false,
        placeholder = undefined,
        header
    }: Props = $props();

    const is_browser = typeof window !== "undefined";

    let element: HTMLDivElement = $state();
    let view: EditorView = $state();



    let update_from_prop = false;
    let update_from_state = false;
    let first_config = true;
    let first_update = true;

    let langPlugin = null;

    let extensions: Extension[] = [];


    if (langPlugin !== null) extensions.push(langPlugin);
    if (contentAttributes !== null)
        extensions.push(EditorView.contentAttributes.of(contentAttributes));

    onMount(() => {
        view = create_editor_view();
        dispatch("ready", view);
    });
    onDestroy(() => view?.destroy());

    const dispatch = createEventDispatcher<{
        change: string;
        ready: EditorView;
        reconfigure: EditorView;
    }>();

    function create_editor_view(): EditorView {
        return new EditorView({
            parent: element,
            state: create_editor_state(value),
            dispatch(transaction) {
                view.update([transaction]);

                if (!update_from_prop && transaction.docChanged) {
                    on_change();
                }
            },
        });
    }

    function reconfigure(): void {
        if (first_config) {
            first_config = false;
            return;
        }

        view.dispatch({
            effects: StateEffect.reconfigure.of(state_extensions),
        });

        dispatch("reconfigure", view);
    }

    function update(value: string | null | undefined): void {
        if (first_update) {
            first_update = false;
            return;
        }

        if (update_from_state) {
            update_from_state = false;
            return;
        }

        update_from_prop = true;

        if (value === undefined) {
            return;
        }
        const currentValue = view ? view.state.doc.toString() : "";
        if (view && value !== currentValue) {
            view.dispatch({
                changes: {
                    from: 0,
                    to: currentValue.length,
                    insert: value || "",
                },
            });
        }

        update_from_prop = false;
    }

    function handle_change(): void {
        const new_value = view.state.doc.toString();
        if (new_value === value) return;

        update_from_state = true;

        value = new_value;
        dispatch("change", value);
    }

    function create_editor_state(
        value: string | null | undefined,
    ): EditorState {
        return EditorState.create({
            doc: value ?? undefined,
            extensions: state_extensions,
        });
    }

    // $: console.log(value)

    // import { linter, lintGutter } from "@codemirror/lint";
    // import * as eslint from "eslint-linter-browserify";

    // lintGutter(),
    // linter(esLint(new eslint.Linter(), config)),
    run(() => {
        view && update(value);
    });
    let state_extensions = $derived([
        ...get_base_extensions(
            useTab,
            tabSize,
            lineWrapping,
            placeholder,
            editable,
            readonly,
            lang,
        ),
        $theme == "dark" ? githubDark : githubLight,
        ...extensions,
    ]);
    run(() => {
        view && state_extensions && reconfigure();
    });
    let on_change = $derived(handle_change);
</script>

<div class="editor-wrapper card" class:no-header={!header}>
    {#if header}
        <div class="header">
            {@render header?.()}
        </div>
    {/if}
    {#if is_browser}
        <div class="codemirror-wrapper editor" bind:this={element}></div>
    {:else}
        <div class="scm-waiting editor">
            <div class="scm-waiting__loading scm-loading">
                <div class="scm-loading__spinner"></div>
                <p class="scm-loading__text">Loading editor...</p>
            </div>
            <div class="cm-editor"><pre class="scm-pre">{value}</pre></div>
            
        </div>
    {/if}
    <!-- <CodeMirror
        {value}
        class="editor"
        theme={$theme == "dark" ? githubDark : githubLight}
        {extensions}
        {readonly}
        on:change
    /> -->
</div>

<!-- <CodeMirror basic={true} bind:value lang={javascript({})}   class="editor" /> -->

<style>
    .editor-wrapper {
        /* min-height: 200px; */
        position: relative;
        z-index: 1;
        background-color: var(--surface-secondary-color);
    }

    .codemirror-wrapper :global(.cm-focused) {
        outline: none;
    }

    .scm-waiting {
        position: relative;
    }
    .scm-waiting__loading {
        position: absolute;
        top: 0;
        left: 0;
        bottom: 0;
        right: 0;
        background-color: var(--shadow-color);
    }

    .scm-loading {
        display: flex;
        align-items: center;
        justify-content: center;
    }
    .scm-loading__spinner {
        width: 1rem;
        height: 1rem;
        border-radius: 100%;
        border: solid 2px var(--theme);
        border-top-color: transparent;
        margin-right: 0.75rem;
        animation: spin 1s linear infinite;
    }
    .scm-loading__text {
        font-family: sans-serif;
    }
    .scm-pre {
        font-size: 0.85rem;
        margin: 0;
        padding: 4px 2px 4px 6px;
        font-family: monospace;
        tab-size: 2;
        -moz-tab-size: 2;
        resize: none;
        pointer-events: none;
        user-select: none;
        overflow: auto;
    }

    @keyframes spin {
        0% {
            transform: rotate(0deg);
        }
        100% {
            transform: rotate(360deg);
        }
    }

    :global(.editor-wrapper .cm-scroller, .editor-wrapper .cm-editor) {
        min-height: 200px;
        border-bottom-left-radius: var(--border-radius);
        border-bottom-right-radius: var(--border-radius);
        /* box-shadow: var(--shadow);
        background-color: var(--surface-color); */
    }
    :global(
            .editor-wrapper.no-header .cm-scroller,
            .editor-wrapper.no-header .cm-editor
        ) {
        border-radius: var(--border-radius);
    }
    :global(pre.cm-editor) {
        margin: 0;
    }
</style>
