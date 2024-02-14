<style>
    .hero {
        display: flex;
        justify-content: center;
        flex-direction: column;
        align-items: center;
        gap: var(--spacing);
        margin: 48px auto;
        max-width: 320px;
    }

    .logo {
        width: 160px;
        height: 160px;
        flex-shrink: 0;
    }

    .content {
        display: flex;
  flex-direction: column;
  gap: calc(var(--spacing) / 2);
    }

    .title {
        text-align: center;
        font-size: 32px;
        margin: 0;
    }

    .description {
        text-align: center;
    }

    
    @media screen and (min-width: 540px) {
        .hero {
            flex-direction: row;
            margin: 96px auto;
            max-width: 520px;
        }
        .title, .description {
            text-align: left;
        }
    }

</style>

<script lang="ts">
    import url from "./logo.svg?url"
</script>
<div class="hero">
    <div class="logo">
        <img src="{url}" alt="Logo">
    </div>
    <div class="content">
        <h1 class="title">
            Jade
        </h1>
        <div class="description">
            Finally, a website!
        </div>
    </div>
</div>