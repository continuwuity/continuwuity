<script lang="ts">
    // https://github.com/mattjennings/sveltekit-blog-template/blob/main/src/routes/post/%5Bslug%5D/%2Bpage.svelte

    import { browser } from "$app/environment";
    import SvelteSeo from "svelte-seo";
    export let data;
    import { SITE_URL } from "$lib/metadata";
    import Toc from "$lib/Toc.svelte";
    // let GhReleasesDownload: Promise<any>;
    // if (data.ghReleaseData) {
    //     GhReleasesDownload = import("$lib/GhReleasesDownload.svelte").then((m) => m.default)
    // }
    $: canonical = SITE_URL + "/blog/" + data.post.canonical;

    $: webShareAPISupported = browser && typeof navigator.share !== 'undefined';
    // let webShareAPISupported = true;

    $: handleWebShare;
    const handleWebShare = async () => {
        try {
            let url = new URL(canonical)
            url.searchParams.set("utm_medium", "share")
            navigator.share({
                title: data.post.title,
                text: data.post.description,
                url: url.href,
            });
        } catch (error) {
            webShareAPISupported = false;
        }
    };
</script>

<SvelteSeo
    title={data.post.title}
    description={data.post.description}
    {canonical}
    twitter={{
        card: "summary",
        // site: "@primalmovement",
        title: data.post.title,
        description: data.post.description,
        // image: data.post.image
    }}
    openGraph={{
        title: data.post.title,
        description: data.post.description,
    }}
/>

<article class="h-entry">
    <h1 id="title" class="p-name">{data.post.title}</h1>
    <aside>
        <a class="u-url" href={canonical}
            >Published on <time class="dt-published" datetime={data.post.date}
                >{new Date(data.post.date).toLocaleDateString()}</time
            ></a
        >
        · <span>{data.post.readingTime.text}</span>
        {#if webShareAPISupported} · <button class="link" on:click={handleWebShare}
                >Share</button
            >
        {/if}
    </aside>
    <Toc headings={data.post.headings} />
    <!-- {#await GhReleasesDownload}
    
{:then component} 
    <svelte:component this={component} releaseData={data.ghReleaseData} />
{/await} -->

    <div class="e-content">
        <svelte:component this={data.component} />
    </div>
</article>

<style>
    aside {
        font-size: 0.85em;
    }
    aside a {
        color: currentColor;
        text-decoration: unset;
    }
    button.link {
        background: none;
        border: none;
        color: unset;
        padding: 0;
        /* margin: 0; */
        display: inline;
        text-decoration: underline;
        cursor: pointer;
    }
</style>
