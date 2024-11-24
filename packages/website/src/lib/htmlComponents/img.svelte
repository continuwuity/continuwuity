<script lang="ts">
    interface Props {
        src: any;
        alt: any;
        title: any;
        thumb: any;
        class?: string;
    }

    let {
        src,
        alt,
        title,
        thumb,
        class: className
    }: Props = $props();
    // export let align
    // export let small: boolean;
    // console.log("imgcmp", thumb);
    let loaded = $state(false)
    // console.log(thumb)
    // import _PastedImage20240716123726Png from "./Pasted%20image%2020240716123726.png?meta";
</script>
<!-- <figure class={className}> -->
<!-- Svelte 5 hydration bug means we can't nest image inside figure -->
    <img
        {src}
        {alt}
        {title}
        class={className}
        width={thumb?.originalWidth}
        height={thumb?.originalHeight}
        style:background-image={loaded ? "none" : `url('${thumb?.thumbSrc}')`}
        on:load={() => loaded = true}
        decoding="async"
        style:--aspect-ratio={thumb?.originalWidth / thumb?.originalHeight}
    />
    <!-- {#if title}
        <figcaption>{title}</figcaption>
    {/if} -->
<!-- </figure> -->
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
        height: 100%;
        background-size: cover;
        background-repeat: no-repeat;
        display: block;
        margin-inline: auto;
        max-width: calc(min(100%, 60vh * var(--aspect-ratio)));
    }
    figure {
        text-align: center;
    }
    figcaption {
        font-size: 0.95em;
        margin-block: calc(var(--spacing) / 2);
    }
</style>
