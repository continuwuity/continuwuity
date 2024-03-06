<script lang="ts">
    export let name: string;
    export let text: string;
</script>

<span class={`menuicon-${name}`} title={name}>
    {text}
</span>
