<script>
    /** @type {{children?: import('svelte').Snippet}} */
    let { children } = $props();
</script>

<script module>
    import img from "$lib/htmlComponents/img.svelte";
    import Callout from "$lib/Callout.svelte";
    export { img, Callout };
</script>

{@render children?.()}
