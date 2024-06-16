<script lang="ts">
    import "$lib/analytics"
    import "$lib/styles.css";
    import Favicons from "$lib/Favicons.svelte";
    import Nav from "$lib/Nav.svelte";
</script>
<svelte:head>
    <Favicons />
</svelte:head>
<Nav />
<main class="main container" id="page-content">
    <slot />

</main>