<script lang="ts">
    import TocItem from "./TocItem.svelte";
    const className = "toc";
    type FlatHeading = { level: number; title: string };
    export let headings: nestedListNode[];

    // creates a `class` property, even
    // though it is a reserved word
    export { className as class };
    export const listType = "ul";

    let open = false;
    /** @type {import('./$types').Snapshot<string>} */
    export const snapshot = {
        capture: () => open,
        restore: (value: boolean) => (open = value),
    };

    // console.log(headings);
</script>

{#if headings?.length > 0}
    <aside class={className}>
        <details bind:open>
            <summary accesskey="c" title="(Alt + C)">Table of Contents</summary>
            <div class="inner">
                <svelte:element
                    this={listType}
                    class="toc-level {'toc-level-' + headings[0].level}"
                >
                    {#each headings as node}
                        <TocItem {node} {listType} />
                    {/each}
                </svelte:element>
            </div>
        </details>
    </aside>
{/if}

<style>
    aside {
        margin-block: calc(var(--spacing) / 4);
    }
    details {
        /* margin: var(--spacing) 2px; */
        margin: 0 2px;
        border: 1px solid var(--surface-secondary-color);
        background: var(--surface-color);
        border-radius: var(--border-radius);
        padding: 0.4em;
    }
    details summary {
        cursor: zoom-in;
        margin-inline-start: 10px;
        user-select: none;
    }
    details[open] summary {
        cursor: zoom-out;
    }
    summary {
        font-weight: 500;
    }
    .inner {
        padding: 0 10px;
        opacity: 0.9;
        margin-block-start: calc(var(--spacing) / 4);
        margin-block-end: calc(var(--spacing) / 2);
        margin-inline: calc(var(--spacing) / 2);
    }
    .inner :global(ul) {
        margin: 0;
        margin-inline-start: calc(var(--spacing));
        padding: 0;
    }
    summary:focus {
        outline: 0;
    }
</style>
