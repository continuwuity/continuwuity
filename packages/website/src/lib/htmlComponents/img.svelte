<script>
    export let src;
    export let alt;
    let className = ""
    export { className as class };
    // console.log(src)
    // import _PastedImage20240716123726Png from "./Pasted%20image%2020240716123726.png?meta";
</script>

<figure class={className}>
    <img {src} {alt} />
    {#if alt}
        <figcaption>{alt}</figcaption>
    {/if}
</figure>

<style>
    img {
        max-height: 60vh;
        display: block;
        margin-inline: auto;
    }
    figure {
        text-align: center;
    }
    figcaption {

        font-size: 0.95em;
        margin-block: calc(var(--spacing)/2);
    }
</style>
