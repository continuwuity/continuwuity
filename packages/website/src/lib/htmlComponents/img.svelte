<script>
    export let src;
    export let alt;
    export let title;
    export let thumb;
    console.log("imgcmp", thumb);
    let className = "";
    export { className as class };
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
        style:background-image="url('{thumb?.thumbSrc}')"
    />
    {#if title}
        <figcaption>{title}</figcaption>
    {/if}
</figure>

<style>
    img {
        max-height: 60vh;
        background-size: cover;
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
