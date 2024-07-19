<script lang="ts">
    export let src;
    export let alt;
    export let title;
    export let thumb;
    // export let align
    // export let small: boolean;
    // console.log("imgcmp", thumb);
    let className = "";
    export { className as class };
    let loaded = false
    // console.log(src)
    // import _PastedImage20240716123726Png from "./Pasted%20image%2020240716123726.png?meta";
</script>

<figure class={className}>
    <img
        {src}
        {alt}
        {title}
        width={thumb?.originalWidth}
        height={thumb?.originalHeight}
        style:background-image={loaded ? "none" : `url('${thumb?.thumbSrc}')`}
        on:load={() => loaded = true}
    />
    {#if title}
        <figcaption>{title}</figcaption>
    {/if}
</figure>
<!-- {:else}
<img
    {src}
    {alt}
    {title}
    style:float={align}
    width={thumb?.originalWidth}
    height={thumb?.originalHeight}
    style:background-image={loaded ? "none" : `url('${thumb?.thumbSrc}')`}
    on:load={() => loaded = true}
/>
{/if} -->

<style>
    img {
        max-height: 60vh;
        background-size: cover;
        background-repeat: no-repeat;
        width: auto;
        height: auto;
        display: block;
        margin-inline: auto;
    }
    figure {
        text-align: center;
    }
    figcaption {
        font-size: 0.95em;
        margin-block: calc(var(--spacing) / 2);
    }
</style>
