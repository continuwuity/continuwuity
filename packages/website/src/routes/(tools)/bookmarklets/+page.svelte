<script lang="ts">
    import Editor, { LanguageConfig } from "$lib/Editor.svelte";
    import SvelteSeo from "svelte-seo";
    import { bookmarkify, parseMeta } from "./bookmarklets";
    import type { Config } from "./config";

    /** @type {import('./$types').Snapshot<string>} */
    export const snapshot = {
        capture: () => value,
        restore: (v: string) => (value = v),
    };

    let value = "";
    let output = "";
    let options: Config = {};
    async function process(str: string) {
        options = await parseMeta(str);
        let res = await bookmarkify(str, options);
        if (typeof res == "string") {
            output = res;
        }
    }

    $: progress = process(value);
</script>

<SvelteSeo
    title="Bookmarklet Maker"
    description="Make booklets in your browser with this tool. Make handy shortcuts to save time."
    canonical="https://jade.ellis.link/bookmarklets"
/>
<h1>Bookmarklet Maker</h1>
<Editor
    {value}
    on:change={(e) => (value = e.detail)}
    lang={LanguageConfig.JavaScript}
>
    <div slot="header" class="code-header">Input</div>
</Editor>

<h2>Output</h2>
{#await progress}
    <p>...waiting</p>
{:catch error}
    <p style="color: red">{error.message}</p>
{/await}
<textarea name="output" class="output card" rows="1" value={output} readonly
></textarea>

<!-- <Editor readonly={true}  /> -->
<p>
    Bookmark this link: <a href={output}>{options.name || "My Bookmarklet"}</a>
</p>
<p>
    Either drag the link to your bookmarlets bar or, on FireFox, right click and
    select "Bookmark Link"
</p>

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
        user-select: all;
    }
</style>
