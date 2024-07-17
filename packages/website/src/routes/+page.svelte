<script lang="ts">
    import Hero from "$lib/Hero.svelte";
    import SvelteSeo from "svelte-seo";
    import Homepage from "Notes/Website Homepage.md";
    import { SITE_URL, SITE_TITLE } from "$lib/metadata";
</script>

<svelte:head>
    <link rel="alternate" type="application/rss+xml" title={SITE_TITLE} href={SITE_URL + "/blog/rss.xml"}>
    <link rel="alternate" type="application/feed+json" title={SITE_TITLE} href={SITE_URL + "/blog/feed.json"}>
</svelte:head>

<SvelteSeo
  title="Jade Ellis"
  description="Student, Creative & Computer Scientist. See what I'm doing."
  canonical="https://jade.ellis.link"
/>
<Hero />

<Homepage />