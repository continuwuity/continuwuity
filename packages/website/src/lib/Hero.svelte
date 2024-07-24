<script lang="ts">
    import url from "./logo.svg?url";
    import { SITE_URL } from "$lib/metadata";
</script>

<div class="hero card edge h-card">
    <div class="logo">
        <a href={SITE_URL} class="u-url u-uid" rel="me"><img class="u-photo" src={url} alt="Logo" /></a>
    </div>
    <div class="content">
        <div>
        <h1 class="title p-name"><span class="p-given-name">Jade</span> <span class="p-family-name">Ellis</span></h1>
        <div role="doc-subtitle"><span class="p-nickname">JadedBlueEyes</span></div></div>
        <div class="description p-note">Student, Computer Scientist and Creative</div>
    </div>
</div>

<style>
    .hero {
        display: flex;
        justify-content: center;
        flex-direction: column;
        align-items: center;
        gap: var(--spacing);
        margin: 48px auto;
        max-width: 320px;
        padding: 3rem .5rem;
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
    [role="doc-subtitle"] { 
        padding-block-start: 0;
        font-size: 18px;
        font-weight: 600;
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
        .title,
        [role="doc-subtitle"],
        .description {
            text-align: left;
        }
    }
</style>
