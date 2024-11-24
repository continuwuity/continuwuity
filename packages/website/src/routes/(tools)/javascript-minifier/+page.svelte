<script lang="ts">
    import Editor from "$lib/Editor.svelte";
    import { javascript } from "@codemirror/lang-javascript";
    import SvelteSeo from "svelte-seo";
    import { init } from "$lib/workers/terser";
    import { SITE_URL } from "$lib/metadata";

    /** @type {import('./$types').Snapshot<string>} */
    export const snapshot = {
        capture: () => value,
        restore: (v: string) => (value = v),
    };

    const minify = init().minify;

    let value = $state("");
    let output = $state("");
    async function process(str: string): string {
        if (value === "") {
            return "";
        }
        const result = await minify(str);
        if (typeof result.code == "string") {
            return result.code;
        } else {
            console.error(result);
        }
    }

    const contentAttributes = { "aria-label": "Javascript editor" };

    let computation = $derived(process(value));

    $effect(async () => {
        output = await computation;
    });
</script>

<SvelteSeo
    title="Javascript Minifier"
    description="Reduce JavaScript code size with this handy online tool. It's easy to minify your JavaScript code."
    canonical={SITE_URL + "/javascript-minifier"}
/>

<main class="main container" id="page-content">
    <h1>Javascript Minifier</h1>
    <Editor
        {value}
        on:change={(e) => (value = e.detail)}
        lang={javascript()}
        {contentAttributes}
    >
        {#snippet header()}
                <div  class="code-header">Input</div>
            {/snippet}
    </Editor>

    <h2>Output</h2>
    {#await computation}
        <p>...waiting</p>
    {:catch error}
        <p style="color: red">{error.message}</p>
    {/await}
    <label for="output">Minified code</label>
    <textarea
        name="output"
        id="output"
        class="output card"
        rows="1"
        value={output}
        readonly
    ></textarea>
</main>

<style>
    .code-header {
        padding: 0.25em 0.5em;
    }
    .output {
        position: relative;
        z-index: 1;
        background-color: var(--input-background-color);
        color: var(--input-color);
        border: none;
        font-family: monospace;
        line-height: 1.4;
        display: block;
        /* white-space: pre; */
        /* word-wrap: normal; */
        resize: vertical;
        width: 100%;
        height: 8ex;
        padding: 4px 2px 4px 6px;
        font-size: 1rem;
        scrollbar-gutter: stable;
        /* user-select: all; */
    }
</style>
