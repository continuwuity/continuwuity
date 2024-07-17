<script lang="ts">
    import { page } from "$app/stores";
    import { SITE_URL, SITE_TITLE } from "$lib/metadata";
    import SvelteSeo from "svelte-seo";

    export let data;
    let { pages } = data;
    // $: console.log(data);
</script>

<svelte:head>
    <link rel="alternate" type="application/rss+xml" title={SITE_TITLE} href={SITE_URL + "/blog/rss.xml"}>
    <link rel="alternate" type="application/feed+json" title={SITE_TITLE} href={SITE_URL + "/blog/feed.json"}>
</svelte:head>

<SvelteSeo
    title=""
    canonical={SITE_URL + "/blog"} />

<section role="feed" class="h-feed" id="feed">
    <h1 class="p-name">Blog Posts <a class="u-url" href="/#feed"></a></h1>
    {#each pages as post, index}
        <article aria-posinset={index + 1} aria-setsize={pages.length} class="h-entry">
            <div class="content" data-sveltekit-preload-data="hover">
                <h2>
                    <a class="u-url p-name" href="/blog/{post.canonical}">
                        {post.title}
                    </a>
                </h2>
                <span class="quiet"><time class="dt-published" datetime={post.date}>{new Date(post.date).toLocaleDateString()}</time></span>
                {#if post.description}
                <p class="p-summary">{post.description}</p>
                {/if}
            </div>
        </article>
    {:else}
        <p>No posts yet!</p>
    {/each}
    <!-- {#if showPosts < postCount}
		<button type="submit" on:click={handleClick}>See more {H_ELLIPSIS_ENTITY}</button>
	{/if} -->
</section>
