<script lang="ts">
    // https://github.com/mattjennings/sveltekit-blog-template/blob/main/src/routes/post/%5Bslug%5D/%2Bpage.svelte
    
    import SvelteSeo from "svelte-seo";
    export let data;
    import { SITE_URL } from '$lib/metadata';
</script>

<SvelteSeo
    title={data.post.title}
    canonical={SITE_URL + "/projects/" + data.post.slug}
/>
<h1>{data.post.title}</h1>
<svelte:component this={data.component} />