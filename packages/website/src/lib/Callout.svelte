<script lang="ts">
    export let calloutType: string;
    import { IconExclamationCircle } from "@tabler/icons-svelte";
</script>

<div class="callout" data-callout={calloutType}>
    <div class="callout-title">
        {#if $$slots.icon}
            <div class="callout-icon"><slot name="icon" /></div>
        {:else}
            <div class="callout-icon"><IconExclamationCircle /></div>
        {/if}
        <div class="callout-title-inner">
            <slot name="title"
                >{calloutType.replace(/\w\S*/g, function (txt) {
                    return (
                        txt.charAt(0).toUpperCase() +
                        txt.substring(1).toLowerCase()
                    );
                })}</slot
            >
        </div>
    </div>
    {#if $$slots.body}
        <div class="callout-body"><slot name="body" /></div>
    {/if}
</div>
