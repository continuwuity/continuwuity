<script lang="ts">
    import MenuIcon from "./MenuIcon.svelte";
    import type { Command } from "prosemirror-state";
    export let items: { command: Command; name: string; text: string }[];
    export let onCommand: Function;
</script>

<div class="menubar">
    {#each items as item}
        <button on:click|preventDefault={(e) => onCommand(item.command)}>
            <MenuIcon name={item.name} text={item.text} />
        </button>
    {/each}
</div>

<style>
    @import "../styles.css";

    .menubar {
        max-width: 480px;
        margin: auto;
        padding: 0.5em;
        margin: 1em auto;

        background-color: var(--surface-color);
        border: 0.15em var(--shadow-color) solid;
        border-radius: 0.25em;

        display: flex;
        gap: 0.25em;
    }
    .menubar button {
        background-color: var(--background-color);
        /* border: black 1px solid; */
        border: none;
        border-radius: 0.25em;
        width: 2.5em;
        padding: 0.25em 0.5em;
        font-size: 1em;
    }
    .menubar button:hover {
        background-color: #444;
        color: white;
        border: black 1px solid;
    }
</style>
