<script lang="ts">
    // https://github.com/mattjennings/sveltekit-blog-template/blob/main/src/routes/post/%5Bslug%5D/%2Bpage.svelte

    import SvelteSeo from "svelte-seo";
    export let data;
    import { SITE_URL } from "$lib/metadata";
    // let GhReleasesDownload: Promise<any>;
    // if (data.ghReleaseData) {
    //     GhReleasesDownload = import("$lib/GhReleasesDownload.svelte").then((m) => m.default)
    // }

    console.log(data)
</script>

<SvelteSeo
    title={data.post.title}
    description={data.post.description}
    canonical={SITE_URL + "/blog/" + data.post.canonical}
/>
<h1>{data.post.title}</h1>
<!-- {#await GhReleasesDownload}
    
{:then component} 
    <svelte:component this={component} releaseData={data.ghReleaseData} />
{/await} -->

<svelte:component this={data.component} />
