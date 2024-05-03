<script lang="ts">
    import type { Endpoints } from "@octokit/types";

    export let releaseData: Endpoints["GET /repos/{owner}/{repo}/releases/latest"]["response"]["data"];
    // console.log(releaseData);
</script>

<div class="release">
    {#if navigator}
        {#if navigator.platform.startsWith("Win")}
            {@const asset  =releaseData.assets.filter((a) => a.name.endsWith(".exe"))[0]}
            {#if asset}
                <a href={asset.browser_download_url}>Download for Windows</a>
            {/if}
        {:else if navigator.platform.startsWith("Mac")}
        {@const asset  =releaseData.assets.filter((a) => a.name.endsWith(".dmg"))[0]}
        {#if asset}
            <a href={asset.browser_download_url}>Download for MacOS</a>
        {/if}

        {:else if navigator.platform.startsWith("Linux")}

        {@const asset  =releaseData.assets.filter((a) => a.name.endsWith(".AppImage"))[0]}
        {#if asset}
            <a href={asset.browser_download_url}>Download AppImage</a>
        {/if}
        {/if}
    {/if}
    <p>Latest release: <a href={releaseData.html_url}>{releaseData.name}</a></p>
</div>
