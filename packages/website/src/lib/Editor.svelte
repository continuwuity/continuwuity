<script lang="ts" context="module"> 
    
    export enum LanguageConfig {
        None,
        JavaScript
    }
    </script>
<script lang="ts">
    import CodeMirror from "svelte-codemirror-editor";
    import { javascript } from "@codemirror/lang-javascript";
    import { theme } from "$lib/theme";
    import { githubLight, githubDark } from "$lib/themes/github";
    import type { Extension } from "@codemirror/state";


    export let value = "";
    export let readonly = false;
    export let lang: LanguageConfig = LanguageConfig.None;
    let langPlugin = null
    switch (lang) {
        case LanguageConfig.None:
            langPlugin = null
            break;
        case LanguageConfig.JavaScript:
            langPlugin = javascript()
            break;
    
    
        default:
            break;
    }
    let extensions :Extension[] = [];
    if (langPlugin) extensions.push(langPlugin)
    // $: console.log(value)

    // import { linter, lintGutter } from "@codemirror/lint";
    // import * as eslint from "eslint-linter-browserify";
    
        // lintGutter(),
        // linter(esLint(new eslint.Linter(), config)),

</script>

<div class="editor-wrapper card " 
class:no-header={!$$slots.header}>

{#if $$slots.header}
	<div class="header">
		<slot name="header"/>
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
    :global(.editor-wrapper.no-header .cm-scroller, .editor-wrapper.no-header .cm-editor) {
        border-radius: var(--border-radius);
    }
    :global(pre.cm-editor) {
        margin: 0;
    }
    
</style>
<!-- <CodeMirror basic={true} bind:value lang={javascript({})}   class="editor" /> -->
