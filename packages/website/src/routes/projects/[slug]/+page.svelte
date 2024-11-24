<script lang="ts">
    // https://github.com/mattjennings/sveltekit-blog-template/blob/main/src/routes/post/%5Bslug%5D/%2Bpage.svelte

    import SvelteSeo from "svelte-seo";
    import { SITE_URL } from "$lib/metadata";
    import GhReleasesDownload from "$lib/GhReleasesDownload.svelte";
    interface Props {
        data: any;
    }

    let { data }: Props = $props();
    // let GhReleasesDownload: Promise<any>;
    // if (data.ghReleaseData) {
    //     GhReleasesDownload = import("$lib/GhReleasesDownload.svelte").then((m) => m.default)
    // }

    // console.log(data.ghReleaseData)
</script>

<SvelteSeo
    title={data.post.title}
    description={data.post.description}
    canonical={SITE_URL + "/projects/" + data.post.slug}
/>

<main class="main container" id="page-content">
    <h1>{data.post.title}</h1>
    <!-- {#await GhReleasesDownload}
    
{:then component} 
    <svelte:component this={component} releaseData={data.ghReleaseData} />
{/await} -->

    {#if data.ghReleaseData}
        <GhReleasesDownload releaseData={data.ghReleaseData} />
    {/if}

    <data.component />
</main>
