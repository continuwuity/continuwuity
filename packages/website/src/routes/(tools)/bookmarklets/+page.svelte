<script lang="ts">
    import Editor from "$lib/Editor.svelte";
    import { javascript } from "@codemirror/lang-javascript";
    import SvelteSeo from "svelte-seo";
    import { bookmarkify, parseMeta } from "./bookmarklets";
    import type { Config } from "./config";
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
    let options: Config = $state({});
    async function process(str: string) {
        options = await parseMeta(str);
        const res = await bookmarkify(str, options, minify);
        if (typeof res == "string") {
            return res;
        }
    }

    const contentAttributes = { "aria-label": "Bookmarklet editor" };

    let computation = $derived(process(value));

    $effect(async () => {
        output = await computation;
    });
</script>

<SvelteSeo
    title="Bookmarklet Maker"
    description="Make booklets in your browser with this tool. Make handy shortcuts to save time."
    canonical={SITE_URL + "/bookmarklets"}
/>

<main class="main container" id="page-content">
    <h1>Bookmarklet Maker</h1>
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
    <label for="output">Bookmarklet code</label>
    <textarea
        name="output"
        id="output"
        class="output card"
        rows="1"
        value={output}
        readonly
    ></textarea>

    <!-- <Editor readonly={true}  /> -->
    <p>
        Bookmark this link: <a href={output}
            >{options.name || "My Bookmarklet"}</a
        >
    </p>
    <p>
        Either drag the link to your bookmarlets bar or, on FireFox, right click
        and select "Bookmark Link".
    </p>
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
