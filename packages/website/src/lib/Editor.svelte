<script lang="ts" context="module">
    type Attrs = {
        [name: string]: string;
    };
    type AttrSource = Attrs | ((view: EditorView) => Attrs | null);
</script>

<script lang="ts">
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

    export let value = "";
    export let contentAttributes: AttrSource | null = null;
    export let lang: LanguageSupport | null = null;

    export let useTab = true;
    export let tabSize = 2;

    export let lineWrapping = false;
    export let editable = true;
    export let readonly = false;
    export let placeholder: string | HTMLElement | null | undefined = undefined;

    const is_browser = typeof window !== "undefined";

    let element: HTMLDivElement;
    let view: EditorView;

    $: view && update(value);
    $: view && state_extensions && reconfigure();

    $: on_change = handle_change;

    let update_from_prop = false;
    let update_from_state = false;
    let first_config = true;
    let first_update = true;

    let langPlugin = null;

    let extensions: Extension[] = [];

    $: state_extensions = [
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
    ];

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
</script>

<div class="editor-wrapper card" class:no-header={!$$slots.header}>
    {#if $$slots.header}
        <div class="header">
            <slot name="header" />
        </div>
    {/if}
    {#if is_browser}
        <div class="codemirror-wrapper editor" bind:this={element} />
    {:else}
        <div class="scm-waiting editor">
            <div class="scm-waiting__loading scm-loading">
                <div class="scm-loading__spinner" />
                <p class="scm-loading__text">Loading editor...</p>
            </div>

            <pre class="scm-pre cm-editor">{value}</pre>
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
        min-height: 200px;
        position: relative;
        z-index: 1;
        background-color: var(--surface-secondary-color);
    }

    .codemirror-wrapper :global(.cm-focused) {
        outline: none;
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
