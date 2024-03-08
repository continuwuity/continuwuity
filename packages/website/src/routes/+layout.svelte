<script lang="ts">
    import "$lib/styles.css";
    import Favicons from "$lib/Favicons.svelte";
</script>
<svelte:head>
    <Favicons />
</svelte:head>
<main class="main">
    <slot />

</main>