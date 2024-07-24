<script lang="ts">
    // https://github.com/mattjennings/sveltekit-blog-template/blob/main/src/routes/post/%5Bslug%5D/%2Bpage.svelte

    import { browser } from "$app/environment";
    import SvelteSeo from "svelte-seo";
    export let data;
    import { SITE_URL, SITE_TITLE } from "$lib/metadata";
    import Toc from "$lib/Toc.svelte";
    import type { WithContext, Thing } from "schema-dts";
    import pfpUrl from "$lib/logo.svg?url";
    // let GhReleasesDownload: Promise<any>;
    // if (data.ghReleaseData) {
    //     GhReleasesDownload = import("$lib/GhReleasesDownload.svelte").then((m) => m.default)
    // }
    $: canonical = SITE_URL + "/blog/" + data.post.canonical;

    function calcOgURL(
        slug: string,
        date: string,
        ratio?: number,
        width?: number,
    ): URL {
        let url = new URL(SITE_URL + "/blog/image");
        url.searchParams.set("slug", slug);
        url.searchParams.set("date", date);
        if (ratio) {
            url.searchParams.set("ratio", ratio.toString());
        }
        if (width) {
            url.searchParams.set("width", width.toString());
        }
        return url;
    }

    $: webShareAPISupported = browser && typeof navigator.share !== "undefined";
    // let webShareAPISupported = true;

    $: handleWebShare;
    const handleWebShare = async () => {
        try {
            let url = new URL(canonical);
            url.searchParams.set("utm_medium", "share");
            navigator.share({
                title: data.post.title,
                text: data.post.description,
                url: url.href,
            });
        } catch (error) {
            webShareAPISupported = false;
        }
    };

    const defaultAuthor = {
        "@type": "Person",
        name: "Jade Ellis",
        url: "https://jade.ellis.link",
        fediverse: "@JadedBlueEyes@tech.lgbt",
        image: pfpUrl,
    };
    $: jsonLd = {
        "@context": "https://schema.org",
        "@type": "WebPage",
        breadcrumb: {
            "@type": "BreadcrumbList",

            itemListElement: [
                {
                    "@type": "ListItem",
                    position: 1,
                    name: "Blog",
                    item: SITE_URL + "/blog",
                },
                {
                    "@type": "ListItem",
                    position: 2,
                    name: data.post.title,
                    item: canonical,
                },
            ],
        },
        mainEntity: {
            "@type": "BlogPosting",
            "@id": canonical,
            url: canonical,
            mainEntityOfPage: canonical,
            name: data.post.title,
            headline: data.post.title,
            datePublished: new Date(data.post.date).toISOString(),
            author: defaultAuthor,
            description: data.post.description,
            wordCount: data.post.readingTime.words,
            image: {
                "@type": "ImageObject",
                width: "1200",
                height: "630",
                url: calcOgURL(
                    data.post.slug,
                    data.post.date,
                    630 / 1200,
                    1200,
                ).toString(),
            },
            isPartOf: {
                "@type": "Blog",
                "@id": SITE_URL + "/blog",
                name: "Jade's Blog",
            },
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
    {#if defaultAuthor?.fediverse}
        <meta name="fediverse:creator" content={defaultAuthor?.fediverse} />
    {/if}
    <meta
        property="og:image"
        content={calcOgURL(
            data.post.slug,
            data.post.date,
            630 / 1200,
            1200,
        ).toString()}
    />
    <meta property="og:image:width" content="1200" />
    <meta property="og:image:height" content="630" />
    <meta property="og:image:type" content="image/png" />
    {@html `<script type="application/ld+json">${
        JSON.stringify(jsonLd) + "<"
    }/script>`}
</svelte:head>

<SvelteSeo
    title={data.post.title}
    description={data.post.description}
    {canonical}
    twitter={{
        card: "summary_large_image",
        // site: "@primalmovement",
        title: data.post.title,
        description: data.post.description,
        image: calcOgURL(
            data.post.slug,
            data.post.date,
            630 / 1200,
            1200,
        ).toString(),
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
        <span class="author p-author h-card vcard">
            by <img
                loading="lazy"
                style="display: none;"
                src={defaultAuthor.image}
                class="avatar avatar-96 photo u-photo"
            /><a class="u-url url fn n p-name" href={defaultAuthor.url}
                >{defaultAuthor.name}</a
            ></span
        >
        · <span>{data.post.readingTime.text}</span>
        {#if webShareAPISupported}
            · <button class="link" on:click={handleWebShare}>Share</button>
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
