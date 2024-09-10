<script lang="ts">
    // import { page } from "$app/stores";
    import { SITE_URL, SITE_TITLE } from "$lib/metadata";
    import SvelteSeo from "svelte-seo";

    import type { WithContext, Thing } from "schema-dts";
    export let data;
    let { pages } = data;

    const jsonLd = {
        "@context": "https://schema.org",
        "@type": "WebPage",
        name: "Jade's Blog - Posts",
        breadcrumb: {
            "@type": "BreadcrumbList",

            itemListElement: [
                {
                    "@type": "ListItem",
                    position: 1,
                    name: "Blog",
                    item: SITE_URL + "/blog",
                },
            ],
        },
        mainEntity: {
            "@type": "Blog",
            "@id": SITE_URL + "/blog",
            name: "Jade's Blog",
            mainEntityOfPage: SITE_URL + "/blog",
        },
    } as WithContext<Thing>;
</script>

<svelte:head>
    <link
        rel="alternate"
        type="application/rss+xml"
        title={SITE_TITLE}
        href={SITE_URL + "/blog/rss.xml"}
    />
    <link
        rel="alternate"
        type="application/feed+json"
        title={SITE_TITLE}
        href={SITE_URL + "/blog/feed.json"}
    />

    {@html `<script type="application/ld+json">${
        JSON.stringify(jsonLd) + "<"
    }/script>`}
</svelte:head>

<SvelteSeo title="Jade's Blog - Posts" canonical={SITE_URL + "/blog"} />

<main class="main container" id="page-content">
    <section role="feed" class="h-feed" id="feed">
        <h1 class="p-name">
            <a
                aria-hidden="true"
                tabindex="-1"
                class="u-url permalink"
                href={SITE_URL + "/blog#feed"}>#</a
            >Jade's Blog - Posts
        </h1>
        {#each pages as post, index}
            <article
                aria-posinset={index + 1}
                aria-setsize={pages.length}
                class="h-entry"
            >
                <div class="content" data-sveltekit-preload-data="hover">
                    <h2>
                        <a class="u-url p-name" href="/blog/{post.canonical}">
                            {post.title}
                        </a>
                    </h2>
                    <span class="quiet"
                        ><time class="dt-published" datetime={post.date}
                            >{new Date(post.date).toLocaleDateString()}</time
                        ></span
                    >
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
</main>

<style>
    .permalink {
        opacity: 0;
        width: 1.4em;
        height: 1em;
        transition: opacity 0.2s;
        display: block;
        /* bottom: 0.25em; */
        text-decoration: none;
        left: -1em;
        position: absolute !important;
    }
    h1 {
        position: relative;
    }
    h1:hover .permalink {
        opacity: 1;
    }
</style>
