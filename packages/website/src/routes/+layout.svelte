<script lang="ts">
    import "$lib/analytics"
    import "$lib/styles.css";
    import Favicons from "$lib/Favicons.svelte";
    import Nav from "$lib/Nav.svelte";
    import Footer from "$lib/Footer.svelte";
    import { SITE_TITLE } from "$lib/metadata"
</script>
<svelte:head>
    <Favicons />
    <link rel="webmention" href="https://webmention.io/jade.ellis.link/webmention" />
    <meta property="og:site_name" content={SITE_TITLE}>
</svelte:head>
<Nav />
<slot />
<Footer />