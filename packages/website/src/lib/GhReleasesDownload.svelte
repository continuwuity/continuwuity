<script lang="ts">
    import type { Endpoints } from "@octokit/types";

    export let releaseData: Endpoints["GET /repos/{owner}/{repo}/releases/latest"]["response"]["data"];
    import { browser } from "$app/environment";
    // console.log(releaseData);
</script>

<div class="release">
    {#if releaseData?.assets && browser && navigator}
        {#if navigator.platform.startsWith("Win")}
            {@const asset = (releaseData?.assets || []).filter((a) =>
                a.name.endsWith(".exe"),
            )[0]}
            {#if asset}
                <a href={asset.browser_download_url}>Download for Windows</a>
            {/if}
        {:else if navigator.platform.startsWith("Mac")}
            {@const asset = (releaseData?.assets || []).filter((a) =>
                a.name.endsWith(".dmg"),
            )[0]}
            {#if asset}
                <a href={asset.browser_download_url}>Download for MacOS</a>
            {/if}
        {:else if navigator.platform.startsWith("Linux") && navigator.platform.includes("x86_64")}
            {@const asset = (releaseData?.assets || []).filter((a) =>
                a.name.endsWith(".AppImage"),
            )[0]}
            {#if asset}
                <a href={asset.browser_download_url}>Download AppImage</a>
            {/if}
        {:else}
            <span>No download found for your device</span>
        {/if}
    {:else if releaseData?.assets}
        <span>Finding download for your device</span>
    {/if}
    <p>Latest release: <a href={releaseData.html_url}>{releaseData.name}</a></p>
</div>
