<script lang="ts" context="module">
    export enum LanguageConfig {
        None,
        JavaScript,
    }

    type Attrs = {
        [name: string]: string;
    };
    type AttrSource = Attrs | ((view: EditorView) => Attrs | null);
</script>

<script lang="ts">
    // look at https://github.com/sveltejs/learn.svelte.dev/blob/main/src/routes/tutorial/%5Bslug%5D/Editor.svelte
    import CodeMirror from "svelte-codemirror-editor";
    import { javascript } from "@codemirror/lang-javascript";
    import { theme } from "$lib/theme";
    import { githubLight, githubDark } from "$lib/themes/github";
    import type { Extension } from "@codemirror/state";
    import { EditorView } from "@codemirror/view";

    export let value = "";
    export let contentAttributes: AttrSource | null = null;
    export let readonly = false;
    export let lang: LanguageConfig = LanguageConfig.None;
    let langPlugin = null;

    switch (lang) {
        case LanguageConfig.None:
            langPlugin = null;
            break;
        case LanguageConfig.JavaScript:
            langPlugin = javascript();
            break;

        default:
            break;
    }
    let extensions: Extension[] = [
    ];
    if (langPlugin !== null) extensions.push(langPlugin);
    if (contentAttributes !== null) extensions.push(EditorView.contentAttributes.of(contentAttributes));

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
    <CodeMirror
        {value}
        class="editor"
        theme={$theme == "dark" ? githubDark : githubLight}
        {extensions}
        {readonly}
        on:change
    />
</div>
<!-- <CodeMirror basic={true} bind:value lang={javascript({})}   class="editor" /> -->

<style>
    .editor-wrapper {
        min-height: 200px;
        position: relative;
        z-index: 1;
        background-color: var(--surface-secondary-color);
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
