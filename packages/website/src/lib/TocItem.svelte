<script lang="ts">
    import TocItem from './TocItem.svelte';
    
    interface Props {
        node: nestedListNode;
    }

    let { node }: Props = $props();
    export const listType = "ul"
</script>

<li class="toc-item {"toc-item-" + node.level}">
<a href={"#" + node.id} class="toc-link {"toc-link-" + node.level}">{node.title}</a>
{#if node.children.length > 0}
<svelte:element this={listType} class="toc-level {"toc-level-" + node.children[0].level}">
    {#each node.children as nodes}
        <TocItem node={nodes} {listType} />
    {/each}
</svelte:element>
{/if}
</li>