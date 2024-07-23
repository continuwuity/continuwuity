<script lang="ts">
    import "$lib/analytics"
    import "$lib/styles.css";
    import Favicons from "$lib/Favicons.svelte";
    import Nav from "$lib/Nav.svelte";
</script>
<svelte:head>
    <Favicons />
    <link rel="webmention" href="https://webmention.io/jade.ellis.link/webmention" />
</svelte:head>
<Nav />
<main class="main container" id="page-content">
    <slot />

</main>