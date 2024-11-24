<script lang="ts">
    import { IconExclamationCircle } from "@tabler/icons-svelte";
    interface Props {
        calloutType: string;
        icon?: import('svelte').Snippet;
        title?: import('svelte').Snippet;
        body?: import('svelte').Snippet;
    }

    let {
        calloutType,
        icon,
        title,
        body
    }: Props = $props();
</script>

<div class="callout" data-callout={calloutType}>
    <div class="callout-title">
        {#if icon}
            <div class="callout-icon">{@render icon?.()}</div>
        {:else}
            <div class="callout-icon"><IconExclamationCircle /></div>
        {/if}
        <div class="callout-title-inner">
            {#if title}{@render title()}{:else}{calloutType.replace(/\w\S*/g, function (txt) {
                    return (
                        txt.charAt(0).toUpperCase() +
                        txt.substring(1).toLowerCase()
                    );
                })}{/if}
        </div>
    </div>
    {#if body}
        <div class="callout-body">{@render body?.()}</div>
    {/if}
</div>
